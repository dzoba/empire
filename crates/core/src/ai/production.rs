//! What the computer opponent's cities build.
//!
//! The order of priorities: armies where the city's own continent still has
//! cities to take or ground to explore; transports when armies are waiting
//! for a ride; a few fighters for scouting and defence; warships in
//! proportion to the fleet of transports; and otherwise more armies to ship
//! overseas.

use super::plan::Plan;
use crate::game::Game;
use crate::types::{PlayerId, UnitType};

impl Game {
    /// Set production for cities that need a decision this turn.
    pub(crate) fn ai_choose_production(&mut self, p: PlayerId, plan: &mut Plan) {
        let cities: Vec<usize> = self.player_cities(p).collect();
        for ci in cities {
            let c = &self.cities[ci];
            let decide = match c.production {
                None => true,
                // Standard reconsiders only now and then; Expert after every unit.
                Some(_) => c.just_produced && (plan.style.rethink || (self.turn as usize + ci).is_multiple_of(4)),
            };
            if !decide {
                continue;
            }
            let old = c.production;
            if let Some(k) = old {
                plan.building[k.index()] -= 1;
                if k == UnitType::Army {
                    if let Some(cid) = plan.cont_id(c.loc) {
                        plan.continents[cid].building_armies -= 1;
                    }
                }
            }
            let kind = self.ai_pick_production(p, plan, ci);
            plan.building[kind.index()] += 1;
            if kind == UnitType::Army {
                if let Some(cid) = plan.cont_id(self.cities[ci].loc) {
                    plan.continents[cid].building_armies += 1;
                }
            }
            if old != Some(kind) {
                self.set_production_internal(ci, Some(kind));
            }
        }
    }

    fn ai_pick_production(&self, p: PlayerId, plan: &Plan, ci: usize) -> UnitType {
        use UnitType::*;
        let city = &self.cities[ci];
        let style = plan.style;
        let total = |k: UnitType| plan.have[k.index()] + plan.building[k.index()];
        let cities = self.player_cities(p).count() as u32;
        let home = plan.cont_id(city.loc);

        // 1. The home continent still needs armies.
        if let Some(c) = home {
            if plan.continents[c].wanted() > 0 {
                return Army;
            }
        }

        // Armies wanted elsewhere, against armies with nothing to do here
        // and armies being built on continents that don't need them.
        let mut demand: u32 = 0;
        let mut supply: u32 = 0;
        for (i, c) in plan.continents.iter().enumerate() {
            if Some(i) != home && c.worth_landing() {
                demand += c.wanted();
            }
            supply += c.surplus();
            if c.wanted() == 0 {
                supply += c.building_armies;
            }
        }
        if plan.unknown > 0 {
            demand += 2;
        }
        // Don't build more armies for export than the transports can carry.
        let hulls = total(Transport);
        let export = demand > supply && supply < 6 * hulls + 4;
        let at_war = self.player(p).contact || self.turn > 50;

        let fighters_wanted = if cities < 2 {
            0
        } else if at_war {
            2 + (cities as f32 * style.fighters_per_city) as u32
        } else {
            1
        };

        // 2. Inland cities: armies for export, else fighters.
        if !city.port {
            return if export || total(Fighter) >= fighters_wanted.max(3) * 2 { Army } else { Fighter };
        }

        // 3. Ports: enough hulls to carry the spare armies, then armies while
        // other continents want them, then fighters and warships.
        let transports_wanted = if supply >= 2 || (supply >= 1 && hulls == 0) {
            supply.div_ceil(6).max(1).min(plan.ports.max(1))
        } else {
            0
        };
        if hulls < transports_wanted {
            return Transport;
        }
        if export && hulls > 0 {
            return Army;
        }
        if total(Fighter) < fighters_wanted {
            return Fighter;
        }
        if at_war {
            let mut want: Vec<(UnitType, u32)> = vec![(Destroyer, if style.full_navy { hulls.max(1) } else { hulls / 2 })];
            if style.full_navy {
                want.push((Submarine, cities / 5));
                want.push((Cruiser, cities / 8));
                if self.turn > 100 {
                    want.push((Battleship, cities / 12));
                }
            }
            if let Some(&(k, _)) = want.iter().find(|&&(k, n)| total(k) < n) {
                return k;
            }
            // Everything is at quota: lean on fighters and destroyers rather
            // than armies nobody can ship.
            if supply > 6 * hulls {
                return if total(Fighter) <= total(Destroyer) * 2 { Fighter } else { Destroyer };
            }
        }
        Army
    }
}
