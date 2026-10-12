//! Units and their standing orders.

use crate::geom::{Dir, Loc};
use crate::types::{PlayerId, UnitType};
use serde::{Deserialize, Serialize};

/// Index into `Game::units`. Slots are reused after a unit dies, so long-lived
/// references also store the unit's `serial` (see [`UnitRef`]).
pub type UnitId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitRef {
    pub id: UnitId,
    pub serial: u32,
}

/// Standing orders a human gives a unit (the Orders menu of the original).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Orders {
    /// Ask the player every turn.
    #[default]
    None,
    /// Stay put until woken by an enemy or the Unload Ship order.
    Sentry,
    /// Keep moving in one direction.
    Direction(Dir),
    /// Move to a location.
    MoveTo(Loc),
    /// Fighter chasing a friendly carrier.
    FollowCarrier(UnitRef),
    /// Fighters: nearest friendly city or carrier in range. Ships: nearest port.
    GoHome,
    /// Wander at random without taking risks (not for fighters).
    Random,
    /// Shuttle between two locations.
    Patrol { from: Loc, to: Loc, outbound: bool },
    /// Follow another ship, staying close behind it.
    Escort(UnitRef),
    /// Transport or carrier: wait until full.
    Load,
}

impl Orders {
    pub fn describe(&self) -> String {
        match self {
            Orders::None => "None".into(),
            Orders::Sentry => "Sentry".into(),
            Orders::Direction(d) => format!("Go {}", crate::geom_names::dir_name(*d)),
            Orders::MoveTo(_) => "Move To".into(),
            Orders::FollowCarrier(_) => "Follow Carrier".into(),
            Orders::GoHome => "Go Home".into(),
            Orders::Random => "Go Random".into(),
            Orders::Patrol { .. } => "Patrol".into(),
            Orders::Escort(_) => "Escort".into(),
            Orders::Load => "Load Ship".into(),
        }
    }
}

/// What the computer opponent has a unit doing. Most units decide afresh
/// every turn (`Free`); transports and their escorts keep longer jobs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Task {
    #[default]
    Free,
    /// Transport: waiting at a pickup point for armies.
    Gather,
    /// Transport: sailing loaded toward a landing point.
    Deliver,
    /// Transport: at the landing point while armies go ashore.
    Unload,
    /// Ship: heading to port for repairs.
    Repair,
    /// Fighter: one-way reconnaissance flight into unexplored territory.
    Scout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Mission {
    pub task: Task,
    /// Where the job happens: pickup point, landing point, port...
    pub target: Option<Loc>,
    /// Transport: where its current load came aboard.
    pub origin: Option<Loc>,
    /// Turn the job started.
    pub since: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    pub alive: bool,
    pub serial: u32,
    pub owner: PlayerId,
    pub kind: UnitType,
    pub loc: Loc,
    /// Hit points remaining.
    pub hits: u8,
    /// Fighters: moves left before the tank is empty.
    pub fuel: u8,
    /// Moves remaining this turn.
    pub moves_left: u8,
    /// Done for this turn.
    pub moved: bool,
    /// Deferred with the Wait command this turn.
    pub waiting: bool,
    pub orders: Orders,
    /// A fighter on Go Direction has already been warned about half fuel.
    #[serde(default)]
    pub fuel_warned: bool,
    /// Ship name, or e.g. "5th Army".
    pub name: String,
    /// Computer opponent's job for this unit.
    #[serde(default)]
    pub mission: Mission,
}

impl Unit {
    pub fn unit_ref(&self, id: UnitId) -> UnitRef {
        UnitRef { id, serial: self.serial }
    }
}
