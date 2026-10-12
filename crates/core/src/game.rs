//! The game state and turn sequencing.
//!
//! The UI drives the game by calling [`Game::advance`], which runs computer
//! players and automatic orders until a human decision is needed, then returns
//! a [`Pending`] describing it. The UI answers with a command (move, orders,
//! production...) and calls `advance` again.

use crate::city::City;
use crate::config::{GameConfig, MapSource};
use crate::event::{Event, LossCause, UnitDesc};
use crate::geom::{Geometry, Loc};
use crate::map::WorldMap;
use crate::mapgen;
use crate::names;
use crate::path::PathScratch;
use crate::player::{Intel, Player, PlayerStats};
use crate::rng::Rng;
use crate::types::{MapVal, PlayerId, Rules, Terrain, UnitType, MAX_PLAYERS};
use crate::unit::{Mission, Orders, Unit, UnitId, UnitRef};
use serde::{Deserialize, Serialize};

/// Who is looking: cities see everything adjacent; units have blind spots
/// (most ships can't see submarines, submarines can't see armies or fighters).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observer {
    City,
    Unit(UnitType),
}

/// Why a human unit is asking for orders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WakeReason {
    /// No standing orders.
    NoOrders,
    /// An enemy unit is adjacent.
    EnemyNear,
    /// An army is next to a city it doesn't own.
    CityNear,
    /// The unit's orders can't be carried out from here.
    Blocked,
    /// Reached its destination.
    Arrived,
    /// A fighter on Go Direction is at half fuel.
    LowFuel,
    /// A loading transport or carrier is full.
    Full,
    /// Moved with moves to spare.
    MovesLeft,
}

impl WakeReason {
    pub fn message(self) -> &'static str {
        match self {
            WakeReason::NoOrders => "",
            WakeReason::EnemyNear => "Enemy sighted!",
            WakeReason::CityNear => "City nearby",
            WakeReason::Blocked => "Path blocked",
            WakeReason::Arrived => "Arrived",
            WakeReason::LowFuel => "Half fuel remaining",
            WakeReason::Full => "Fully loaded",
            WakeReason::MovesLeft => "",
        }
    }
}

