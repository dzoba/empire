//! Moving units, combat, and capturing cities.

use crate::event::{Event, LossCause};
use crate::game::{Game, Observer};
use crate::geom::{Dir, Loc};
use crate::player::CombatRecord;
use crate::types::{Terrain, UnitType};
use crate::unit::{Orders, UnitId};
use serde::{Deserialize, Serialize};

/// What a single step would do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveKind {
    Move,
    EnterCity,
    Board(UnitId),
    LandInCity,
    LandOnCarrier(UnitId),
    Dock,
    Attack(UnitId),
    Bombard(UnitId),
    AttackCity(usize),
}

/// Why a step is not allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveError {
    NoUnit,
    NoMovesLeft,
    OffMap,
    Occupied,
    /// Armies can't walk into the sea.
    WouldDrown,
    /// Ships can't sail onto land.
    WouldRunAground,
    /// Fighters can't land in, and ships can't enter, cities they don't own.
    HostileCity,
    /// This unit type can't attack that target.
    CannotAttack,
    /// Armies can't attack ships from aboard a transport.
    AttackFromTransport,
    TransportFull,
    CarrierFull,
}

impl MoveError {
    pub fn message(self) -> &'static str {
        match self {
            MoveError::NoUnit => "No unit",
            MoveError::NoMovesLeft => "No moves left",
            MoveError::OffMap => "Can't move off the map",
            MoveError::Occupied => "Square is occupied",
            MoveError::WouldDrown => "Armies can't swim",
            MoveError::WouldRunAground => "Ships can't sail on land",
            MoveError::HostileCity => "Not your city",
            MoveError::CannotAttack => "Can't attack that",
            MoveError::AttackFromTransport => "Can't attack ships from a transport",
            MoveError::TransportFull => "Transport is full",
            MoveError::CarrierFull => "Carrier is full",
        }
    }
}

/// What actually happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepOutcome {
    Moved,
    Boarded,
    Landed,
    Docked,
    WonBattle,
    LostBattle,
    CapturedCity,
    Repelled,
    Destroyed(LossCause),
}

impl Game {
    /// Classify a one-square move without performing it.
    pub fn classify_move(&self, id: UnitId, dir: Dir) -> Result<MoveKind, MoveError> {
        let u = self.units.get(id).filter(|u| u.alive).ok_or(MoveError::NoUnit)?;
        let geom = self.map.geom;
        let dest = geom.step(u.loc, dir).ok_or(MoveError::OffMap)?;
        if !geom.is_playable(dest) {
            return Err(MoveError::OffMap);
        }
        let terrain = self.map.get(dest);
        let st = self.rules.stats(u.kind);
        let here_sea = self.map.get(u.loc) == Terrain::Sea;

        if terrain == Terrain::City {
            let ci = self.city_at(dest).expect("city index");
            let city = &self.cities[ci];
            if city.owner == u.owner {
                return Ok(match u.kind {
                    UnitType::Army => MoveKind::EnterCity,
                    UnitType::Fighter => MoveKind::LandInCity,
                    _ => MoveKind::Dock,
                });
            }
            return match u.kind {
                UnitType::Army => Ok(MoveKind::AttackCity(ci)),
                _ => Err(MoveError::HostileCity),
            };
        }

        if let Some(o) = self.top_unit(dest) {
            let other = &self.units[o];
            if other.owner == u.owner {
                return match (u.kind, other.kind) {
                    (UnitType::Army, UnitType::Transport) if terrain == Terrain::Sea => {
                        if self.is_full(o) {
                            Err(MoveError::TransportFull)
                        } else {
                            Ok(MoveKind::Board(o))
                        }
                    }
                    (UnitType::Fighter, UnitType::Carrier) => {
                        if self.is_full(o) {
                            Err(MoveError::CarrierFull)
                        } else {
                            Ok(MoveKind::LandOnCarrier(o))
                        }
                    }
                    _ => Err(MoveError::Occupied),
                };
            }
            // Enemy unit.
            let target_on_sea = terrain == Terrain::Sea;
            return match u.kind {
                UnitType::Army => {
                    if !target_on_sea {
                        Ok(MoveKind::Attack(o))
                    } else if here_sea {
                        Err(MoveError::AttackFromTransport)
                    } else if other.kind == UnitType::Submarine {
                        Err(MoveError::WouldDrown)
                    } else {
                        Ok(MoveKind::Attack(o))
                    }
                }
                UnitType::Fighter => Ok(MoveKind::Attack(o)),
                _ => {
                    if target_on_sea {
                        if u.kind == UnitType::Submarine && other.kind == UnitType::Fighter {
                            Err(MoveError::CannotAttack)
                        } else {
                            Ok(MoveKind::Attack(o))
                        }
                    } else if st.bombards && other.kind == UnitType::Army && st.attacks_armies {
                        Ok(MoveKind::Bombard(o))
                    } else if other.kind == UnitType::Army || other.kind == UnitType::Fighter {
                        Err(MoveError::CannotAttack)
                    } else {
                        Err(MoveError::WouldRunAground)
                    }
                }
            };
        }

        match (u.kind, terrain) {
            (UnitType::Army, Terrain::Sea) => Err(MoveError::WouldDrown),
            (UnitType::Army, _) => Ok(MoveKind::Move),
            (UnitType::Fighter, _) => Ok(MoveKind::Move),
            (_, Terrain::Sea) => Ok(MoveKind::Move),
            _ => Err(MoveError::WouldRunAground),
        }
    }

