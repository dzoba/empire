//! The turn plan: what the computer opponent works out once per turn from
//! its own map, before moving any unit.

use super::field::{Bfs, Cell};
use crate::game::Game;
use crate::geom::Loc;
use crate::player::{Control, Skill};
use crate::types::{PlayerId, UnitType};
use crate::unit::Task;
use serde::{Deserialize, Serialize};

/// Behaviour switches that separate the two skill levels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    /// Check combat odds before attacking, and weigh what could be lost.
    pub weigh_odds: bool,
    /// Rank targets by value as well as distance (otherwise nearest first).
    pub values: bool,
    /// Send destroyers to guard loaded transports.
    pub escorts: bool,
    /// Send damaged ships home for repairs.
    pub repairs: bool,
    /// Hunt enemy ships seen on earlier turns.
    pub memory: bool,
    /// Build submarines, cruisers and battleships, not just destroyers.
    pub full_navy: bool,
    /// Reconsider a city's production every time it finishes a unit.
    pub rethink: bool,
    /// Fighters wanted per city owned.
    pub fighters_per_city: f32,
    /// Armies a transport waits for before sailing, unless it waits too long.
    pub min_load: usize,
    /// Turns a transport waits at a pickup point for a fuller load.
    pub patience: u32,
}

impl Style {
    pub fn expert() -> Style {
        Style {
            weigh_odds: true,
            values: true,
            escorts: true,
            repairs: true,
            memory: true,
            full_navy: true,
            rethink: true,
            fighters_per_city: 0.4,
            min_load: 4,
            patience: 6,
        }
    }

    pub fn standard() -> Style {
        Style {
            weigh_odds: false,
            values: false,
            escorts: false,
            repairs: false,
            memory: false,
            full_navy: false,
            rethink: false,
            fighters_per_city: 0.15,
            min_load: 2,
            patience: 3,
        }
    }

    pub fn for_control(c: Control) -> Style {
        match c {
            Control::Computer(Skill::Standard) => Style::standard(),
            _ => Style::expert(),
        }
    }
}

impl Default for Style {
    fn default() -> Self {
        Style::expert()
    }
}

/// Where armies with nothing left to do on their continent wait for a ship.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pickup {
    /// A friendly port city, or a sea square next to the shore.
    pub loc: Loc,
    pub port: bool,
}

/// A body of known land and what the player knows about it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Continent {
    pub cells: u32,
    pub own_cities: u32,
    pub neutral_cities: u32,
    pub enemy_cities: u32,
    /// Known land squares next to unexplored ones.
    pub frontier: u32,
    pub own_armies: u32,
    pub enemy_armies: u32,
    /// Armies aboard transports heading here.
    pub inbound: u32,
    /// Own cities here building armies.
    pub building_armies: u32,
    pub pickup: Option<Pickup>,
}

impl Continent {
    /// Cities here to capture.
    pub fn targets(&self) -> u32 {
        self.neutral_cities + self.enemy_cities
    }

    /// Armies this continent can use: attackers for its cities, defenders
    /// against enemy armies, and scouts for unexplored ground.
    pub fn need(&self) -> u32 {
        let scouts = if self.frontier == 0 { 0 } else { (1 + self.frontier / 12).min(6) };
        2 * self.neutral_cities + 3 * self.enemy_cities + self.enemy_armies + scouts
    }

    /// Armies still wanted after counting those present, on the way and being built.
    pub fn wanted(&self) -> u32 {
        self.need().saturating_sub(self.own_armies + self.inbound + self.building_armies)
    }

    /// Armies here with nothing to do.
    pub fn surplus(&self) -> u32 {
        self.own_armies.saturating_sub(self.need())
    }

    pub fn worth_landing(&self) -> bool {
        self.targets() > 0 || self.frontier > 0
    }
}

pub const NO_CONTINENT: u32 = u32::MAX;

