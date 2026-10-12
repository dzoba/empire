//! # empire-core
//!
//! The rules engine for a tribute to *Empire*, the 1987 strategy game by
//! Walter Bright and Mark Baldwin (not affiliated). It has no I/O and no
//! rendering; the `empire-app` crate builds the user interface on top of it.
//!
//! Start with [`Game::new`] and a [`GameConfig`], then repeatedly call
//! [`Game::advance`] and answer the [`Pending`] decisions it returns.
//!
//! Everything a modder is likely to change lives in data: unit statistics in
//! [`Rules`], world size and density in [`MapGenParams`], players and their
//! efficiency ratings in [`GameConfig`].

pub mod ai;
pub mod city;
pub mod config;
pub mod event;
pub mod game;
pub mod geom;
pub mod geom_names;
pub mod map;
pub mod mapgen;
pub mod movement;
pub mod names;
pub mod orders;
pub mod path;
pub mod player;
pub mod rng;
pub mod save;
pub mod types;
pub mod unit;

pub use city::City;
pub use config::{GameConfig, MapSource, PlayerConfig};
pub use event::{Event, LossCause, UnitDesc};
pub use game::{Game, Observer, Pending, WakeReason};
pub use geom::{Dir, Geometry, Loc};
pub use map::WorldMap;
pub use mapgen::MapGenParams;
pub use movement::{MoveError, MoveKind, StepOutcome};
pub use player::{Control, Player, Skill};
pub use rng::Rng;
pub use types::{MapVal, PlayerId, Rules, Terrain, UnitStats, UnitType, MAX_PLAYERS, UNIT_TYPES};
pub use unit::{Orders, Unit, UnitId, UnitRef};
