//! Players: control type, efficiency ratings, fog-of-war view, statistics,
//! and what a computer player remembers.

use crate::geom::Loc;
use crate::types::{MapVal, PlayerId, UnitType};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Skill {
    /// Plays at slightly reduced efficiency by default.
    Standard,
    /// Same ratings as a human.
    Expert,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Control {
    Human,
    Computer(Skill),
}

impl Control {
    pub fn is_human(self) -> bool {
        matches!(self, Control::Human)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerStats {
    /// Enemy units destroyed, by type.
    pub destroyed: [u32; 8],
    /// Own units lost, by type.
    pub lost: [u32; 8],
    pub cities_captured: u32,
    pub cities_lost: u32,
    /// Units built, by type.
    pub built: [u32; 8],
}

/// One attack against this player's units, for the Combat Report.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CombatRecord {
    pub turn: u32,
    pub loc: Loc,
    pub attacker: PlayerId,
    pub attacker_kind: Option<UnitType>,
    pub defender_kind: Option<UnitType>,
    pub defender_name: String,
    pub defender_won: bool,
}

/// An enemy unit the player has seen, and when.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sighting {
    pub loc: Loc,
    pub owner: PlayerId,
    pub kind: UnitType,
    pub turn: u32,
}

/// What the computer opponent remembers between turns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Intel {
    /// Recent enemy sightings, newest last.
    pub sightings: Vec<Sighting>,
    /// Where spare armies were last told to wait for transports.
    #[serde(default)]
    pub pickups: Vec<Loc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub control: Control,
    /// Percent; 50 is normal.
    pub production_eff: u8,
    /// Percent; 50 is normal.
    pub combat_eff: u8,
    /// Hidden multiplier on both efficiencies, in percent (100 = none). The
    /// original's "Standard" computer opponent plays with a handicap.
    #[serde(default = "full")]
    pub handicap: u8,
    /// What this player knows about each cell.
    pub view: Vec<MapVal>,
    /// Bumped whenever `view` changes, so a UI can skip redraws.
    #[serde(default)]
    pub view_rev: u64,
    pub defeated: bool,
    pub stats: PlayerStats,
    /// Attacks on our units since our last turn started.
    pub combat_log: Vec<CombatRecord>,
    /// Attacks shown by the Combat Report this turn.
    pub last_combat_log: Vec<CombatRecord>,
    /// Photo reconnaissance: city index -> (production seen, turn seen).
    pub recon: Vec<(usize, Option<UnitType>, u32)>,
    /// Has met an enemy unit or city.
    pub contact: bool,
    #[serde(default)]
    pub intel: Intel,
    /// Counter for numbering armies and fighters ("5th Army").
    pub unit_counter: [u32; 8],
}

fn full() -> u8 {
    100
}

impl Player {
    pub fn is_human(&self) -> bool {
        self.control.is_human()
    }

    /// Production efficiency after any handicap, in percent.
    pub fn effective_production(&self) -> u8 {
        ((self.production_eff as u32 * self.handicap as u32) / 100).max(1) as u8
    }

    /// Combat efficiency after any handicap, in percent.
    pub fn effective_combat(&self) -> f64 {
        self.combat_eff as f64 * self.handicap as f64 / 100.0
    }
}
