//! Human players: carrying out standing orders, and the commands a UI sends.

use crate::game::{Game, WakeReason};
use crate::geom::{Dir, Loc};
use crate::movement::{MoveError, MoveKind, StepOutcome};
use crate::path::{Pass, Search};
use crate::types::{PlayerId, Terrain, UnitType};
use crate::unit::{Orders, UnitId, UnitRef};

/// What a unit's orders want to do next.
enum Auto {
    Step(Dir),
    Stay,
    Wake(WakeReason),
}

fn peaceful(k: MoveKind) -> bool {
    matches!(
        k,
        MoveKind::Move
            | MoveKind::EnterCity
            | MoveKind::Board(_)
            | MoveKind::LandInCity
            | MoveKind::LandOnCarrier(_)
            | MoveKind::Dock
    )
}

impl Game {
    /// Pick the next unit to ask the human about, running standing orders first.
    pub(crate) fn next_human_unit(&mut self, p: PlayerId) -> Option<(UnitId, WakeReason)> {
        // Units with orders move on their own until something needs attention.
        loop {
            let auto = self.units.iter().enumerate().find(|(id, u)| {
                u.alive
                    && u.owner == p
                    && !u.moved
                    && !u.waiting
                    && u.moves_left > 0
                    && (u.orders != Orders::None || self.rides_quietly(*id))
            });
            let Some((id, _)) = auto else { break };
            if let Some(reason) = self.run_orders(id) {
                return Some((id, reason));
            }
            if self.game_over {
                return None;
            }
        }
        // Then idle units, nearest to the last one first; then units told to wait.
        for waiting in [false, true] {
            let near = self.last_loc;
            let geom = self.map.geom;
            let pick = self
                .units
                .iter()
                .enumerate()
                .filter(|(_, u)| u.alive && u.owner == p && !u.moved && u.moves_left > 0 && u.waiting == waiting)
                .min_by_key(|(id, u)| (near.map(|l| geom.dist(l, u.loc)).unwrap_or(0), *id))
                .map(|(id, _)| id);
            if let Some(id) = pick {
                self.units[id].waiting = false;
                return Some((id, WakeReason::NoOrders));
            }
        }
        None
    }

    /// An idle army riding a transport out at sea doesn't need to be asked.
    fn rides_quietly(&self, id: UnitId) -> bool {
        let u = &self.units[id];
        if u.kind != UnitType::Army || self.carrier_of(id).is_none() {
            return false;
        }
        let view = &self.player(u.owner).view;
        self.map.geom.neighbors(u.loc).all(|(_, l)| {
            let v = view[l];
            !((v.is_land() || v.is_city()) && v.owner() != u.owner) && !(v.is_city() && v.owner() == 0)
        })
    }

    pub(crate) fn run_orders_pub(&mut self, id: UnitId) -> Option<WakeReason> {
        self.run_orders(id)
    }

    /// Execute a unit's orders for the rest of its turn. Returns a reason if
    /// the player must be asked.
    fn run_orders(&mut self, id: UnitId) -> Option<WakeReason> {
        loop {
            let u = &self.units[id];
            if !u.alive {
                return None;
            }
            if u.moves_left == 0 {
                self.units[id].moved = true;
                return None;
            }
            match self.auto_step(id) {
                Auto::Stay => {
                    self.units[id].moved = true;
                    return None;
                }
                Auto::Wake(r) => return Some(r),
                Auto::Step(d) => {
                    if self.step_unit(id, d).is_err() {
                        return Some(WakeReason::Blocked);
                    }
                }
            }
        }
    }

    fn path_step(&mut self, id: UnitId, target: Loc) -> Option<Dir> {
        let u = &self.units[id];
        let (owner, kind, loc) = (u.owner, u.kind, u.loc);
        let turn = if u.serial & 1 == 1 { 1 } else { -1 };
        let geom = self.map.geom;
        let view = &self.players[owner as usize - 1].view;
        let search = Search { geom: &geom, view, pass: Pass::for_unit(kind, owner) };
        search.first_step(&mut self.scratch, loc, target, turn)
    }

    fn try_peaceful(&self, id: UnitId, d: Dir) -> bool {
        self.classify_move(id, d).map(peaceful).unwrap_or(false)
    }

