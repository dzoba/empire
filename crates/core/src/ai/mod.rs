//! The computer opponent.
//!
//! Once per turn the planner reads the player's own map (never the true
//! one), splits the known land into continents, and works out where armies
//! are still needed, where spare armies should wait for a ship, and how far
//! every square is from a friendly airfield. Cities then choose what to
//! build, and units choose targets one at a time by searching outward from
//! where they stand and scoring what they find. Transports run a small cycle:
//! gather armies at a pickup point, deliver them to a landing, unload.
//!
//! Two styles share the code. Expert weighs combat odds, escorts its
//! transports, sends damaged ships for repair, remembers enemy sightings and
//! builds a mixed navy. Standard plays a plainer game.

mod field;
mod odds;
mod plan;
mod production;
mod units;

pub use odds::duel;
pub use plan::{Continent, Pickup, Plan, Style};

use crate::game::Game;
use crate::geom::Loc;
use crate::player::Sighting;
use crate::types::{MapVal, PlayerId, UnitType};
use crate::unit::UnitId;

/// Enemy sightings remembered per player.
const SIGHTINGS_KEPT: usize = 48;
/// Sightings older than this many turns are dropped first.
const SIGHTING_TURNS: u32 = 12;

impl Game {
    /// Called after a computer player's units are refreshed at the start of its turn.
    pub(crate) fn ai_begin_turn(&mut self, p: PlayerId) {
        let mut plan = self.ai_build_plan(p);
        self.ai_choose_production(p, &mut plan);
        self.ai_plan = Some(Box::new(plan));
    }

    /// Move one computer unit for this turn.
    pub(crate) fn ai_move_unit(&mut self, id: UnitId) {
        if !self.units[id].alive {
            return;
        }
        let p = self.units[id].owner;
        let mut plan = match self.ai_plan.take() {
            Some(plan) if plan.turn == self.turn && plan.player == p => *plan,
            _ => self.ai_build_plan(p),
        };
        match self.units[id].kind {
            UnitType::Army => self.ai_army(&mut plan, id),
            UnitType::Fighter => self.ai_fighter(&mut plan, id),
            UnitType::Transport => self.ai_transport(&mut plan, id),
            UnitType::Carrier => self.ai_carrier(&mut plan, id),
            _ => self.ai_warship(&mut plan, id),
        }
        // A captured city needs orders, and changes the picture.
        if self.player_cities(p).any(|c| self.cities[c].production.is_none()) {
            let claims = std::mem::take(&mut plan.claims);
            let mut fresh = self.ai_build_plan(p);
            if claims.len() == fresh.claims.len() {
                fresh.claims = claims;
            }
            plan = fresh;
            self.ai_choose_production(p, &mut plan);
        }
        self.ai_plan = Some(Box::new(plan));
    }

    /// Remember an enemy unit a computer player has just seen.
    pub(crate) fn ai_note(&mut self, p: PlayerId, loc: Loc, v: MapVal) {
        let Some(kind) = v.unit_type() else { return };
        if !v.is_enemy_of(p) {
            return;
        }
        let turn = self.turn;
        let sightings = &mut self.player_mut(p).intel.sightings;
        sightings.push(Sighting { loc, owner: v.owner(), kind, turn });
        if sightings.len() > SIGHTINGS_KEPT {
            sightings.retain(|s| s.turn + SIGHTING_TURNS >= turn);
            if sightings.len() > SIGHTINGS_KEPT {
                let extra = sightings.len() - SIGHTINGS_KEPT;
                sightings.drain(..extra);
            }
        }
    }
}