    /// True if the step is legal but likely to cost the unit: a fighter
    /// running out of fuel, or an army attacking a ship (it drowns even if it wins).
    pub fn move_is_risky(&self, id: UnitId, dir: Dir) -> bool {
        let Ok(kind) = self.classify_move(id, dir) else { return false };
        let u = &self.units[id];
        match kind {
            MoveKind::Move | MoveKind::Attack(_) if u.kind == UnitType::Fighter => u.fuel <= 1,
            MoveKind::Attack(o) if u.kind == UnitType::Army => self.map.get(self.units[o].loc) == Terrain::Sea,
            _ => false,
        }
    }

    /// Perform one step for a unit. Consumes one move.
    pub fn step_unit(&mut self, id: UnitId, dir: Dir) -> Result<StepOutcome, MoveError> {
        if !self.units.get(id).map(|u| u.alive).unwrap_or(false) {
            return Err(MoveError::NoUnit);
        }
        if self.units[id].moves_left == 0 {
            return Err(MoveError::NoMovesLeft);
        }
        let kind = self.classify_move(id, dir)?;
        let geom = self.map.geom;
        let from = self.units[id].loc;
        let dest = geom.step(from, dir).expect("classified");
        self.units[id].moves_left -= 1;

        let outcome = match kind {
            MoveKind::Move | MoveKind::EnterCity => {
                self.move_with_cargo(id, dest);
                StepOutcome::Moved
            }
            MoveKind::Board(_) => {
                self.relocate(id, dest);
                let u = &mut self.units[id];
                u.orders = Orders::Sentry;
                u.moves_left = 0;
                let desc = self.unit_desc(id);
                self.emit(Event::Boarded { loc: dest, unit: desc });
                StepOutcome::Boarded
            }
            MoveKind::LandInCity => {
                self.relocate(id, dest);
                let fuel = self.rules.fighter_fuel();
                let fp = self.city_at(dest).and_then(|c| self.cities[c].flight_path);
                let u = &mut self.units[id];
                u.fuel = fuel;
                u.fuel_warned = false;
                u.moves_left = 0;
                u.orders = match u.orders {
                    // Patrols and longer trips continue after refuelling.
                    o @ Orders::Patrol { .. } => o,
                    Orders::MoveTo(t) if t != dest => Orders::MoveTo(t),
                    _ => match fp {
                        Some(target) if target != dest => Orders::MoveTo(target),
                        _ => Orders::None,
                    },
                };
                let desc = self.unit_desc(id);
                self.emit(Event::Landed { loc: dest, unit: desc });
                StepOutcome::Landed
            }
            MoveKind::LandOnCarrier(_) => {
                self.relocate(id, dest);
                let fuel = self.rules.fighter_fuel();
                let u = &mut self.units[id];
                u.fuel = fuel;
                u.fuel_warned = false;
                u.moves_left = 0;
                if !matches!(u.orders, Orders::Patrol { .. }) {
                    u.orders = Orders::Sentry;
                }
                let desc = self.unit_desc(id);
                self.emit(Event::Landed { loc: dest, unit: desc });
                StepOutcome::Landed
            }
            MoveKind::Dock => {
                let cargo = self.cargo(id);
                self.relocate(id, dest);
                for c in cargo {
                    self.relocate(c, dest);
                    // Ships unload automatically when they dock.
                    self.units[c].orders = Orders::None;
                }
                self.units[id].moves_left = 0;
                if self.units[id].orders == Orders::Load {
                    self.units[id].orders = Orders::None;
                }
                let desc = self.unit_desc(id);
                self.emit(Event::Docked { loc: dest, unit: desc });
                StepOutcome::Docked
            }
            MoveKind::Attack(def) => {
                if self.fight(id, def, false) {
                    // Attacker won and advances into the square.
                    if self.units[id].kind == UnitType::Army && self.map.get(dest) == Terrain::Sea {
                        self.observe_unit(id);
                        self.destroy_unit(id, Some(LossCause::Drowned));
                        return Ok(StepOutcome::Destroyed(LossCause::Drowned));
                    }
                    self.move_with_cargo(id, dest);
                    StepOutcome::WonBattle
                } else {
                    StepOutcome::LostBattle
                }
            }
            MoveKind::Bombard(def) => {
                if self.fight(id, def, true) {
                    StepOutcome::WonBattle
                } else {
                    StepOutcome::LostBattle
                }
            }
            MoveKind::AttackCity(ci) => {
                if self.attack_city(id, ci) {
                    StepOutcome::CapturedCity
                } else {
                    StepOutcome::Repelled
                }
            }
        };

        if !self.units[id].alive {
            return Ok(outcome);
        }
        let loc = self.units[id].loc;
        let kind_now = self.units[id].kind;
        // Fighters burn fuel on every square that isn't a landing.
        if kind_now == UnitType::Fighter && !matches!(outcome, StepOutcome::Landed) && loc != from {
            let u = &mut self.units[id];
            u.fuel = u.fuel.saturating_sub(1);
            if u.fuel == 0 {
                self.observe_unit(id);
                self.destroy_unit(id, Some(LossCause::OutOfFuel));
                return Ok(StepOutcome::Destroyed(LossCause::OutOfFuel));
            }
        }
        // Badly damaged ships crawl.
        if kind_now.is_ship() {
            let max = self.rules.stats(kind_now).hits;
            if (self.units[id].hits as u32) * 2 <= max as u32 {
                let cap = self.rules.damaged_ship_speed;
                let used = self.rules.stats(kind_now).speed.saturating_sub(self.units[id].moves_left);
                if used >= cap {
                    self.units[id].moves_left = 0;
                }
            }
        }
        // Entering a city ends the turn's movement.
        if self.map.get(loc) == Terrain::City {
            self.units[id].moves_left = 0;
        }
        self.observe_unit(id);
        self.after_move(id);
        Ok(outcome)
    }