    fn head_for(&mut self, id: UnitId, target: Loc) -> Auto {
        let loc = self.units[id].loc;
        if loc == target {
            return Auto::Wake(WakeReason::Arrived);
        }
        let d = self.path_step(id, target).or_else(|| self.map.geom.dir_towards(loc, target));
        match d {
            Some(d) if self.try_peaceful(id, d) => Auto::Step(d),
            _ => Auto::Wake(WakeReason::Blocked),
        }
    }

    fn auto_step(&mut self, id: UnitId) -> Auto {
        let geom = self.map.geom;
        let u = self.units[id].clone();
        let p = u.owner;
        let aboard = self.carrier_of(id).is_some();

        let enemy_near = {
            let view = &self.player(p).view;
            geom.neighbors(u.loc).any(|(_, l)| view[l].is_unit() && view[l].is_enemy_of(p))
        };
        if enemy_near && !aboard {
            if matches!(u.orders, Orders::Sentry | Orders::Load) {
                self.units[id].orders = Orders::None;
            }
            return Auto::Wake(WakeReason::EnemyNear);
        }

        if u.kind == UnitType::Fighter && u.orders == Orders::None {
            if let Some(ci) = self.city_at(u.loc) {
                if let Some(fp) = self.cities[ci].flight_path {
                    if fp != u.loc {
                        self.units[id].orders = Orders::MoveTo(fp);
                        return self.auto_step(id);
                    }
                }
            }
        }

        if u.kind == UnitType::Army && !matches!(u.orders, Orders::None | Orders::Sentry) {
            let view = &self.player(p).view;
            if geom.neighbors(u.loc).any(|(_, l)| view[l].is_city() && view[l].owner() != p) {
                return Auto::Wake(WakeReason::CityNear);
            }
        }

        match u.orders {
            Orders::None => {
                if self.rides_quietly(id) {
                    Auto::Stay
                } else {
                    Auto::Wake(WakeReason::NoOrders)
                }
            }
            Orders::Sentry => Auto::Stay,
            Orders::Load => {
                if u.kind.cargo().is_none() {
                    self.units[id].orders = Orders::None;
                    return Auto::Wake(WakeReason::NoOrders);
                }
                self.load_adjacent(id);
                if self.is_full(id) {
                    self.units[id].orders = Orders::None;
                    Auto::Wake(WakeReason::Full)
                } else {
                    Auto::Stay
                }
            }
            Orders::Direction(d) => {
                if u.kind == UnitType::Fighter && !u.fuel_warned && u.fuel <= self.rules.fighter_wake_fuel {
                    self.units[id].fuel_warned = true;
                    return Auto::Wake(WakeReason::LowFuel);
                }
                if self.try_peaceful(id, d) {
                    Auto::Step(d)
                } else {
                    Auto::Wake(WakeReason::Blocked)
                }
            }
            Orders::MoveTo(target) => match self.head_for(id, target) {
                Auto::Wake(WakeReason::Arrived) => {
                    self.units[id].orders = Orders::None;
                    Auto::Wake(WakeReason::Arrived)
                }
                other => other,
            },
            Orders::FollowCarrier(r) => {
                if !self.unit_valid(r) || self.units[r.id].owner != p {
                    self.units[id].orders = Orders::None;
                    return Auto::Wake(WakeReason::Blocked);
                }
                let target = self.units[r.id].loc;
                self.head_for(id, target)
            }
            Orders::GoHome => match self.home_for(id) {
                Some(target) if target == u.loc => {
                    self.units[id].orders = Orders::None;
                    Auto::Wake(WakeReason::Arrived)
                }
                Some(target) => self.head_for(id, target),
                None => Auto::Wake(WakeReason::Blocked),
            },
            Orders::Random => {
                if u.kind == UnitType::Army && !aboard {
                    for d in crate::geom::ALL_DIRS {
                        if let Ok(MoveKind::Board(_)) = self.classify_move(id, d) {
                            self.units[id].orders = Orders::None;
                            return Auto::Step(d);
                        }
                    }
                }
                let start = self.rng.below(8) as i32;
                let turn = if u.serial & 1 == 1 { 1 } else { -1 };
                for k in 0..8 {
                    let d = crate::geom::rotate(start as Dir, k * turn);
                    // Don't wander onto ships or into cities.
                    if let Ok(MoveKind::Move) = self.classify_move(id, d) {
                        return Auto::Step(d);
                    }
                }
                Auto::Wake(WakeReason::Blocked)
            }
            Orders::Patrol { from, to, outbound } => {
                let mut outbound = outbound;
                let mut target = if outbound { to } else { from };
                if u.loc == target {
                    outbound = !outbound;
                    target = if outbound { to } else { from };
                    self.units[id].orders = Orders::Patrol { from, to, outbound };
                }
                if u.kind == UnitType::Fighter {
                    // Go refuel when the tank is low.
                    if let Some(home) = self.home_for(id) {
                        if geom.dist(u.loc, home) + 1 >= u.fuel as usize && home != u.loc {
                            return self.head_for(id, home);
                        }
                    }
                }
                match self.head_for(id, target) {
                    Auto::Wake(WakeReason::Arrived) => Auto::Stay,
                    other => other,
                }
            }
            Orders::Escort(r) => {
                if !self.unit_valid(r) || self.units[r.id].owner != p || r.id == id {
                    self.units[id].orders = Orders::None;
                    return Auto::Wake(WakeReason::Blocked);
                }
                let target = self.units[r.id].loc;
                if geom.dist(u.loc, target) <= 2 {
                    Auto::Stay
                } else {
                    match self.head_for(id, target) {
                        Auto::Wake(WakeReason::Blocked) => Auto::Stay,
                        other => other,
                    }
                }
            }
        }
    }

