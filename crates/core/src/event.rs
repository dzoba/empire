//! Things that happened, for the user interface to report.

use crate::geom::Loc;
use crate::types::{PlayerId, UnitType};
use crate::unit::UnitId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitDesc {
    pub owner: PlayerId,
    pub kind: UnitType,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LossCause {
    Drowned,
    OutOfFuel,
    ShotDown,
    Overloaded,
    CityCaptured,
    WentDownWithShip,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Event {
    TurnStarted { player: PlayerId, turn: u32 },
    Produced { player: PlayerId, city: usize, unit: UnitId, kind: UnitType },
    OverPopulation { player: PlayerId },
    Combat { loc: Loc, attacker: UnitDesc, defender: UnitDesc, attacker_won: bool, bombardment: bool },
    /// `unit` is the name of the army that took the city.
    CityCaptured { loc: Loc, city: usize, by: PlayerId, from: PlayerId, unit: String },
    CityRepelled { loc: Loc, city: usize, attacker: PlayerId, defender: PlayerId, unit: String },
    UnitLost { loc: Loc, unit: UnitDesc, cause: LossCause },
    CargoLost { loc: Loc, owner: PlayerId, kind: UnitType, count: u32, cause: LossCause },
    Boarded { loc: Loc, unit: UnitDesc },
    Landed { loc: Loc, unit: UnitDesc },
    Docked { loc: Loc, unit: UnitDesc },
    PhotoRecon { player: PlayerId, city: usize, production: Option<UnitType> },
    PlayerDefeated { player: PlayerId },
    SurrenderOffered { player: PlayerId },
    GameOver { winner: Option<PlayerId> },
}

impl Event {
    /// Players who should be told about this event.
    pub fn concerns(&self, p: PlayerId) -> bool {
        match self {
            Event::TurnStarted { player, .. } => *player == p,
            Event::Produced { player, .. } => *player == p,
            Event::OverPopulation { player } => *player == p,
            Event::Combat { attacker, defender, .. } => attacker.owner == p || defender.owner == p,
            Event::CityCaptured { by, from, .. } => *by == p || *from == p,
            Event::CityRepelled { attacker, defender, .. } => *attacker == p || *defender == p,
            Event::UnitLost { unit, .. } => unit.owner == p,
            Event::CargoLost { owner, .. } => *owner == p,
            Event::Boarded { unit, .. } | Event::Landed { unit, .. } | Event::Docked { unit, .. } => unit.owner == p,
            Event::PhotoRecon { player, .. } => *player == p,
            Event::PlayerDefeated { .. } | Event::SurrenderOffered { .. } | Event::GameOver { .. } => true,
        }
    }
}