    /// Move a unit; transports and carriers bring their cargo along.
    fn move_with_cargo(&mut self, id: UnitId, dest: Loc) {
        let from = self.units[id].loc;
        let kind = self.units[id].kind;
        let owner = self.units[id].owner;
        let Some(cargo_type) = kind.cargo() else {
            self.relocate(id, dest);
            return;
        };
        let capacity = self.capacity(id);
        let in_city = self.map.get(from) == Terrain::City;
        let mut riders: Vec<UnitId> = self.at[from]
            .iter()
            .copied()
            .filter(|&o| o != id && self.units[o].owner == owner && self.units[o].kind == cargo_type)
            .collect();
        if in_city {
            // Leaving port: pick up idle or sentried units, up to capacity.
            riders.retain(|&o| matches!(self.units[o].orders, Orders::None | Orders::Sentry));
            riders.truncate(capacity);
        }
        self.relocate(id, dest);
        let mut lost = 0;
        for (i, r) in riders.into_iter().enumerate() {
            if i < capacity {
                self.relocate(r, dest);
                self.units[r].orders = Orders::Sentry;
            } else {
                self.destroy_unit(r, None);
                lost += 1;
            }
        }
        if lost > 0 {
            self.emit(Event::CargoLost { loc: dest, owner, kind: cargo_type, count: lost, cause: LossCause::Overloaded });
        }
    }