/// What the game is waiting for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Pending {
    /// Computer players are still moving; call `advance` again.
    Busy,
    /// Hot-seat: ask `player` to come to the screen, then call `ack_handoff`.
    HandOff { player: PlayerId },
    /// A human's city needs a production decision.
    CityProduction { player: PlayerId, city: usize },
    /// A human unit needs a move or orders.
    UnitOrders { player: PlayerId, unit: UnitId, reason: WakeReason },
    /// A beaten computer player offers to surrender to `to`.
    SurrenderOffer { from: PlayerId, to: PlayerId },
    GameOver { winner: Option<PlayerId> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Game {
    pub rules: Rules,
    pub map: WorldMap,
    pub cities: Vec<City>,
    pub units: Vec<Unit>,
    pub players: Vec<Player>,
    /// Turn number, starting at 1. A turn is complete when every player has moved.
    pub turn: u32,
    /// Whose turn it is.
    pub current: PlayerId,
    pub(crate) turn_begun: bool,
    pub(crate) handoff_done: bool,
    pub rng: Rng,
    pub(crate) next_serial: u32,
    pub(crate) ships_named: u32,
    pub winner: Option<PlayerId>,
    pub game_over: bool,
    pub over_population: bool,
    /// The human unit currently asking for orders.
    pub prompt: Option<(UnitId, WakeReason)>,
    /// Where the last human prompt was, to pick the next unit nearby.
    pub last_loc: Option<Loc>,
    pub(crate) surrender_pending: Option<(PlayerId, PlayerId)>,
    pub(crate) surrender_declined: Vec<PlayerId>,
    #[serde(skip)]
    pub(crate) city_index: Vec<Option<usize>>,
    #[serde(skip)]
    pub(crate) at: Vec<Vec<UnitId>>,
    #[serde(skip)]
    pub(crate) scratch: PathScratch,
    #[serde(skip)]
    pub(crate) events: Vec<Event>,
    /// The computer player's plan for the turn in progress. It is saved only
    /// while a computer turn is under way, so a game saved then resumes exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ai_plan: Option<Box<crate::ai::Plan>>,
}

impl Game {
    // ------------------------------------------------------------------
    // Construction and persistence

    pub fn new(config: &GameConfig) -> Result<Game, String> {
        if config.players.len() < 2 || config.players.len() > MAX_PLAYERS {
            return Err(format!("need 2 to {MAX_PLAYERS} players"));
        }
        let mut rng = Rng::new(config.seed);
        let mut map = match &config.map {
            MapSource::Random(params) => mapgen::generate(params, &mut rng),
            MapSource::Text(text) => WorldMap::parse(text)?,
        };
        if config.randomize_orientation && matches!(config.map, MapSource::Text(_)) {
            if rng.coin() {
                map.rotate_180();
            }
            if rng.coin() {
                map.mirror_horizontal();
            }
        }
        let geom = map.geom;

        // Cities, in shuffled order like the original.
        let mut locs = map.city_locs();
        if locs.len() < config.players.len() {
            return Err("map has fewer cities than players".into());
        }
        rng.shuffle(&mut locs);
        let generated = names::city_names(locs.len(), &mut rng);
        let cities: Vec<City> = locs
            .iter()
            .enumerate()
            .map(|(i, &loc)| City {
                loc,
                name: map
                    .city_names
                    .iter()
                    .find(|(l, _)| *l == loc)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| generated[i].clone()),
                owner: 0,
                production: None,
                ready_turn: 0,
                just_produced: false,
                flight_path: None,
                port: map.sea_neighbors(loc) > 0,
            })
            .collect();

        let players: Vec<Player> = config
            .players
            .iter()
            .enumerate()
            .map(|(i, pc)| Player {
                id: (i + 1) as PlayerId,
                name: pc.name.clone(),
                control: pc.control,
                production_eff: pc.production_eff.clamp(1, 100),
                combat_eff: pc.combat_eff.clamp(1, 100),
                handicap: pc.handicap.clamp(1, 100),
                view: vec![MapVal::UNKNOWN; geom.size()],
                view_rev: 0,
                defeated: false,
                stats: PlayerStats::default(),
                combat_log: Vec::new(),
                last_combat_log: Vec::new(),
                recon: Vec::new(),
                contact: false,
                intel: Intel::default(),
                unit_counter: [0; 8],
            })
            .collect();

        let mut game = Game {
            rules: config.rules.clone(),
            map,
            cities,
            units: Vec::new(),
            players,
            turn: 1,
            current: 1,
            turn_begun: false,
            handoff_done: false,
            rng,
            next_serial: 1,
            ships_named: 0,
            winner: None,
            game_over: false,
            over_population: false,
            prompt: None,
            last_loc: None,
            surrender_pending: None,
            surrender_declined: Vec::new(),
            city_index: Vec::new(),
            at: Vec::new(),
            scratch: PathScratch::default(),
            events: Vec::new(),
            ai_plan: None,
        };
        game.rebuild_indices();
        game.assign_start_cities()?;
        Ok(game)
    }

    fn assign_start_cities(&mut self) -> Result<(), String> {
        let (mass, _) = self.map.land_masses();
        let n = self.players.len();
        let mut used_masses: Vec<usize> = Vec::new();
        for pi in 0..n {
            let p = (pi + 1) as PlayerId;
            let mut choice = None;
            // Prefer a city on a land mass no other player starts on.
            for strict in [true, false] {
                for _ in 0..400 {
                    let ci = self.rng.below_usize(self.cities.len());
                    let c = &self.cities[ci];
                    if c.owner != 0 || self.map.sea_neighbors(c.loc) == 8 {
                        continue;
                    }
                    if strict && used_masses.contains(&mass[c.loc]) {
                        continue;
                    }
                    choice = Some(ci);
                    break;
                }
                if choice.is_some() {
                    break;
                }
            }
            let ci = choice
                .or_else(|| self.cities.iter().position(|c| c.owner == 0))
                .ok_or("no free city for a player")?;
            used_masses.push(mass[self.cities[ci].loc]);
            self.cities[ci].owner = p;
            let loc = self.cities[ci].loc;
            // Humans choose their first production; the computer chooses on its first turn.
            self.observe(p, loc, Observer::City);
        }
        Ok(())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("game serializes")
    }

    pub fn from_json(s: &str) -> Result<Game, String> {
        let mut g: Game = serde_json::from_str(s).map_err(|e| e.to_string())?;
        g.rebuild_indices();
        Ok(g)
    }

    pub(crate) fn rebuild_indices(&mut self) {
        let n = self.map.geom.size();
        self.city_index = vec![None; n];
        for (i, c) in self.cities.iter().enumerate() {
            self.city_index[c.loc] = Some(i);
        }
        self.at = vec![Vec::new(); n];
        for (id, u) in self.units.iter().enumerate() {
            if u.alive {
                self.at[u.loc].push(id);
            }
        }
    }

    // ------------------------------------------------------------------
    // Queries

    pub fn geom(&self) -> Geometry {
        self.map.geom
    }

    pub fn player(&self, p: PlayerId) -> &Player {
        &self.players[p as usize - 1]
    }

    pub fn player_mut(&mut self, p: PlayerId) -> &mut Player {
        &mut self.players[p as usize - 1]
    }

    pub fn num_players(&self) -> usize {
        self.players.len()
    }

    pub fn unit(&self, id: UnitId) -> &Unit {
        &self.units[id]
    }

    pub fn unit_valid(&self, r: UnitRef) -> bool {
        r.id < self.units.len() && self.units[r.id].alive && self.units[r.id].serial == r.serial
    }

    pub fn city_at(&self, loc: Loc) -> Option<usize> {
        self.city_index.get(loc).copied().flatten()
    }

    pub fn units_at(&self, loc: Loc) -> &[UnitId] {
        &self.at[loc]
    }

    /// The unit shown at a non-city cell: a transport or carrier if one is
    /// there (its cargo is hidden inside), otherwise the single unit.
    pub fn top_unit(&self, loc: Loc) -> Option<UnitId> {
        let here = &self.at[loc];
        if here.is_empty() || self.map.get(loc) == Terrain::City {
            return None;
        }
        here.iter()
            .copied()
            .find(|&id| self.units[id].kind.cargo().is_some())
            .or_else(|| here.first().copied())
    }

    /// The true contents of a cell, encoded as a map value.
    pub fn truth(&self, loc: Loc) -> MapVal {
        match self.map.get(loc) {
            Terrain::City => {
                let owner = self.city_at(loc).map(|c| self.cities[c].owner).unwrap_or(0);
                MapVal::city(owner)
            }
            t => match self.top_unit(loc) {
                Some(id) => {
                    let u = &self.units[id];
                    MapVal::unit(u.owner, u.kind, t == Terrain::Sea)
                }
                None => MapVal::terrain(t),
            },
        }
    }

    pub fn in_city(&self, id: UnitId) -> bool {
        self.map.get(self.units[id].loc) == Terrain::City
    }

    /// Units aboard a transport or carrier (none while it is in a city).
    pub fn cargo(&self, ship: UnitId) -> Vec<UnitId> {
        let s = &self.units[ship];
        let Some(ct) = s.kind.cargo() else { return Vec::new() };
        if self.map.get(s.loc) == Terrain::City {
            return Vec::new();
        }
        self.at[s.loc]
            .iter()
            .copied()
            .filter(|&id| id != ship && self.units[id].kind == ct && self.units[id].owner == s.owner)
            .collect()
    }

    pub fn capacity(&self, ship: UnitId) -> usize {
        let s = &self.units[ship];
        s.hits as usize * self.rules.stats(s.kind).cargo_per_hit as usize
    }

    pub fn is_full(&self, ship: UnitId) -> bool {
        self.cargo(ship).len() >= self.capacity(ship)
    }

    /// The transport or carrier a unit is riding, if any.
    pub fn carrier_of(&self, id: UnitId) -> Option<UnitId> {
        let u = &self.units[id];
        if self.map.get(u.loc) == Terrain::City {
            return None;
        }
        self.at[u.loc]
            .iter()
            .copied()
            .find(|&o| o != id && self.units[o].owner == u.owner && self.units[o].kind.cargo() == Some(u.kind))
    }

    pub fn unit_desc(&self, id: UnitId) -> UnitDesc {
        let u = &self.units[id];
        UnitDesc { owner: u.owner, kind: u.kind, name: u.name.clone() }
    }

    pub fn max_units(&self) -> usize {
        if self.rules.max_units > 0 {
            self.rules.max_units
        } else {
            500.max(self.map.geom.playable_cells() / 11)
        }
    }

    pub fn live_unit_count(&self) -> usize {
        self.units.iter().filter(|u| u.alive).count()
    }

    pub fn player_units(&self, p: PlayerId) -> impl Iterator<Item = UnitId> + '_ {
        self.units.iter().enumerate().filter(move |(_, u)| u.alive && u.owner == p).map(|(i, _)| i)
    }

    pub fn player_cities(&self, p: PlayerId) -> impl Iterator<Item = usize> + '_ {
        self.cities.iter().enumerate().filter(move |(_, c)| c.owner == p).map(|(i, _)| i)
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub(crate) fn emit(&mut self, e: Event) {
        self.events.push(e);
    }

    pub fn humans(&self) -> usize {
        self.players.iter().filter(|p| p.is_human() && !p.defeated).count()
    }

    /// Effective speed for a unit at the start of a turn.
    pub fn speed_of(&self, id: UnitId) -> u8 {
        let u = &self.units[id];
        let st = self.rules.stats(u.kind);
        if u.kind.is_ship() && (u.hits as u32) * 2 <= st.hits as u32 {
            self.rules.damaged_ship_speed.min(st.speed)
        } else {
            st.speed
        }
    }

    // ------------------------------------------------------------------
    // Scenario building (tests, custom scenarios, mods)

    /// Put a new unit on the map. Returns None if the unit limit is reached.
    pub fn place_unit(&mut self, owner: PlayerId, kind: UnitType, loc: Loc) -> Option<UnitId> {
        let id = self.spawn_unit(owner, kind, loc)?;
        self.observe_unit(id);
        Some(id)
    }

    /// Give a city to a player (0 = neutral). Production is cleared.
    pub fn set_city_owner(&mut self, ci: usize, owner: PlayerId) {
        self.cities[ci].owner = owner;
        self.cities[ci].production = None;
        if owner != 0 {
            let loc = self.cities[ci].loc;
            self.observe(owner, loc, Observer::City);
        }
    }

    /// Remove every unit and make every city neutral.
    pub fn clear_board(&mut self) {
        for id in 0..self.units.len() {
            if self.units[id].alive {
                self.destroy_unit(id, None);
            }
        }
        for p in self.players.iter_mut() {
            p.stats = PlayerStats::default();
        }
        for c in self.cities.iter_mut() {
            c.owner = 0;
            c.production = None;
        }
    }

    /// Remove a unit without counting it as a loss.
    pub fn remove_unit(&mut self, id: UnitId) {
        if self.units[id].alive {
            let owner = self.units[id].owner;
            let kind = self.units[id].kind;
            self.destroy_unit(id, None);
            self.player_mut(owner).stats.lost[kind.index()] -= 1;
        }
    }

    /// Set production without the ownership checks of `cmd_set_production`.
    pub fn set_city_owner_production(&mut self, ci: usize, kind: UnitType) {
        self.set_production_internal(ci, Some(kind));
    }

    #[doc(hidden)]
    pub fn rebuild_for_test(&mut self) {
        self.rebuild_indices();
    }

    #[doc(hidden)]
    pub fn end_turn_for_test(&mut self) {
        let p = self.current;
        if !self.turn_begun {
            self.begin_turn(p);
        }
        self.end_turn(p);
        let q = self.current;
        self.begin_turn(q);
    }

    #[doc(hidden)]
    pub fn check_victory_for_test(&mut self) {
        self.check_victory();
    }

    #[doc(hidden)]
    pub fn observe_for_test(&mut self, p: PlayerId, at: Loc) {
        self.observe(p, at, Observer::City);
    }

    #[doc(hidden)]
    /// Run one human unit's standing orders; returns why it stopped, if it needs input.
    pub fn advance_unit_for_test(&mut self, id: UnitId) -> Option<WakeReason> {
        self.run_orders_pub(id)
    }

    // ------------------------------------------------------------------
    // Unit bookkeeping

    pub(crate) fn spawn_unit(&mut self, owner: PlayerId, kind: UnitType, loc: Loc) -> Option<UnitId> {
        if self.live_unit_count() >= self.max_units() {
            return None;
        }
        let slot = self.units.iter().position(|u| !u.alive).unwrap_or(self.units.len());
        let st = self.rules.stats(kind).clone();
        let counter = {
            let p = self.player_mut(owner);
            p.unit_counter[kind.index()] += 1;
            p.stats.built[kind.index()] += 1;
            p.unit_counter[kind.index()]
        };
        let name = if kind.is_ship() {
            self.ships_named += 1;
            names::ship_name(self.ships_named - 1 + self.rng.below(7))
        } else {
            format!("{} {}", crate::geom_names::ordinal(counter), kind.name())
        };
        let unit = Unit {
            alive: true,
            serial: self.next_serial,
            owner,
            kind,
            loc,
            hits: st.hits,
            fuel: st.fuel.unwrap_or(0),
            moves_left: st.speed,
            moved: false,
            waiting: false,
            orders: Orders::None,
            fuel_warned: false,
            name,
            mission: Mission::default(),
        };
        self.next_serial += 1;
        if slot == self.units.len() {
            self.units.push(unit);
        } else {
            self.units[slot] = unit;
        }
        self.index_add(loc, slot);
        Some(slot)
    }

    /// Each square's unit list is kept sorted by unit id, so its order never
    /// depends on history and a loaded game behaves exactly like the original.
    fn index_add(&mut self, loc: Loc, id: UnitId) {
        let list = &mut self.at[loc];
        if let Err(pos) = list.binary_search(&id) {
            list.insert(pos, id);
        }
    }

    pub(crate) fn index_remove(&mut self, loc: Loc, id: UnitId) {
        let list = &mut self.at[loc];
        if let Ok(pos) = list.binary_search(&id) {
            list.remove(pos);
        }
    }

    pub(crate) fn relocate(&mut self, id: UnitId, to: Loc) {
        let from = self.units[id].loc;
        self.index_remove(from, id);
        self.units[id].loc = to;
        self.index_add(to, id);
    }

    /// Remove a unit (and anything aboard it) from the game.
    pub(crate) fn destroy_unit(&mut self, id: UnitId, cause: Option<LossCause>) {
        if !self.units[id].alive {
            return;
        }
        let cargo = self.cargo(id);
        let loc = self.units[id].loc;
        let owner = self.units[id].owner;
        let kind = self.units[id].kind;
        if let Some(cause) = cause {
            let desc = self.unit_desc(id);
            self.emit(Event::UnitLost { loc, unit: desc, cause });
        }
        self.player_mut(owner).stats.lost[kind.index()] += 1;
        self.units[id].alive = false;
        self.index_remove(loc, id);
        if self.prompt.map(|(u, _)| u) == Some(id) {
            self.prompt = None;
        }
        if !cargo.is_empty() {
            let ck = self.units[cargo[0]].kind;
            let count = cargo.len() as u32;
            for c in cargo {
                self.destroy_unit(c, None);
            }
            self.emit(Event::CargoLost { loc, owner, kind: ck, count, cause: LossCause::WentDownWithShip });
        }
        // The owner learns what is there now.
        let truth = self.truth(loc);
        let pl = self.player_mut(owner);
        pl.view[loc] = truth;
        pl.view_rev += 1;
    }

    // ------------------------------------------------------------------
    // Sensing (fog of war)

    fn sees(observer: Observer, rules: &Rules, observed: MapVal) -> MapVal {
        let Some(t) = observed.unit_type() else { return observed };
        let (subs, land_air) = match observer {
            Observer::City => (true, true),
            Observer::Unit(k) => {
                let st = rules.stats(k);
                (st.sees_subs, st.sees_land_and_air)
            }
        };
        match t {
            UnitType::Submarine if !subs => MapVal::SEA,
            UnitType::Army | UnitType::Fighter if !land_air => {
                if observed.is_sea() {
                    MapVal::SEA
                } else {
                    MapVal::LAND
                }
            }
            _ => observed,
        }
    }

    fn observer_at(&self, loc: Loc) -> Option<(PlayerId, Observer)> {
        if let Some(ci) = self.city_at(loc) {
            let o = self.cities[ci].owner;
            return (o != 0).then_some((o, Observer::City));
        }
        self.top_unit(loc).map(|id| (self.units[id].owner, Observer::Unit(self.units[id].kind)))
    }

    /// Update `p`'s view of the 3x3 block around `at`, seen by `observer`.
    /// Enemies adjacent to `at` also get to see `at`.
    pub(crate) fn observe(&mut self, p: PlayerId, at: Loc, observer: Observer) {
        let geom = self.map.geom;
        let cells: Vec<Loc> = std::iter::once(at).chain(geom.neighbors(at).map(|(_, l)| l)).collect();
        for loc in cells {
            let truth = self.truth(loc);
            let seen = if loc == at || truth.owner() == p || truth.owner() == 0 {
                truth
            } else {
                Self::sees(observer, &self.rules, truth)
            };
            self.update_view(p, loc, seen);
            if seen.is_enemy_of(p) {
                self.player_mut(p).contact = true;
            }
            // Reverse probe: an adjacent enemy notices what is at `at`.
            if loc != at && truth.is_enemy_of(p) {
                if let Some((enemy, eobs)) = self.observer_at(loc) {
                    let tat = self.truth(at);
                    let seen_back = Self::sees(eobs, &self.rules, tat);
                    self.update_view(enemy, at, seen_back);
                    if seen_back.is_enemy_of(enemy) {
                        self.player_mut(enemy).contact = true;
                    }
                }
            }
        }
    }

    fn update_view(&mut self, p: PlayerId, loc: Loc, v: MapVal) {
        let pl = self.player_mut(p);
        if pl.view[loc] != v {
            pl.view[loc] = v;
            pl.view_rev += 1;
            if !pl.is_human() {
                self.ai_note(p, loc, v);
            }
        }
    }

    pub(crate) fn observe_unit(&mut self, id: UnitId) {
        let u = &self.units[id];
        if u.alive {
            let (o, l, k) = (u.owner, u.loc, u.kind);
            self.observe(o, l, Observer::Unit(k));
        }
    }

    // ------------------------------------------------------------------
    // Production

    pub(crate) fn set_production_internal(&mut self, ci: usize, kind: Option<UnitType>) {
        let owner = self.cities[ci].owner;
        let eff = if owner == 0 { 50 } else { self.player(owner).effective_production() };
        let c = &mut self.cities[ci];
        if c.production == kind && kind.is_some() {
            return;
        }
        c.production = kind;
        if let Some(k) = kind {
            c.ready_turn = self.turn + self.rules.build_time(k, true, eff);
        }
    }

    /// Can this city build this unit type? Inland cities build only armies and fighters.
    pub fn can_build(&self, ci: usize, kind: UnitType) -> bool {
        !kind.is_ship() || self.cities[ci].port
    }

    fn produce(&mut self, p: PlayerId) {
        let eff = self.player(p).effective_production();
        let cities: Vec<usize> = self.player_cities(p).collect();
        for ci in cities {
            self.cities[ci].just_produced = false;
            let loc = self.cities[ci].loc;
            self.observe(p, loc, Observer::City);
            let Some(kind) = self.cities[ci].production else { continue };
            if self.turn < self.cities[ci].ready_turn {
                continue;
            }
            match self.spawn_unit(p, kind, loc) {
                Some(id) => {
                    self.cities[ci].ready_turn = self.turn + self.rules.build_time(kind, false, eff);
                    self.cities[ci].just_produced = true;
                    if self.over_population {
                        self.over_population = false;
                    }
                    if kind == UnitType::Fighter {
                        if let Some(fp) = self.cities[ci].flight_path {
                            self.units[id].orders = Orders::MoveTo(fp);
                        }
                    }
                    self.emit(Event::Produced { player: p, city: ci, unit: id, kind });
                }
                None => {
                    if !self.over_population {
                        self.over_population = true;
                        self.emit(Event::OverPopulation { player: p });
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Turn sequencing

    fn begin_turn(&mut self, p: PlayerId) {
        self.turn_begun = true;
        self.handoff_done = false;
        self.prompt = None;
        {
            let pl = self.player_mut(p);
            pl.last_combat_log = std::mem::take(&mut pl.combat_log);
        }
        self.emit(Event::TurnStarted { player: p, turn: self.turn });

        // Ships docked in friendly cities are repaired.
        let ids: Vec<UnitId> = self.player_units(p).collect();
        for &id in &ids {
            let u = &self.units[id];
            if u.kind.is_ship() && self.map.get(u.loc) == Terrain::City {
                let max = self.rules.stats(u.kind).hits;
                let rep = self.rules.repair_per_turn;
                let u = &mut self.units[id];
                u.hits = (u.hits + rep).min(max);
            }
        }
        self.produce(p);
        let ids: Vec<UnitId> = self.player_units(p).collect();
        for id in ids {
            let speed = self.speed_of(id);
            let u = &mut self.units[id];
            u.moves_left = speed;
            u.moved = false;
            u.waiting = false;
            self.observe_unit(id);
        }
        if !self.player(p).is_human() {
            self.ai_begin_turn(p);
        }
    }

    fn end_turn(&mut self, p: PlayerId) {
        self.ai_plan = None;
        self.check_victory();
        self.turn_begun = false;
        self.prompt = None;
        if self.game_over {
            return;
        }
        let n = self.players.len() as PlayerId;
        let mut next = p;
        let mut wrapped = false;
        for _ in 0..n {
            if next >= n {
                next = 1;
                wrapped = true;
            } else {
                next += 1;
            }
            if !self.player(next).defeated {
                break;
            }
        }
        if wrapped {
            self.turn += 1;
        }
        self.current = next;
    }

    pub(crate) fn check_victory(&mut self) {
        let n = self.players.len();
        for pi in 0..n {
            let p = (pi + 1) as PlayerId;
            if self.player(p).defeated {
                continue;
            }
            let has_city = self.cities.iter().any(|c| c.owner == p);
            let has_army = self.units.iter().any(|u| u.alive && u.owner == p && u.kind == UnitType::Army);
            if !has_city && !has_army {
                self.player_mut(p).defeated = true;
                // Remaining units are lost with the empire.
                let ids: Vec<UnitId> = self.player_units(p).collect();
                for id in ids {
                    self.destroy_unit(id, None);
                }
                self.emit(Event::PlayerDefeated { player: p });
            }
        }
        let alive: Vec<PlayerId> = self.players.iter().filter(|p| !p.defeated).map(|p| p.id).collect();
        if alive.len() <= 1 {
            self.game_over = true;
            self.winner = alive.first().copied();
            self.emit(Event::GameOver { winner: self.winner });
        }
        self.check_surrender();
    }

    /// The last computer opponent offers to surrender when it can no longer win.
    fn check_surrender(&mut self) {
        if self.game_over || self.surrender_pending.is_some() {
            return;
        }
        let alive: Vec<PlayerId> = self.players.iter().filter(|p| !p.defeated).map(|p| p.id).collect();
        if alive.len() != 2 {
            return;
        }
        let (a, b) = (alive[0], alive[1]);
        let (comp, human) = match (self.player(a).is_human(), self.player(b).is_human()) {
            (false, true) => (a, b),
            (true, false) => (b, a),
            _ => return,
        };
        if self.surrender_declined.contains(&comp) {
            return;
        }
        let cc = self.player_cities(comp).count();
        let hc = self.player_cities(human).count();
        if hc >= 12 && cc * 8 <= hc {
            self.surrender_pending = Some((comp, human));
            self.emit(Event::SurrenderOffered { player: comp });
        }
    }

    pub fn answer_surrender(&mut self, accept: bool) {
        if let Some((comp, human)) = self.surrender_pending.take() {
            if accept {
                self.player_mut(comp).defeated = true;
                self.game_over = true;
                self.winner = Some(human);
                self.emit(Event::PlayerDefeated { player: comp });
                self.emit(Event::GameOver { winner: Some(human) });
            } else {
                self.surrender_declined.push(comp);
            }
        }
    }

    pub fn ack_handoff(&mut self) {
        self.handoff_done = true;
    }

    /// Run the game until a human decision is needed. `budget` limits how many
    /// computer unit moves happen before returning `Pending::Busy`, so a UI can
    /// keep drawing while the computer thinks.
    pub fn advance(&mut self, budget: usize) -> Pending {
        let mut work = 0usize;
        loop {
            if self.game_over {
                return Pending::GameOver { winner: self.winner };
            }
            if let Some((from, to)) = self.surrender_pending {
                return Pending::SurrenderOffer { from, to };
            }
            let p = self.current;
            if !self.turn_begun {
                self.begin_turn(p);
                work += 1;
            }
            let human = self.player(p).is_human();
            if human {
                if self.humans() > 1 && !self.handoff_done {
                    return Pending::HandOff { player: p };
                }
                if let Some(ci) = self.player_cities(p).find(|&c| self.cities[c].production.is_none()) {
                    return Pending::CityProduction { player: p, city: ci };
                }
                if let Some((id, reason)) = self.prompt {
                    let u = &self.units[id];
                    if u.alive && u.owner == p && !u.moved && u.moves_left > 0 {
                        return Pending::UnitOrders { player: p, unit: id, reason };
                    }
                    self.prompt = None;
                }
                match self.next_human_unit(p) {
                    Some((id, reason)) => {
                        self.prompt = Some((id, reason));
                        self.last_loc = Some(self.units[id].loc);
                        return Pending::UnitOrders { player: p, unit: id, reason };
                    }
                    None => {
                        self.end_turn(p);
                        continue;
                    }
                }
            } else {
                match self.next_unmoved(p) {
                    Some(id) => {
                        self.ai_move_unit(id);
                        let u = &mut self.units[id];
                        u.moved = true;
                        work += 1;
                        if self.game_over {
                            continue;
                        }
                        if work >= budget {
                            return Pending::Busy;
                        }
                    }
                    None => {
                        self.end_turn(p);
                        work += 1;
                        if work >= budget {
                            return Pending::Busy;
                        }
                    }
                }
            }
        }
    }

    fn next_unmoved(&self, p: PlayerId) -> Option<UnitId> {
        self.units
            .iter()
            .enumerate()
            .find(|(_, u)| u.alive && u.owner == p && !u.moved && u.moves_left > 0)
            .map(|(i, _)| i)
    }
}
