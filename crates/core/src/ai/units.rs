//! How each computer unit chooses what to do.
//!
//! Most units search outward from where they stand, score every target the
//! search reaches by value and distance, and take the first step towards
//! the best one. Searches run on the player's own map, so units can only go
//! for what their side has seen. Several units can pick the same city to
//! attack, but each target accepts only a few; exploring units claim a
//! patch of ground so they spread out.

use super::field::{Bfs, Cell};
use super::plan::Plan;
use crate::game::Game;
use crate::geom::{Dir, Geometry, Loc};
use crate::movement::MoveKind;
use crate::types::{MapVal, PlayerId, Terrain, UnitType};
use crate::unit::{Mission, Orders, Task, UnitId};

/// What a unit is worth, in turns of production. Enemy transports are
/// assumed to carry a few armies.
fn worth(game: &Game, kind: UnitType) -> f64 {
    let base = game.rules.stats(kind).build_next as f64;
    if kind == UnitType::Transport {
        base + 15.0
    } else {
        base
    }
}

fn unknown_around(view: &[MapVal], geom: &Geometry, loc: Loc) -> u32 {
    geom.neighbors(loc).filter(|&(_, n)| !geom.is_border(n) && view[n].is_unknown()).count() as u32
}

/// What a search found worth going for.
#[derive(Clone, Copy)]
struct Goal {
    score: f64,
    loc: Loc,
    /// Claim this many squares around the goal, so others look elsewhere.
    radius: usize,
}

fn better(best: &mut Option<Goal>, g: Goal) {
    if best.is_none_or(|b| g.score > b.score) {
        *best = Some(g);
    }
}

impl Game {
    // ------------------------------------------------------------------
    // Shared helpers

    fn ai_spin(&self, id: UnitId) -> u8 {
        (self.units[id].serial % 8) as u8
    }

    /// A non-city square holding one of `p`'s units (units can't share those).
    fn ai_own_occupied(&self, p: PlayerId, loc: Loc) -> bool {
        self.map.get(loc) != Terrain::City && self.at[loc].iter().any(|&o| self.units[o].owner == p)
    }

    /// One of `p`'s transports waiting for armies here, with room aboard.
    fn ai_boardable(&self, p: PlayerId, loc: Loc) -> bool {
        self.map.get(loc) == Terrain::Sea
            && self.at[loc].iter().any(|&o| {
                let s = &self.units[o];
                s.owner == p
                    && s.kind == UnitType::Transport
                    && matches!(s.mission.task, Task::Gather | Task::Free)
                    && !self.is_full(o)
            })
    }

    fn ai_near_own_city(&self, p: PlayerId, loc: Loc, radius: usize) -> bool {
        let g = self.map.geom;
        self.cities.iter().any(|c| c.owner == p && g.dist(c.loc, loc) <= radius)
    }

    /// Should unit `id` attack an enemy of this kind? Expert weighs the chance
    /// of winning against what each side stands to lose; Standard attacks
    /// whenever the odds look fair.
    fn ai_should_attack(&self, plan: &Plan, id: UnitId, kind: UnitType, owner: PlayerId, bombard: bool, bonus: f64) -> bool {
        let pw = self.win_chance(id, kind, owner, bombard);
        if !plan.style.weigh_odds {
            return pw >= 0.4;
        }
        let mine = worth(self, self.units[id].kind);
        let theirs = worth(self, kind) + bonus;
        pw >= 0.2 && pw * theirs > (1.0 - pw) * mine
    }

    fn ai_step(&mut self, id: UnitId, d: Dir) -> bool {
        self.step_unit(id, d).is_ok() && self.units[id].alive
    }

    fn ai_set_mission(&mut self, id: UnitId, task: Task, target: Option<Loc>) {
        let turn = self.turn;
        let m = &mut self.units[id].mission;
        if m.task != task || m.target != target {
            m.since = turn;
        }
        m.task = task;
        m.target = target;
    }

    // ------------------------------------------------------------------
    // Armies