    /// Automatic follow-ups after a unit moves.
    fn after_move(&mut self, id: UnitId) {
        let geom = self.map.geom;
        let u = &self.units[id];
        let (owner, loc, kind) = (u.owner, u.loc, u.kind);
        // An army next to a loading transport climbs aboard.
        if kind == UnitType::Army && self.map.get(loc) == Terrain::Land {
            let transport = geom.neighbors(loc).find_map(|(_, l)| {
                self.top_unit(l).filter(|&t| {
                    let tu = &self.units[t];
                    tu.owner == owner
                        && tu.kind == UnitType::Transport
                        && tu.orders == Orders::Load
                        && self.map.get(l) == Terrain::Sea
                        && !self.is_full(t)
                })
            });
            if let Some(t) = transport {
                let tloc = self.units[t].loc;
                self.relocate(id, tloc);
                self.units[id].orders = Orders::Sentry;
                self.units[id].moves_left = 0;
                let desc = self.unit_desc(id);
                self.emit(Event::Boarded { loc: tloc, unit: desc });
                if self.is_full(t) {
                    self.units[t].orders = Orders::None;
                }
            }
        }
        // Fighters photograph enemy cities they fly past.
        if kind == UnitType::Fighter {
            let near: Vec<usize> = geom
                .neighbors(loc)
                .filter_map(|(_, l)| self.city_at(l))
                .filter(|&c| self.cities[c].owner != 0 && self.cities[c].owner != owner)
                .collect();
            for ci in near {
                let prod = self.cities[ci].production;
                let turn = self.turn;
                let p = self.player_mut(owner);
                p.recon.retain(|(c, _, _)| *c != ci);
                p.recon.push((ci, prod, turn));
                self.emit(Event::PhotoRecon { player: owner, city: ci, production: prod });
            }
        }
    }