/// Per-square arrays are saved as (value, count) runs: most are long
/// stretches of a single value.
mod runs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[allow(clippy::ptr_arg)]
    pub fn serialize<T: Copy + PartialEq + Serialize, S: Serializer>(v: &Vec<T>, s: S) -> Result<S::Ok, S::Error> {
        let mut runs: Vec<(T, u32)> = Vec::new();
        for &x in v {
            match runs.last_mut() {
                Some((y, n)) if *y == x => *n += 1,
                _ => runs.push((x, 1)),
            }
        }
        runs.serialize(s)
    }

    pub fn deserialize<'de, T: Copy + Deserialize<'de>, D: Deserializer<'de>>(d: D) -> Result<Vec<T>, D::Error> {
        let runs: Vec<(T, u32)> = Vec::deserialize(d)?;
        let mut out = Vec::new();
        for (x, n) in runs {
            out.extend(std::iter::repeat_n(x, n as usize));
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Plan {
    pub player: PlayerId,
    pub turn: u32,
    pub style: Style,
    /// Continent of each known land or city square; `NO_CONTINENT` elsewhere.
    #[serde(with = "runs")]
    pub continent_of: Vec<u32>,
    pub continents: Vec<Continent>,
    /// Fighter moves from each square to the nearest friendly city.
    #[serde(with = "runs")]
    pub refuel: Vec<u16>,
    /// Units that have picked each square as a target this turn.
    #[serde(with = "runs")]
    pub claims: Vec<u8>,
    /// Squares worth a fighter's look: near enemy cities and recent sightings.
    #[serde(with = "runs", default)]
    pub patrol: Vec<u8>,
    /// Squares within striking distance of enemy warships and fighters seen
    /// lately. Loaded transports keep out of them when they can.
    #[serde(with = "runs", default)]
    pub danger: Vec<u8>,
    /// Own units by type.
    pub have: [u32; 8],
    /// Own cities building each type.
    pub building: [u32; 8],
    /// Armies on each continent that have picked a target there this turn.
    #[serde(default)]
    pub assigned: Vec<u32>,
    pub ports: u32,
    /// Unexplored squares left on the player's map.
    pub unknown: usize,
    /// Fighters that may still be sent on one-way scouting flights this turn.
    #[serde(default)]
    pub scouts: u32,
    #[serde(skip)]
    pub bfs: Bfs,
}

impl Plan {
    pub fn cont_id(&self, loc: Loc) -> Option<usize> {
        match self.continent_of.get(loc) {
            Some(&c) if c != NO_CONTINENT => Some(c as usize),
            _ => None,
        }
    }

    pub fn continent(&self, loc: Loc) -> Option<&Continent> {
        self.cont_id(loc).map(|c| &self.continents[c])
    }

    /// The continent a sea square touches most.
    pub fn shore_continent(&self, game: &Game, sea: Loc) -> Option<usize> {
        if let Some(c) = self.cont_id(sea) {
            return Some(c);
        }
        let mut best: Option<(usize, usize)> = None;
        for (_, n) in game.map.geom.neighbors(sea) {
            if let Some(c) = self.cont_id(n) {
                let k = game.map.geom.neighbors(sea).filter(|&(_, m)| self.cont_id(m) == Some(c)).count();
                if best.is_none_or(|(_, bk)| k > bk) {
                    best = Some((c, k));
                }
            }
        }
        best.map(|(c, _)| c)
    }

    /// Mark a square (and its surroundings out to `radius`) as taken.
    pub fn claim(&mut self, game: &Game, loc: Loc, radius: usize) {
        if radius == 0 {
            self.claims[loc] = self.claims[loc].saturating_add(1);
            return;
        }
        let g = game.map.geom;
        let (r0, c0) = (g.row(loc) as i64, g.col(loc) as i64);
        let rad = radius as i64;
        for r in (r0 - rad).max(0)..=(r0 + rad).min(g.height as i64 - 1) {
            for c in (c0 - rad).max(0)..=(c0 + rad).min(g.width as i64 - 1) {
                let l = g.loc(r as usize, c as usize);
                self.claims[l] = self.claims[l].saturating_add(1);
            }
        }
    }
}

impl Game {
    /// Work out the turn plan for computer player `p`.
    pub(crate) fn ai_build_plan(&mut self, p: PlayerId) -> Plan {
        let geom = self.map.geom;
        let n = geom.size();
        let mut plan = Plan {
            player: p,
            turn: self.turn,
            style: Style::for_control(self.player(p).control),
            continent_of: vec![NO_CONTINENT; n],
            claims: vec![0; n],
            ..Default::default()
        };

        // Continents: 8-connected bodies of known land and cities.
        {
            let view = &self.player(p).view;
            let mut stack = Vec::new();
            for start in 0..n {
                let v = view[start];
                if plan.continent_of[start] != NO_CONTINENT || geom.is_border(start) || !(v.is_land() || v.is_city()) {
                    continue;
                }
                let id = plan.continents.len() as u32;
                let mut cont = Continent::default();
                plan.continent_of[start] = id;
                stack.push(start);
                while let Some(l) = stack.pop() {
                    cont.cells += 1;
                    let v = view[l];
                    if v.is_city() {
                        match v.owner() {
                            0 => cont.neutral_cities += 1,
                            o if o == p => cont.own_cities += 1,
                            _ => cont.enemy_cities += 1,
                        }
                    } else if v.is_enemy_of(p) && v.unit_type() == Some(UnitType::Army) {
                        cont.enemy_armies += 1;
                    }
                    let mut frontier = false;
                    for (_, nb) in geom.neighbors(l) {
                        if geom.is_border(nb) {
                            continue;
                        }
                        let w = view[nb];
                        if w.is_unknown() {
                            frontier = true;
                        } else if (w.is_land() || w.is_city()) && plan.continent_of[nb] == NO_CONTINENT {
                            plan.continent_of[nb] = id;
                            stack.push(nb);
                        }
                    }
                    if frontier {
                        cont.frontier += 1;
                    }
                }
                plan.continents.push(cont);
            }
            plan.unknown = view.iter().enumerate().filter(|&(l, v)| v.is_unknown() && !geom.is_border(l)).count();
        }

        plan.assigned = vec![0; plan.continents.len()];

        // Own forces.
        let ids: Vec<usize> = self.player_units(p).collect();
        for &id in &ids {
            let u = &self.units[id];
            plan.have[u.kind.index()] += 1;
            if u.kind == UnitType::Army {
                if let Some(ship) = self.carrier_of(id) {
                    // Armies waiting aboard a transport at a beach still count
                    // for the continent they are leaving.
                    let s = &self.units[ship];
                    if s.mission.task == Task::Gather {
                        if let Some(c) = plan.shore_continent(self, s.loc) {
                            plan.continents[c].own_armies += 1;
                        }
                    }
                } else if let Some(c) = plan.cont_id(u.loc) {
                    plan.continents[c].own_armies += 1;
                }
            } else if u.kind == UnitType::Transport && matches!(u.mission.task, Task::Deliver | Task::Unload) {
                if let Some(t) = u.mission.target {
                    let load = self.cargo(id).len() as u32;
                    if let Some(c) = plan.shore_continent(self, t) {
                        plan.continents[c].inbound += load;
                    }
                }
            }
        }
        for ci in self.player_cities(p).collect::<Vec<_>>() {
            let c = &self.cities[ci];
            if c.port {
                plan.ports += 1;
            }
            if let Some(k) = c.production {
                plan.building[k.index()] += 1;
                if k == UnitType::Army {
                    if let Some(cid) = plan.cont_id(c.loc) {
                        plan.continents[cid].building_armies += 1;
                    }
                }
            }
        }

        // Pickup points for continents with armies to spare. Last turn's
        // points are kept while they still work, so armies don't wander.
        let old: Vec<Loc> = self.player(p).intel.pickups.clone();
        let mut chosen = Vec::new();
        for cid in 0..plan.continents.len() {
            if plan.continents[cid].surplus() == 0 {
                continue;
            }
            let keep = old.iter().copied().find_map(|loc| {
                let pk = Pickup { loc, port: self.city_at(loc).is_some() };
                self.ai_pickup_valid(p, &plan, cid, pk).then_some(pk)
            });
            let pk = keep.or_else(|| self.ai_find_pickup(p, &plan, cid));
            if let Some(pk) = pk {
                chosen.push(pk.loc);
            }
            plan.continents[cid].pickup = pk;
        }
        self.player_mut(p).intel.pickups = chosen;

        // While much of the world is unexplored, a few fighters fly one-way
        // scouting missions: a fighter costs little next to what it finds.
        let cities = self.player_cities(p).count() as u32;
        if plan.unknown * 5 > geom.playable_cells() && cities >= 2 {
            let flying = self
                .player_units(p)
                .filter(|&id| self.units[id].kind == UnitType::Fighter && self.units[id].mission.task == Task::Scout)
                .count() as u32;
            plan.scouts = (1 + cities / 10).saturating_sub(flying);
        }
        self.ai_threat_maps(p, &mut plan);
        self.ai_refuel_field(p, &mut plan);
        plan
    }

    fn ai_threat_maps(&self, p: PlayerId, plan: &mut Plan) {
        let geom = self.map.geom;
        let n = geom.size();
        plan.patrol = vec![0; n];
        plan.danger = vec![0; n];
        let mark = |field: &mut Vec<u8>, at: Loc, radius: i64, value: u8| {
            let (r0, c0) = (geom.row(at) as i64, geom.col(at) as i64);
            for r in (r0 - radius).max(1)..=(r0 + radius).min(geom.height as i64 - 2) {
                for c in (c0 - radius).max(1)..=(c0 + radius).min(geom.width as i64 - 2) {
                    let l = geom.loc(r as usize, c as usize);
                    field[l] = field[l].max(value);
                }
            }
        };
        let view = &self.player(p).view;
        for (l, v) in view.iter().enumerate() {
            if v.is_city() && v.is_enemy_of(p) {
                mark(&mut plan.patrol, l, 2, 6);
            }
            if let (Some(k), true) = (v.unit_type(), v.is_enemy_of(p)) {
                if k == UnitType::Fighter {
                    mark(&mut plan.danger, l, 5, 1);
                } else if k.is_ship() && k != UnitType::Transport {
                    mark(&mut plan.danger, l, self.rules.stats(k).speed as i64 + 1, 1);
                }
            }
        }
        let turn = self.turn;
        for s in &self.player(p).intel.sightings {
            if s.turn + 3 < turn {
                continue;
            }
            if s.kind.is_ship() {
                mark(&mut plan.patrol, s.loc, 2, if s.kind == UnitType::Transport { 12 } else { 8 });
                if s.kind != UnitType::Transport && s.turn + 1 >= turn {
                    mark(&mut plan.danger, s.loc, self.rules.stats(s.kind).speed as i64 + 1, 1);
                }
            }
        }
    }

    fn ai_pickup_valid(&self, p: PlayerId, plan: &Plan, cid: usize, pk: Pickup) -> bool {
        let view = &self.player(p).view;
        if pk.port {
            plan.cont_id(pk.loc) == Some(cid)
                && self.city_at(pk.loc).is_some_and(|ci| self.cities[ci].owner == p && self.cities[ci].port)
        } else {
            view[pk.loc].is_sea() && !view[pk.loc].is_enemy_of(p) && plan.shore_continent(self, pk.loc) == Some(cid)
        }
    }

    /// Choose where a continent's spare armies should gather: the friendly
    /// port closest to them, or failing that the nearest stretch of shore.
    fn ai_find_pickup(&self, p: PlayerId, plan: &Plan, cid: usize) -> Option<Pickup> {
        let geom = self.map.geom;
        let armies: Vec<Loc> = self
            .player_units(p)
            .filter(|&id| self.units[id].kind == UnitType::Army && plan.cont_id(self.units[id].loc) == Some(cid))
            .map(|id| self.units[id].loc)
            .take(24)
            .collect();
        if armies.is_empty() {
            return None;
        }
        let cost = |l: Loc| armies.iter().map(|&a| geom.dist(a, l)).sum::<usize>();
        let port = self
            .player_cities(p)
            .filter(|&ci| self.cities[ci].port && plan.cont_id(self.cities[ci].loc) == Some(cid))
            .map(|ci| self.cities[ci].loc)
            .min_by_key(|&l| (cost(l), l));
        if let Some(loc) = port {
            return Some(Pickup { loc, port: true });
        }
        let view = &self.player(p).view;
        let mut best: Option<(usize, Loc)> = None;
        for (l, v) in view.iter().enumerate() {
            if !v.is_sea() || v.is_unit() || geom.is_border(l) {
                continue;
            }
            if !geom.neighbors(l).any(|(_, nb)| plan.cont_id(nb) == Some(cid)) {
                continue;
            }
            let c = cost(l);
            if best.is_none_or(|(bc, _)| c < bc) {
                best = Some((c, l));
            }
        }
        best.map(|(_, loc)| Pickup { loc, port: false })
    }

    /// Fighter distance from every square to the nearest friendly city.
    pub(crate) fn ai_refuel_field(&mut self, p: PlayerId, plan: &mut Plan) {
        let geom = self.map.geom;
        let cities: Vec<Loc> = self.player_cities(p).map(|ci| self.cities[ci].loc).collect();
        let view = &self.player(p).view;
        let mut bfs = std::mem::take(&mut plan.bfs);
        bfs.run(&geom, &cities, 200, 0, |l| {
            let v = view[l];
            if v.is_city() && v.owner() != p {
                Cell::Block
            } else {
                Cell::Open
            }
        });
        bfs.to_field(&mut plan.refuel, geom.size());
        plan.bfs = bfs;
    }
}