    pub(super) fn ai_army(&mut self, plan: &mut Plan, id: UnitId) {
        if self.units[id].moves_left == 0 {
            return;
        }
        if self.carrier_of(id).is_some() {
            self.ai_army_aboard(plan, id);
            return;
        }
        if let Some(d) = self.ai_army_strike(plan, id) {
            self.ai_step(id, d);
            return;
        }
        let geom = self.map.geom;
        let (p, loc) = (self.units[id].owner, self.units[id].loc);
        let style = plan.style;
        let cont = plan.cont_id(loc);
        // Armies beyond what the continent needs look for a ride instead.
        let need_left = cont.map(|c| plan.continents[c].need() as i64 - plan.assigned[c] as i64).unwrap_or(0);
        let pickup = cont.and_then(|c| plan.continents[c].pickup);
        let mut bfs = std::mem::take(&mut plan.bfs);
        {
            let view = &self.player(p).view;
            bfs.run(&geom, &[loc], 250, self.ai_spin(id), |n| {
                let v = view[n];
                if v.is_unknown() {
                    Cell::Block
                } else if v.is_city() {
                    if v.owner() == p {
                        Cell::Open
                    } else {
                        Cell::Stop
                    }
                } else if v.is_land() {
                    if v.is_enemy_of(p) {
                        Cell::Stop
                    } else if geom.dist(loc, n) == 1 && self.ai_own_occupied(p, n) {
                        Cell::Block
                    } else {
                        Cell::Open
                    }
                } else if self.ai_boardable(p, n) {
                    Cell::Stop
                } else {
                    Cell::Block
                }
            });
        }
        let view = &self.player(p).view;
        let score_of = |value: f64, d: u32| if style.values { value / (d as f64 + 1.0) } else { 1000.0 - d as f64 + value / 1000.0 };
        // Targets on this continent: cities, enemy armies, unexplored ground.
        let mut real: Option<Goal> = None;
        let mut real_is_attack = false;
        // Ways off the continent: a transport, the pickup port, the beach.
        let mut ride: Option<Goal> = None;
        for (cell, d) in bfs.reached() {
            let v = view[cell];
            let claims = plan.claims[cell] as u32;
            if d > 0 && v.is_city() && v.owner() != p {
                let limit = if v.owner() == 0 { 2 } else { 3 };
                if claims < limit {
                    let value = (if v.owner() == 0 { 100.0 } else { 120.0 }) / (1 + claims) as f64;
                    let g = Goal { score: score_of(value, d), loc: cell, radius: 0 };
                    if real.is_none_or(|b| g.score > b.score) {
                        real = Some(g);
                        real_is_attack = true;
                    }
                }
            } else if d > 0 && v.is_enemy_of(p) && v.is_land() {
                if v.unit_type() == Some(UnitType::Army) && claims < 2 {
                    let value = if self.ai_near_own_city(p, cell, 3) { 90.0 } else { 40.0 };
                    let g = Goal { score: score_of(value, d), loc: cell, radius: 0 };
                    if real.is_none_or(|b| g.score > b.score) {
                        real = Some(g);
                        real_is_attack = true;
                    }
                }
            } else if d > 0 && v.is_sea() {
                // A transport waiting for armies.
                better(&mut ride, Goal { score: score_of(80.0, d), loc: cell, radius: 0 });
            } else if v.is_land() || v.is_city() {
                if let Some(pk) = pickup {
                    let waiting_spot = if pk.port { pk.loc == cell } else { geom.dist(pk.loc, cell) == 1 && v.is_land() };
                    if waiting_spot {
                        better(&mut ride, Goal { score: score_of(45.0, d), loc: cell, radius: 0 });
                    }
                }
                if d > 0 && v.is_land() && claims == 0 {
                    let unknown = unknown_around(view, &geom, cell);
                    if unknown > 0 {
                        let g = Goal { score: score_of(25.0 + 4.0 * unknown as f64, d), loc: cell, radius: 2 };
                        if real.is_none_or(|b| g.score > b.score) {
                            real = Some(g);
                            real_is_attack = false;
                        }
                    }
                }
            }
        }
        let choice = match (real, ride) {
            (Some(r), _) if real_is_attack || need_left > 0 => {
                if let Some(c) = cont {
                    plan.assigned[c] += 1;
                }
                Some(r)
            }
            (_, Some(r)) => Some(r),
            (Some(r), None) => Some(r),
            (None, None) => None,
        };
        let step = choice.filter(|g| g.loc != loc).and_then(|g| {
            plan.claim(self, g.loc, g.radius);
            bfs.first_step(g.loc)
        });
        plan.bfs = bfs;
        if let Some(d) = step {
            self.ai_step(id, d);
        }
    }