    /// Nearest friendly city (or carrier, for fighters) this unit can get to.
    pub fn home_for(&mut self, id: UnitId) -> Option<Loc> {
        let u = self.units[id].clone();
        let geom = self.map.geom;
        let mut best: Option<(usize, Loc)> = None;
        match u.kind {
            UnitType::Fighter => {
                let range = u.fuel as usize;
                for c in self.cities.iter().filter(|c| c.owner == u.owner) {
                    let d = geom.dist(u.loc, c.loc);
                    if d <= range && best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, c.loc));
                    }
                }
                for (cid, c) in self.units.iter().enumerate() {
                    if c.alive && c.owner == u.owner && c.kind == UnitType::Carrier && cid != id {
                        let d = geom.dist(u.loc, c.loc);
                        if d <= range && best.is_none_or(|(bd, _)| d < bd) && !self.is_full(cid) {
                            best = Some((d, c.loc));
                        }
                    }
                }
            }
            k if k.is_ship() => {
                let ports: Vec<Loc> = self.cities.iter().filter(|c| c.owner == u.owner && c.port).map(|c| c.loc).collect();
                let view = &self.players[u.owner as usize - 1].view;
                let search = Search { geom: &geom, view, pass: Pass::Ship(u.owner) };
                let mut sorted: Vec<(usize, Loc)> = ports.iter().map(|&l| (geom.dist(u.loc, l), l)).collect();
                sorted.sort();
                for (_, l) in sorted.into_iter().take(12) {
                    if let Some(n) = search.distance_within(&mut self.scratch, u.loc, l, usize::MAX) {
                        if best.is_none_or(|(bd, _)| n < bd) {
                            best = Some((n, l));
                        }
                    }
                }
            }
            _ => {}
        }
        best.map(|(_, l)| l)
    }

    /// A transport on Load takes on sentried armies standing next to it.
    fn load_adjacent(&mut self, ship: UnitId) {
        let s = &self.units[ship];
        if s.kind != UnitType::Transport || self.map.get(s.loc) != Terrain::Sea {
            return;
        }
        let (owner, loc) = (s.owner, s.loc);
        let geom = self.map.geom;
        for (_, l) in geom.neighbors(loc).collect::<Vec<_>>() {
            if self.is_full(ship) {
                break;
            }
            if self.map.get(l) != Terrain::Land {
                continue;
            }
            if let Some(a) = self.top_unit(l) {
                let au = &self.units[a];
                if au.owner == owner && au.kind == UnitType::Army && au.orders == Orders::Sentry {
                    self.relocate(a, loc);
                    self.units[a].moves_left = 0;
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Commands from the user interface

    fn own_unit(&self, id: UnitId) -> Result<(), String> {
        match self.units.get(id) {
            Some(u) if u.alive && u.owner == self.current => Ok(()),
            _ => Err("Not your unit".into()),
        }
    }

    /// Units of the current player still waiting for orders this turn
    /// (not counting armies riding quietly at sea).
    pub fn idle_units(&self) -> usize {
        let p = self.current;
        (0..self.units.len()).filter(|&id| self.idle(p, id)).count()
    }

    fn idle(&self, p: PlayerId, id: UnitId) -> bool {
        let u = &self.units[id];
        u.alive && u.owner == p && !u.moved && u.moves_left > 0 && u.orders == Orders::None && !self.rides_quietly(id)
    }

    /// End the turn early: every idle unit stays where it is. Units with
    /// standing orders still carry them out (and may still ask for orders
    /// if something comes up). Returns how many units were skipped.
    pub fn cmd_end_turn(&mut self) -> usize {
        let p = self.current;
        let ids: Vec<UnitId> = (0..self.units.len()).filter(|&id| self.idle(p, id)).collect();
        for &id in &ids {
            self.units[id].moved = true;
            self.units[id].waiting = false;
        }
        self.prompt = None;
        ids.len()
    }

    /// Move a unit one square by hand.
    pub fn cmd_move(&mut self, id: UnitId, dir: Dir) -> Result<StepOutcome, MoveError> {
        if self.own_unit(id).is_err() {
            return Err(MoveError::NoUnit);
        }
        let out = self.step_unit(id, dir)?;
        if self.units[id].alive && self.units[id].moves_left > 0 {
            self.prompt = Some((id, WakeReason::MovesLeft));
            self.last_loc = Some(self.units[id].loc);
        } else {
            if self.units[id].alive {
                self.units[id].moved = true;
                self.last_loc = Some(self.units[id].loc);
            }
            self.prompt = None;
        }
        Ok(out)
    }

    /// Skip Move: the unit does nothing more this turn.
    pub fn cmd_skip(&mut self, id: UnitId) -> Result<(), String> {
        self.own_unit(id)?;
        self.units[id].moved = true;
        if self.prompt.map(|(u, _)| u) == Some(id) {
            self.prompt = None;
        }
        Ok(())
    }

    /// Wait: come back to this unit after the others.
    pub fn cmd_wait(&mut self, id: UnitId) -> Result<(), String> {
        self.own_unit(id)?;
        self.units[id].waiting = true;
        if self.prompt.map(|(u, _)| u) == Some(id) {
            self.prompt = None;
        }
        Ok(())
    }

    /// Give a unit standing orders.
    pub fn cmd_set_orders(&mut self, id: UnitId, orders: Orders) -> Result<(), String> {
        self.own_unit(id)?;
        let u = self.units[id].clone();
        let geom = self.map.geom;
        match orders {
            Orders::Random if u.kind == UnitType::Fighter => return Err("Fighters can't go random".into()),
            Orders::GoHome if u.kind == UnitType::Army => return Err("Armies have no home to go to".into()),
            Orders::Escort(r)
                if (!u.kind.is_ship() || !self.unit_valid(r) || !self.units[r.id].kind.is_ship() || r.id == id) => {
                    return Err("Only ships can escort ships".into());
                }
            Orders::Load if u.kind.cargo().is_none() => return Err("Only transports and carriers load".into()),
            Orders::Sentry if u.kind == UnitType::Fighter && !self.in_city(id) && self.carrier_of(id).is_none() => {
                return Err("Fighters can only sentry in a city or on a carrier".into())
            }
            Orders::MoveTo(t) if u.kind == UnitType::Fighter && geom.dist(u.loc, t) > u.fuel as usize => {
                return Err("Out of fighter range".into())
            }
            Orders::Patrol { to, .. } if u.kind == UnitType::Fighter => {
                let on_carrier = self.carrier_of(id).is_some();
                if !self.in_city(id) && !on_carrier {
                    return Err("Fighters patrol from a city or carrier".into());
                }
                let limit = if on_carrier { 6 } else { 10 };
                if geom.dist(u.loc, to) > limit {
                    return Err(format!("Patrol range is {limit} squares"));
                }
            }
            _ => {}
        }
        if let Orders::MoveTo(t) = orders {
            // A fighter told to fly to a carrier chases it.
            if u.kind == UnitType::Fighter {
                if let Some(c) = self.top_unit(t) {
                    if self.units[c].kind == UnitType::Carrier && self.units[c].owner == u.owner {
                        let r = self.units[c].unit_ref(c);
                        self.units[id].orders = Orders::FollowCarrier(r);
                        self.after_orders(id);
                        return Ok(());
                    }
                }
            }
        }
        self.units[id].orders = orders;
        if orders == Orders::Load {
            self.load_adjacent(id);
        }
        self.after_orders(id);
        Ok(())
    }

    fn after_orders(&mut self, id: UnitId) {
        self.units[id].fuel_warned = false;
        if self.prompt.map(|(u, _)| u) == Some(id) {
            self.prompt = None;
        }
    }

    /// Clear orders on a unit, or on every unit in a city if `loc` is a city.
    pub fn cmd_clear_orders_at(&mut self, loc: Loc) -> Result<(), String> {
        let p = self.current;
        let ids: Vec<UnitId> = if self.city_at(loc).is_some() {
            self.at[loc].iter().copied().filter(|&i| self.units[i].owner == p).collect()
        } else {
            self.top_unit(loc).filter(|&i| self.units[i].owner == p).into_iter().collect()
        };
        if ids.is_empty() {
            return Err("Nothing to clear".into());
        }
        for i in ids {
            self.units[i].orders = Orders::None;
        }
        Ok(())
    }

    /// Unload Ship: wake everything aboard (or everything in a city).
    pub fn cmd_unload(&mut self, id: UnitId) -> Result<usize, String> {
        self.own_unit(id)?;
        let loc = self.units[id].loc;
        let p = self.current;
        let woken: Vec<UnitId> = if self.city_at(loc).is_some() {
            self.at[loc].iter().copied().filter(|&i| self.units[i].owner == p && i != id).collect()
        } else {
            if self.units[id].kind.cargo().is_none() {
                return Err("Not a transport or carrier".into());
            }
            self.cargo(id)
        };
        for &i in &woken {
            self.units[i].orders = Orders::None;
        }
        if self.units[id].orders == Orders::Load {
            self.units[id].orders = Orders::None;
        }
        Ok(woken.len())
    }

    /// Survey mode Activate: move this unit now.
    pub fn cmd_activate(&mut self, id: UnitId) -> Result<(), String> {
        self.own_unit(id)?;
        let u = &mut self.units[id];
        if u.moved || u.moves_left == 0 {
            return Err("That unit has already moved".into());
        }
        u.orders = Orders::None;
        u.waiting = false;
        self.prompt = Some((id, WakeReason::NoOrders));
        self.last_loc = Some(self.units[id].loc);
        Ok(())
    }

    pub fn cmd_set_production(&mut self, ci: usize, kind: UnitType) -> Result<(), String> {
        let c = self.cities.get(ci).ok_or("No such city")?;
        if c.owner != self.current {
            return Err("Not your city".into());
        }
        if !self.can_build(ci, kind) {
            return Err("Inland cities can only build armies and fighters".into());
        }
        self.set_production_internal(ci, Some(kind));
        Ok(())
    }

    /// Set or clear a city's flight path for fighters.
    pub fn cmd_set_flight_path(&mut self, ci: usize, target: Option<Loc>) -> Result<(), String> {
        let c = self.cities.get(ci).ok_or("No such city")?;
        if c.owner != self.current {
            return Err("Not your city".into());
        }
        if let Some(t) = target {
            let tc = self.city_at(t).ok_or("Flight paths must end at a city")?;
            if self.cities[tc].owner != self.current || tc == ci {
                return Err("Flight paths must end at another of your cities".into());
            }
            if self.map.geom.dist(c.loc, t) > self.rules.fighter_fuel() as usize {
                return Err("Too far for a fighter".into());
            }
        }
        self.cities[ci].flight_path = target;
        Ok(())
    }

    /// A unit reference for building Escort / FollowCarrier orders.
    pub fn unit_ref(&self, id: UnitId) -> UnitRef {
        self.units[id].unit_ref(id)
    }
}
