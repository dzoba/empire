//! Cities.

use crate::geom::Loc;
use crate::types::{PlayerId, UnitType};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct City {
    pub loc: Loc,
    pub name: String,
    /// 0 = neutral.
    pub owner: PlayerId,
    /// What the city is building. None = needs a decision.
    pub production: Option<UnitType>,
    /// Turn on which the unit in production completes.
    pub ready_turn: u32,
    /// True on the turn a unit was completed.
    pub just_produced: bool,
    /// Fighters landing here or built here fly on to this city.
    pub flight_path: Option<Loc>,
    /// A port touches the sea; only ports can build ships.
    pub port: bool,
}