    /// An attack on an adjacent square: enemy and neutral cities always,
    /// enemy units when the odds are good enough.
    fn ai_army_strike(&self, plan: &Plan, id: UnitId) -> Option<Dir> {
        let u = &self.units[id];
        let p = u.owner;
        let view = &self.player(p).view;
        let geom = self.map.geom;
        let mut best: Option<(f64, Dir)> = None;
        for (d, n) in geom.neighbors(u.loc) {
            let v = view[n];
            let score = if v.is_city() && v.owner() != p {
                1000.0
            } else if let (Some(k), true) = (v.unit_type(), v.is_enemy_of(p)) {
                if v.is_land() {
                    let bonus = if self.ai_near_own_city(p, n, 2) { 10.0 } else { 0.0 };
                    if !self.ai_should_attack(plan, id, k, v.owner(), false, bonus) {
                        continue;
                    }
                    100.0 + worth(self, k)
                } else if plan.style.weigh_odds && k == UnitType::Transport {
                    // The army drowns whatever happens, so only a loaded
                    // transport is worth it.
                    let pw = self.win_chance(id, k, v.owner(), false);
                    if pw * worth(self, k) <= worth(self, UnitType::Army) {
                        continue;
                    }
                    50.0
                } else {
                    continue;
                }
            } else {
                continue;
            };
            if self.classify_move(id, d).is_err() {
                continue;
            }
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, d));
            }
        }
        best.map(|(_, d)| d)
    }

    /// An army riding a transport: go ashore once the transport has brought
    /// it somewhere worth landing.
    fn ai_army_aboard(&mut self, plan: &mut Plan, id: UnitId) {
        let Some(ship) = self.carrier_of(id) else { return };
        let task = self.units[ship].mission.task;
        if !matches!(task, Task::Deliver | Task::Unload) {
            return;
        }
        if let Some(d) = self.ai_army_strike(plan, id) {
            self.ai_step(id, d);
            return;
        }
        let origin = self.units[ship].mission.origin.and_then(|o| plan.shore_continent(self, o));
        let geom = self.map.geom;
        let (p, loc) = (self.units[id].owner, self.units[id].loc);
        let view = &self.player(p).view;
        let mut best: Option<(i64, Dir)> = None;
        for (d, n) in geom.neighbors(loc) {
            let v = view[n];
            if !v.is_land() || v.is_unit() {
                continue;
            }
            let Some(c) = plan.cont_id(n) else { continue };
            let cont = &plan.continents[c];
            if Some(c) == origin || !cont.worth_landing() || (cont.own_armies >= cont.need() && cont.targets() == 0) {
                continue;
            }
            if self.classify_move(id, d) != Ok(MoveKind::Move) {
                continue;
            }
            let city = self
                .cities
                .iter()
                .filter(|cy| view[cy.loc].is_city() && view[cy.loc].owner() != p && plan.cont_id(cy.loc) == Some(c))
                .map(|cy| geom.dist(cy.loc, n))
                .min()
                .unwrap_or(30) as i64;
            let score = 3 * (30 - city.min(30)) + 2 * unknown_around(view, &geom, n) as i64;
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, d));
            }
        }
        if let Some((_, d)) = best {
            let to = geom.step(loc, d).expect("adjacent");
            if self.ai_step(id, d) {
                if let Some(c) = plan.cont_id(to) {
                    plan.continents[c].own_armies += 1;
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Fighters

    /// The most unexplored patch of map a fighter at `loc` can reach on its
    /// remaining fuel: the centre of the 5x5 block with the most unknown squares.
    fn ai_scout_target(&self, p: PlayerId, loc: Loc, fuel: u32) -> Option<(Loc, u32)> {
        let geom = self.map.geom;
        let view = &self.player(p).view;
        let (r0, c0) = (geom.row(loc) as i64, geom.col(loc) as i64);
        let reach = fuel as i64 - 1;
        let mut best: Option<(u32, Loc)> = None;
        let mut r = r0 - reach;
        while r <= r0 + reach {
            let mut c = c0 - reach;
            while c <= c0 + reach {
                if r >= 1 && c >= 1 && (r as usize) < geom.height - 1 && (c as usize) < geom.width - 1 {
                    let mut unknown = 0;
                    for rr in (r - 2).max(1)..=(r + 2).min(geom.height as i64 - 2) {
                        for cc in (c - 2).max(1)..=(c + 2).min(geom.width as i64 - 2) {
                            if view[geom.loc(rr as usize, cc as usize)].is_unknown() {
                                unknown += 1;
                            }
                        }
                    }
                    let l = geom.loc(r as usize, c as usize);
                    // Prefer far, unknown patches.
                    let score = unknown * 4 + geom.dist(loc, l) as u32;
                    if unknown >= 15 && best.is_none_or(|(b, _)| score > b) {
                        best = Some((score, l));
                    }
                }
                c += 3;
            }
            r += 3;
        }
        best.map(|(s, l)| (l, s))
    }

    /// A fighter on a one-way scouting flight: head for the unexplored
    /// patch, then the next one, until the tank runs dry.
    fn ai_fighter_scout(&mut self, id: UnitId) {
        let geom = self.map.geom;
        for _ in 0..8 {
            let u = &self.units[id];
            if !u.alive || u.moves_left == 0 {
                return;
            }
            let (p, loc, fuel) = (u.owner, u.loc, u.fuel as u32);
            let mut target = u.mission.target.filter(|&t| t != loc);
            if target.is_none() {
                target = self.ai_scout_target(p, loc, fuel).map(|(l, _)| l);
                self.units[id].mission.target = target;
            }
            let Some(t) = target else {
                // Nothing left to look at: back to ordinary duty.
                self.ai_set_mission(id, Task::Free, None);
                return;
            };
            // Straight towards the patch, sidestepping anything in the way.
            let base = geom.dir_towards(loc, t).unwrap_or(0);
            let step = [0, 1, -1, 2, -2]
                .into_iter()
                .map(|k| crate::geom::rotate(base, k))
                .find(|&d| matches!(self.classify_move(id, d), Ok(MoveKind::Move)));
            match step {
                Some(d) if self.ai_step(id, d) => {}
                _ => {
                    self.ai_set_mission(id, Task::Free, None);
                    return;
                }
            }
        }
    }

    pub(super) fn ai_fighter(&mut self, plan: &mut Plan, id: UnitId) {
        let geom = self.map.geom;
        if self.units[id].mission.task == Task::Scout {
            self.ai_fighter_scout(id);
            if self.units[id].mission.task == Task::Scout || !self.units[id].alive {
                return;
            }
        }
        // Send a fighter with a full tank on a scouting flight?
        if plan.scouts > 0 && self.in_city(id) && self.units[id].fuel >= self.rules.fighter_fuel() {
            let (p, loc, fuel) = (self.units[id].owner, self.units[id].loc, self.units[id].fuel as u32);
            if let Some((t, _)) = self.ai_scout_target(p, loc, fuel) {
                if geom.dist(loc, t) as u32 > fuel / 2 {
                    plan.scouts -= 1;
                    self.ai_set_mission(id, Task::Scout, Some(t));
                    self.ai_fighter_scout(id);
                    return;
                }
            }
        }
        for _ in 0..12 {
            let u = &self.units[id];
            if !u.alive || u.moves_left == 0 {
                return;
            }
            let (p, loc, fuel, moves) = (u.owner, u.loc, u.fuel as u32, u.moves_left as u32);
            let full = self.rules.fighter_fuel() as u32;
            let in_city = self.in_city(id);
            let mut bfs = std::mem::take(&mut plan.bfs);
            {
                let view = &self.player(p).view;
                bfs.run(&geom, &[loc], fuel, self.ai_spin(id), |n| {
                    let v = view[n];
                    if v.is_city() {
                        if v.owner() == p {
                            Cell::Stop
                        } else {
                            Cell::Block
                        }
                    } else if v.is_enemy_of(p) {
                        Cell::Stop
                    } else if geom.dist(loc, n) == 1 && self.ai_own_occupied(p, n) {
                        Cell::Block
                    } else {
                        Cell::Open
                    }
                });
            }
            let view = &self.player(p).view;
            let mut best: Option<Goal> = None;
            // A loaded transport is worth more than a fighter, so Expert
            // fighters will go after one even on a one-way trip.
            let mut one_way: Option<Loc> = None;
            for (cell, d) in bfs.reached() {
                if d == 0 {
                    continue;
                }
                let v = view[cell];
                let own_city = v.is_city() && v.owner() == p;
                let claims = plan.claims[cell];
                let returnable = own_city || plan.refuel[cell] as u32 <= fuel.saturating_sub(d);
                if !returnable {
                    let transport = v.unit_type() == Some(UnitType::Transport) && v.is_enemy_of(p);
                    if transport && plan.style.weigh_odds && claims < 2 {
                        let pw = self.win_chance(id, UnitType::Transport, v.owner(), false);
                        if pw * worth(self, UnitType::Transport) > worth(self, UnitType::Fighter) {
                            let g = Goal { score: 2.0 * pw * worth(self, UnitType::Transport) / (d as f64 + 1.0), loc: cell, radius: 0 };
                            if best.is_none_or(|b| g.score > b.score) {
                                best = Some(g);
                                one_way = Some(cell);
                            }
                        }
                    }
                    // Otherwise never go anywhere the tank can't get back from.
                    continue;
                }
                let (value, radius) = if own_city {
                    (if fuel < full { 1.0 } else { 0.0 }, 0)
                } else if let (Some(k), true) = (v.unit_type(), v.is_enemy_of(p)) {
                    let bonus = if self.ai_near_own_city(p, cell, 3) { 15.0 } else { 0.0 };
                    // Strike what can be reached this turn; chase transports.
                    let chase = k == UnitType::Transport && plan.style.weigh_odds;
                    if (d > moves && !chase) || claims >= 2 || !self.ai_should_attack(plan, id, k, v.owner(), false, bonus) {
                        (0.0, 0)
                    } else {
                        (2.0 * (worth(self, k) + bonus), 0)
                    }
                } else if claims == 0 {
                    let unknown = unknown_around(view, &geom, cell);
                    let explore = if unknown >= 2 { 5.0 * unknown as f64 } else { 0.0 };
                    (explore.max(plan.patrol[cell] as f64), 3)
                } else {
                    (0.0, 0)
                };
                if value <= 0.0 {
                    continue;
                }
                let g = Goal { score: value / (d as f64 + 1.0), loc: cell, radius };
                if best.is_none_or(|b| g.score > b.score) {
                    best = Some(g);
                    one_way = None;
                }
            }
            if best.is_none() && in_city {
                plan.bfs = bfs;
                return;
            }
            let mut step = best.and_then(|g| {
                plan.claim(self, g.loc, g.radius);
                bfs.first_step(g.loc)
            });
            plan.bfs = bfs;
            // Check the step itself, and fall back to heading home.
            let safe = |l: Loc| {
                let v = view[l];
                (v.is_city() && v.owner() == p) || (plan.refuel[l] as u32) < fuel
            };
            if one_way.is_none() && !step.and_then(|d| geom.step(loc, d)).is_some_and(safe) {
                step = geom
                    .neighbors(loc)
                    .filter(|&(d, n)| safe(n) && self.classify_move(id, d).is_ok() && !view[n].is_enemy_of(p))
                    .min_by_key(|&(_, n)| plan.refuel[n])
                    .map(|(d, _)| d);
                if step.is_none() && !in_city {
                    // Out of range of every base: keep looking around while the fuel lasts.
                    self.ai_set_mission(id, Task::Scout, None);
                    self.ai_fighter_scout(id);
                    return;
                }
            }
            match step {
                Some(d) if self.ai_step(id, d) => {}
                _ => return,
            }
        }
    }

    // ------------------------------------------------------------------
    // Ships

    /// Search the sea outward from a ship. With `fight`, enemy ships (and,
    /// for ships that bombard, enemy armies on the shore) are targets the
    /// search can reach; otherwise they are obstacles.
    fn ai_sea_search(&self, bfs: &mut Bfs, id: UnitId, max: u32, fight: bool) {
        self.ai_sea_search_avoiding(bfs, id, max, fight, None);
    }

    /// As `ai_sea_search`, also keeping out of squares marked in `avoid`.
    fn ai_sea_search_avoiding(&self, bfs: &mut Bfs, id: UnitId, max: u32, fight: bool, avoid: Option<(&[u8], Loc)>) {
        let u = &self.units[id];
        let (p, loc) = (u.owner, u.loc);
        let bombard = fight && self.rules.stats(u.kind).bombards;
        let view = &self.player(p).view;
        let geom = self.map.geom;
        bfs.run(&geom, &[loc], max, self.ai_spin(id), |n| {
            let v = view[n];
            if v.is_unknown() {
                Cell::Open
            } else if v.is_city() {
                if v.owner() == p && self.city_at(n).is_some_and(|c| self.cities[c].port) {
                    Cell::Stop
                } else {
                    Cell::Block
                }
            } else if v.is_land() {
                if bombard && v.is_enemy_of(p) && v.unit_type() == Some(UnitType::Army) {
                    Cell::Stop
                } else {
                    Cell::Block
                }
            } else if v.is_enemy_of(p) {
                if fight {
                    Cell::Stop
                } else {
                    Cell::Block
                }
            } else if (geom.dist(loc, n) == 1 && self.ai_own_occupied(p, n))
                || avoid.is_some_and(|(a, target)| a[n] > 0 && geom.dist(n, target) > 4)
            {
                Cell::Block
            } else {
                Cell::Open
            }
        });
    }

    /// Take one step towards `target`. False if there is no way or the step failed.
    /// Loaded transports (Expert) steer clear of known enemy warships and fighters.
    fn ai_sail_toward(&mut self, plan: &mut Plan, id: UnitId, target: Loc, fight: bool) -> bool {
        let mut bfs = std::mem::take(&mut plan.bfs);
        let careful = plan.style.weigh_odds && self.units[id].kind == UnitType::Transport && !self.cargo(id).is_empty();
        let mut step = None;
        if careful {
            self.ai_sea_search_avoiding(&mut bfs, id, 400, fight, Some((&plan.danger, target)));
            step = bfs.first_step(target);
        }
        if step.is_none() {
            self.ai_sea_search(&mut bfs, id, 400, fight);
            step = bfs.first_step(target);
        }
        plan.bfs = bfs;
        match step {
            Some(d) => self.ai_step(id, d),
            None => false,
        }
    }

    fn ai_nearest_port(&self, plan: &mut Plan, id: UnitId) -> Option<Loc> {
        let p = self.units[id].owner;
        let mut bfs = std::mem::take(&mut plan.bfs);
        self.ai_sea_search(&mut bfs, id, 400, false);
        let port = bfs.reached().find(|&(l, _)| self.city_at(l).is_some_and(|c| self.cities[c].owner == p)).map(|(l, _)| l);
        plan.bfs = bfs;
        port
    }

    /// Head for port and wait there until fully repaired. Returns true when
    /// the ship is ready for other work.
    fn ai_repair(&mut self, plan: &mut Plan, id: UnitId) -> bool {
        let max = self.rules.stats(self.units[id].kind).hits;
        if self.in_city(id) {
            if self.units[id].hits >= max {
                self.ai_set_mission(id, Task::Free, None);
                return true;
            }
            return false;
        }
        match self.ai_nearest_port(plan, id) {
            Some(t) => {
                self.ai_sail_toward(plan, id, t, false);
            }
            None => self.ai_set_mission(id, Task::Free, None),
        }
        false
    }

    fn ai_needs_repair(&self, plan: &Plan, id: UnitId) -> bool {
        let u = &self.units[id];
        plan.style.repairs && (u.hits as u32) * 2 <= self.rules.stats(u.kind).hits as u32
    }

    /// The best sea square next to unexplored water, for ships with nothing else to do.
    fn ai_sea_frontier(&self, plan: &mut Plan, id: UnitId) -> Option<Loc> {
        let p = self.units[id].owner;
        let geom = self.map.geom;
        let mut bfs = std::mem::take(&mut plan.bfs);
        self.ai_sea_search(&mut bfs, id, 400, false);
        let view = &self.player(p).view;
        let mut best: Option<Goal> = None;
        for (cell, d) in bfs.reached() {
            if d == 0 || !view[cell].is_sea() || plan.claims[cell] > 0 {
                continue;
            }
            let unknown = unknown_around(view, &geom, cell);
            if unknown > 0 {
                better(&mut best, Goal { score: unknown as f64 / (d as f64 + 1.0), loc: cell, radius: 3 });
            }
        }
        plan.bfs = bfs;
        let g = best?;
        plan.claim(self, g.loc, g.radius);
        Some(g.loc)
    }

    // ------------------------------------------------------------------
    // Transports: gather armies, deliver them, unload, repeat.

    /// Armies aboard, or for a transport in port, the armies that would
    /// board if it sailed now.
    fn ai_load(&self, id: UnitId) -> usize {
        let s = &self.units[id];
        if self.in_city(id) {
            let cap = self.capacity(id);
            self.at[s.loc]
                .iter()
                .filter(|&&o| {
                    let u = &self.units[o];
                    u.owner == s.owner && u.kind == UnitType::Army && matches!(u.orders, Orders::None | Orders::Sentry)
                })
                .count()
                .min(cap)
        } else {
            self.cargo(id).len()
        }
    }

    pub(super) fn ai_transport(&mut self, plan: &mut Plan, id: UnitId) {
        for _ in 0..10 {
            let u = &self.units[id];
            if !u.alive || u.moves_left == 0 {
                return;
            }
            let (loc, task) = (u.loc, u.mission.task);
            let load = self.ai_load(id);
            if load == 0 && task != Task::Repair && self.ai_needs_repair(plan, id) {
                self.ai_set_mission(id, Task::Repair, None);
                continue;
            }
            match task {
                Task::Repair => {
                    if !self.ai_repair(plan, id) {
                        return;
                    }
                }
                Task::Free | Task::Gather | Task::Scout => {
                    if load > 0 && self.ai_ready_to_sail(plan, id, load) {
                        let origin = Some(loc);
                        self.units[id].mission = Mission { task: Task::Deliver, target: None, origin, since: self.turn };
                        continue;
                    }
                    match self.ai_pick_pickup(plan, id) {
                        Some(t) if t == loc => {
                            if task != Task::Gather {
                                self.ai_set_mission(id, Task::Gather, Some(t));
                            }
                            return;
                        }
                        Some(t) => {
                            self.ai_set_mission(id, Task::Free, Some(t));
                            if !self.ai_sail_toward(plan, id, t, false) {
                                return;
                            }
                        }
                        None if load > 0 => {
                            self.units[id].mission = Mission { task: Task::Deliver, target: None, origin: Some(loc), since: self.turn };
                        }
                        None => {
                            // Nothing to fetch: wait in port.
                            self.ai_set_mission(id, Task::Free, None);
                            match self.ai_nearest_port(plan, id) {
                                Some(t) if t != loc => {
                                    if !self.ai_sail_toward(plan, id, t, false) {
                                        return;
                                    }
                                }
                                _ => return,
                            }
                        }
                    }
                }
                Task::Deliver => {
                    if load == 0 {
                        self.ai_set_mission(id, Task::Free, None);
                        continue;
                    }
                    match self.ai_pick_landing(plan, id) {
                        Some(t) if t == loc => {
                            self.units[id].mission.task = Task::Unload;
                            self.units[id].mission.target = Some(t);
                            self.units[id].mission.since = self.turn;
                            self.ai_unload(plan, id);
                            if self.ai_load(id) > 0 {
                                return;
                            }
                        }
                        Some(t) => {
                            self.units[id].mission.target = Some(t);
                            if !self.ai_sail_toward(plan, id, t, false) {
                                return;
                            }
                        }
                        None => match self.ai_sea_frontier(plan, id) {
                            Some(t) => {
                                if !self.ai_sail_toward(plan, id, t, false) {
                                    return;
                                }
                            }
                            None => return,
                        },
                    }
                }
                Task::Unload => {
                    self.ai_unload(plan, id);
                    if self.ai_load(id) == 0 {
                        self.units[id].mission = Mission::default();
                        continue;
                    }
                    if self.turn >= self.units[id].mission.since + 2 {
                        self.units[id].mission.task = Task::Deliver;
                        self.units[id].mission.target = None;
                    }
                    return;
                }
            }
        }
    }

    fn ai_ready_to_sail(&self, plan: &Plan, id: UnitId, load: usize) -> bool {
        let u = &self.units[id];
        let style = plan.style;
        if load >= self.capacity(id) {
            return true;
        }
        let waited = if u.mission.task == Task::Gather { self.turn.saturating_sub(u.mission.since) } else { 0 };
        let more_coming = plan
            .shore_continent(self, u.loc)
            .map(|c| plan.continents[c].surplus() as usize > load)
            .unwrap_or(false);
        (waited >= style.patience && (load >= style.min_load || !more_coming)) || waited >= 3 * style.patience
    }

    /// The pickup point this transport should serve: the one with the most
    /// armies not yet covered by other transports, allowing for distance.
    fn ai_pick_pickup(&mut self, plan: &mut Plan, id: UnitId) -> Option<Loc> {
        let (loc, current) = (self.units[id].loc, self.units[id].mission.target);
        let cap = self.capacity(id).max(1) as u32;
        let mut bfs = std::mem::take(&mut plan.bfs);
        self.ai_sea_search(&mut bfs, id, 400, false);
        let mut best: Option<(f64, Loc)> = None;
        for c in &plan.continents {
            let Some(pk) = c.pickup else { continue };
            let surplus = c.surplus();
            if surplus == 0 {
                continue;
            }
            let d = if pk.loc == loc {
                0
            } else {
                match bfs.dist(pk.loc) {
                    Some(d) => d,
                    None => continue,
                }
            };
            let covered = plan.claims[pk.loc] as u32 * cap;
            let uncovered = surplus.saturating_sub(covered);
            if uncovered == 0 {
                continue;
            }
            let mut score = uncovered.min(cap) as f64 / (d as f64 + 2.0);
            if current == Some(pk.loc) {
                score *= 1.5;
            }
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, pk.loc));
            }
        }
        plan.bfs = bfs;
        let (_, t) = best?;
        plan.claim(self, t, 0);
        Some(t)
    }

    /// How good a sea square is for putting armies ashore. Continents that
    /// already have (or are getting) all the armies they can use are skipped.
    /// `mine` is the continent this transport's load is already counted for.
    fn ai_landing_value(&self, plan: &Plan, p: PlayerId, sea: Loc, origin: Option<usize>, mine: Option<usize>, load: u32) -> f64 {
        let geom = self.map.geom;
        let view = &self.player(p).view;
        let mut value: f64 = 0.0;
        let mut unknown = 0;
        for (_, n) in geom.neighbors(sea) {
            if geom.is_border(n) {
                continue;
            }
            let v = view[n];
            if v.is_unknown() {
                unknown += 1;
                continue;
            }
            let Some(c) = plan.cont_id(n) else { continue };
            if Some(c) == origin {
                continue;
            }
            let cont = &plan.continents[c];
            let inbound = cont.inbound.saturating_sub(if mine == Some(c) { load } else { 0 });
            let room = cont.need() as i64 - (cont.own_armies + inbound) as i64;
            if v.is_city() && v.owner() != p {
                if room > -(load as i64) {
                    value = value.max(if v.owner() == 0 { 100.0 } else { 90.0 });
                }
            } else if v.is_land() && !v.is_unit() && room > 0 {
                let v = if cont.targets() > 0 {
                    50.0 + 15.0 * cont.targets().min(4) as f64
                } else {
                    // Unexplored ground only: worth more on bigger land.
                    10.0 + cont.frontier.min(30) as f64
                };
                value = value.max(v);
            }
        }
        if value == 0.0 {
            value = 4.0 * unknown as f64;
        }
        value
    }

    fn ai_pick_landing(&mut self, plan: &mut Plan, id: UnitId) -> Option<Loc> {
        let u = &self.units[id];
        let (p, loc) = (u.owner, u.loc);
        let origin = u.mission.origin.and_then(|o| plan.shore_continent(self, o));
        let mine = u.mission.target.and_then(|t| plan.shore_continent(self, t));
        let load = self.cargo(id).len() as u32;
        let mut bfs = std::mem::take(&mut plan.bfs);
        self.ai_sea_search(&mut bfs, id, 400, false);
        let view = &self.player(p).view;
        let mut best: Option<(f64, Loc)> = None;
        for (cell, d) in bfs.reached() {
            if !view[cell].is_sea() || (d > 0 && self.ai_own_occupied(p, cell)) {
                continue;
            }
            let mut value = self.ai_landing_value(plan, p, cell, origin, mine, load);
            if value <= 0.0 {
                continue;
            }
            if plan.style.weigh_odds && plan.danger[cell] > 0 {
                value *= 0.6;
            }
            let score = value / (d as f64 + 3.0);
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, cell));
            }
        }
        plan.bfs = bfs;
        let pick = best.map(|(_, l)| l).filter(|&l| l != loc || self.ai_landing_value(plan, p, loc, origin, mine, load) >= 30.0)?;
        // Keep the count of armies heading to each continent current.
        let new = plan.shore_continent(self, pick);
        if new != mine {
            if let Some(c) = mine {
                plan.continents[c].inbound = plan.continents[c].inbound.saturating_sub(load);
            }
            if let Some(c) = new {
                plan.continents[c].inbound += load;
            }
        }
        Some(pick)
    }

    fn ai_unload(&mut self, plan: &mut Plan, ship: UnitId) {
        let riders: Vec<UnitId> = self.cargo(ship);
        for a in riders {
            if self.units[a].alive && self.units[a].moves_left > 0 {
                self.ai_army_aboard(plan, a);
            }
        }
    }

    // ------------------------------------------------------------------
    // Warships

    pub(super) fn ai_warship(&mut self, plan: &mut Plan, id: UnitId) {
        let geom = self.map.geom;
        for _ in 0..8 {
            let u = &self.units[id];
            if !u.alive || u.moves_left == 0 {
                return;
            }
            let (p, loc, kind) = (u.owner, u.loc, u.kind);
            if u.mission.task != Task::Repair && self.ai_needs_repair(plan, id) {
                self.ai_set_mission(id, Task::Repair, None);
            }
            if self.units[id].mission.task == Task::Repair {
                if !self.ai_repair(plan, id) {
                    return;
                }
                continue;
            }
            let mut bfs = std::mem::take(&mut plan.bfs);
            self.ai_sea_search(&mut bfs, id, 400, true);
            let style = plan.style;
            let view = &self.player(p).view;
            let mut best: Option<Goal> = None;
            for (cell, d) in bfs.reached() {
                if d == 0 {
                    continue;
                }
                let v = view[cell];
                let claims = plan.claims[cell];
                let (value, radius) = if let (Some(k), true) = (v.unit_type(), v.is_enemy_of(p)) {
                    let bombard = v.is_land();
                    let bonus = if bombard && self.ai_near_own_city(p, cell, 2) { 20.0 } else { 0.0 };
                    if claims >= 2 || !self.ai_should_attack(plan, id, k, v.owner(), bombard, bonus) {
                        (0.0, 0)
                    } else {
                        (3.0 * (worth(self, k) + bonus) * self.win_chance(id, k, v.owner(), bombard), 0)
                    }
                } else if v.is_sea() && claims == 0 {
                    let unknown = unknown_around(view, &geom, cell);
                    (3.0 * unknown as f64, 3)
                } else {
                    (0.0, 0)
                };
                if value > 0.0 {
                    better(&mut best, Goal { score: value / (d as f64 + 1.0), loc: cell, radius });
                }
            }
            if style.memory {
                let turn = self.turn;
                for s in &self.player(p).intel.sightings {
                    if !s.kind.is_ship() || s.turn + 8 < turn || s.owner == p {
                        continue;
                    }
                    if let Some(d) = bfs.dist(s.loc).filter(|&d| d > 0) {
                        let value = 0.4 * worth(self, s.kind);
                        better(&mut best, Goal { score: value / (d as f64 + 1.0), loc: s.loc, radius: 2 });
                    }
                }
            }
            let mut escort: Option<Loc> = None;
            if style.escorts && matches!(kind, UnitType::Destroyer | UnitType::Cruiser) {
                for t in self.player_units(p) {
                    let s = &self.units[t];
                    if s.kind != UnitType::Transport || !matches!(s.mission.task, Task::Deliver | Task::Unload) {
                        continue;
                    }
                    if plan.claims[s.loc] > 0 && geom.dist(s.loc, loc) > 1 {
                        continue;
                    }
                    let d = geom.dist(s.loc, loc);
                    if d > 15 {
                        continue;
                    }
                    let score = 30.0 / (d as f64 + 1.0);
                    if best.is_none_or(|b| score > b.score) {
                        best = Some(Goal { score, loc: s.loc, radius: 0 });
                        escort = Some(s.loc);
                    }
                }
            }
            let goal = best;
            let step = goal.and_then(|g| {
                if Some(g.loc) == escort && geom.dist(g.loc, loc) <= 1 {
                    return None;
                }
                bfs.first_step(g.loc)
            });
            plan.bfs = bfs;
            if let Some(g) = goal {
                plan.claim(self, g.loc, g.radius);
                if Some(g.loc) == escort && geom.dist(g.loc, loc) <= 1 {
                    // Guarding: stay next to the transport.
                    return;
                }
            }
            match step {
                Some(d) => {
                    if !self.ai_step(id, d) {
                        return;
                    }
                }
                None => {
                    // Nothing to do: wait in port.
                    match self.ai_nearest_port(plan, id) {
                        Some(t) if t != loc => {
                            if !self.ai_sail_toward(plan, id, t, false) {
                                return;
                            }
                        }
                        _ => return,
                    }
                }
            }
        }
    }

    /// Carriers stay in port, where fighters can use them as a second deck.
    pub(super) fn ai_carrier(&mut self, plan: &mut Plan, id: UnitId) {
        if self.in_city(id) {
            return;
        }
        if let Some(t) = self.ai_nearest_port(plan, id) {
            for _ in 0..4 {
                if !self.units[id].alive || self.units[id].moves_left == 0 || self.in_city(id) {
                    return;
                }
                if !self.ai_sail_toward(plan, id, t, false) {
                    return;
                }
            }
        }
    }
}
