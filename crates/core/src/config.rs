//! Settings for a new game.

use crate::mapgen::MapGenParams;
use crate::player::{Control, Skill};
use crate::types::Rules;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MapSource {
    /// Generate a new world.
    Random(MapGenParams),
    /// A map in the `.emap` text format.
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerConfig {
    pub name: String,
    pub control: Control,
    pub production_eff: u8,
    pub combat_eff: u8,
    /// Hidden efficiency multiplier in percent (100 = none).
    #[serde(default = "full")]
    pub handicap: u8,
}

fn full() -> u8 {
    100
}

impl PlayerConfig {
    pub fn human(name: &str) -> Self {
        PlayerConfig { name: name.into(), control: Control::Human, production_eff: 50, combat_eff: 50, handicap: 100 }
    }

    pub fn computer(name: &str, skill: Skill) -> Self {
        // The original's "Standard" computer shows normal efficiencies but plays
        // with a handicap; "Expert" plays on equal terms with a human.
        let handicap = match skill {
            Skill::Standard => 90,
            Skill::Expert => 100,
        };
        PlayerConfig { name: name.into(), control: Control::Computer(skill), production_eff: 50, combat_eff: 50, handicap }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameConfig {
    pub seed: u64,
    pub map: MapSource,
    pub players: Vec<PlayerConfig>,
    pub rules: Rules,
    /// Randomly flip and mirror a loaded map, as the original did.
    pub randomize_orientation: bool,
}

impl Default for GameConfig {
    fn default() -> Self {
        GameConfig {
            seed: 1,
            map: MapSource::Random(MapGenParams::default()),
            players: vec![PlayerConfig::human("Player 1"), PlayerConfig::computer("Computer", Skill::Expert)],
            rules: Rules::default(),
            randomize_orientation: true,
        }
    }
}