    /// Resolve a battle. Returns true if the attacker wins.
    pub(crate) fn fight(&mut self, att: UnitId, def: UnitId, bombard: bool) -> bool {
        let (ak, dk) = (self.units[att].kind, self.units[def].kind);
        let (ao, dow) = (self.units[att].owner, self.units[def].owner);
        let ast = self.rules.stats(ak).clone();
        let dst = self.rules.stats(dk).clone();
        let a = self.player(ao).effective_combat() * ast.attack;
        let mut d = self.player(dow).effective_combat() * dst.defense;
        if bombard && dk == UnitType::Army {
            d *= self.rules.bombarded_army_defense;
        }
        let p_hit = if a + d > 0.0 { a / (a + d) } else { 0.5 };
        let mut ah = self.units[att].hits as i32;
        let mut dh = self.units[def].hits as i32;
        let attacker_won = loop {
            if self.rng.chance(p_hit) {
                dh -= ast.damage as i32;
                if dh <= 0 {
                    break true;
                }
            } else {
                ah -= dst.damage as i32;
                if ah <= 0 {
                    break false;
                }
            }
        };
        let loc = self.units[def].loc;
        let attacker = self.unit_desc(att);
        let defender = self.unit_desc(def);
        self.emit(Event::Combat { loc, attacker, defender: defender.clone(), attacker_won, bombardment: bombard });
        let turn = self.turn;
        self.player_mut(dow).combat_log.push(CombatRecord {
            turn,
            loc,
            attacker: ao,
            attacker_kind: Some(ak),
            defender_kind: Some(dk),
            defender_name: defender.name,
            defender_won: !attacker_won,
        });
        if attacker_won {
            self.units[att].hits = ah.max(1) as u8;
            self.player_mut(ao).stats.destroyed[dk.index()] += 1;
            let cargo: Vec<UnitId> = self.cargo(def);
            for c in &cargo {
                let ck = self.units[*c].kind;
                self.player_mut(ao).stats.destroyed[ck.index()] += 1;
            }
            self.destroy_unit(def, None);
            self.observe_unit(att);
        } else {
            self.units[def].hits = dh.max(1) as u8;
            self.player_mut(dow).stats.destroyed[ak.index()] += 1;
            self.observe_unit(att);
            self.destroy_unit(att, None);
            let dloc = self.units[def].loc;
            self.observe(dow, dloc, Observer::Unit(dk));
        }
        attacker_won
    }

    /// An army assaults a city. The army is always used up: it becomes the
    /// garrison if the assault succeeds. Returns true on capture.
    pub(crate) fn attack_city(&mut self, army: UnitId, ci: usize) -> bool {
        let attacker = self.units[army].owner;
        let defender = self.cities[ci].owner;
        let loc = self.cities[ci].loc;
        let a = self.player(attacker).effective_combat() * self.rules.stats(UnitType::Army).attack;
        let d = if defender == 0 { self.rules.neutral_city_efficiency } else { self.player(defender).effective_combat() };
        let p = if a + d > 0.0 { a / (a + d) } else { 0.5 };
        let captured = self.rng.chance(p);
        let army_name = self.units[army].name.clone();
        // The army leaves the field either way.
        self.units[army].alive = false;
        let aloc = self.units[army].loc;
        self.index_remove(aloc, army);
        if self.prompt.map(|(u, _)| u) == Some(army) {
            self.prompt = None;
        }
        if captured {
            let doomed: Vec<UnitId> = self.at[loc].iter().copied().filter(|&o| self.units[o].owner != attacker).collect();
            for o in doomed {
                let k = self.units[o].kind;
                self.player_mut(attacker).stats.destroyed[k.index()] += 1;
                self.destroy_unit(o, Some(LossCause::CityCaptured));
            }
            let c = &mut self.cities[ci];
            c.owner = attacker;
            c.production = None;
            c.flight_path = None;
            c.just_produced = false;
            self.player_mut(attacker).stats.cities_captured += 1;
            if defender != 0 {
                self.player_mut(defender).stats.cities_lost += 1;
                let d = self.player_mut(defender);
                d.view[loc] = crate::types::MapVal::city(attacker);
                d.view_rev += 1;
            }
            self.emit(Event::CityCaptured { loc, city: ci, by: attacker, from: defender, unit: army_name });
            self.observe(attacker, loc, Observer::City);
        } else {
            self.player_mut(attacker).stats.lost[UnitType::Army.index()] += 1;
            if defender != 0 {
                self.player_mut(defender).stats.destroyed[UnitType::Army.index()] += 1;
            }
            self.emit(Event::CityRepelled { loc, city: ci, attacker, defender, unit: army_name });
            self.observe(attacker, aloc, Observer::Unit(UnitType::Army));
        }
        captured
    }
}
