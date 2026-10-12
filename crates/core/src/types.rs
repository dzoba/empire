//! Unit types, terrain, and the compact per-cell "map value" encoding.

use serde::{Deserialize, Serialize};

/// Maximum number of players the engine supports. The original allowed three.
pub const MAX_PLAYERS: usize = 6;

/// Player number: 1..=MAX_PLAYERS. 0 means "nobody" (neutral).
pub type PlayerId = u8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum UnitType {
    Army = 0,
    Fighter = 1,
    Destroyer = 2,
    Transport = 3,
    Submarine = 4,
    Cruiser = 5,
    Carrier = 6,
    Battleship = 7,
}

pub const UNIT_TYPES: [UnitType; 8] = [
    UnitType::Army,
    UnitType::Fighter,
    UnitType::Destroyer,
    UnitType::Transport,
    UnitType::Submarine,
    UnitType::Cruiser,
    UnitType::Carrier,
    UnitType::Battleship,
];

impl UnitType {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<UnitType> {
        UNIT_TYPES.get(i).copied()
    }

    pub fn is_ship(self) -> bool {
        self.index() >= UnitType::Destroyer.index()
    }

    pub fn letter(self) -> char {
        b"AFDTSRCB"[self.index()] as char
    }

    pub fn from_letter(c: char) -> Option<UnitType> {
        "AFDTSRCB".find(c.to_ascii_uppercase()).and_then(UnitType::from_index)
    }

    pub fn name(self) -> &'static str {
        match self {
            UnitType::Army => "Army",
            UnitType::Fighter => "Fighter",
            UnitType::Destroyer => "Destroyer",
            UnitType::Transport => "Transport",
            UnitType::Submarine => "Submarine",
            UnitType::Cruiser => "Cruiser",
            UnitType::Carrier => "Carrier",
            UnitType::Battleship => "Battleship",
        }
    }

    pub fn long_name(self) -> &'static str {
        match self {
            UnitType::Transport => "Troop Transport",
            UnitType::Carrier => "Aircraft Carrier",
            other => other.name(),
        }
    }

    /// The cargo type a ship carries: armies for transports, fighters for carriers.
    pub fn cargo(self) -> Option<UnitType> {
        match self {
            UnitType::Transport => Some(UnitType::Army),
            UnitType::Carrier => Some(UnitType::Fighter),
            _ => None,
        }
    }
}

/// Per-type statistics. Defaults follow the Interstel v2.05 manual (Table III)
/// and its README errata (cruiser production 36/30).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitStats {
    /// Moves per turn.
    pub speed: u8,
    /// Damage points the unit can absorb.
    pub hits: u8,
    /// Damage points inflicted per successful strike.
    pub damage: u8,
    /// Multiplier on the owner's combat efficiency when attacking.
    pub attack: f64,
    /// Multiplier on the owner's combat efficiency when defending.
    pub defense: f64,
    /// Turns to build the first unit at 50% production efficiency.
    pub build_first: u16,
    /// Turns to build each following unit at 50% production efficiency.
    pub build_next: u16,
    /// Fuel range (fighters only).
    pub fuel: Option<u8>,
    /// Cargo slots per remaining hit point (transports 2, carriers 1).
    pub cargo_per_hit: u8,
    /// Can detect submarines in adjacent cells.
    pub sees_subs: bool,
    /// Can detect armies and fighters in adjacent cells.
    pub sees_land_and_air: bool,
    /// May attack armies. Ships that can do this bombard from the sea.
    pub attacks_armies: bool,
    /// Ship stays in place after destroying an army on land.
    pub bombards: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rules {
    pub units: Vec<UnitStats>,
    /// Ships move this many squares when damaged to half their hits or worse.
    pub damaged_ship_speed: u8,
    /// Hit points restored per turn for a ship docked in a friendly city.
    pub repair_per_turn: u8,
    /// Defense multiplier for an army being bombarded by a ship.
    pub bombarded_army_defense: f64,
    /// Neutral cities defend with this combat efficiency (percent).
    pub neutral_city_efficiency: f64,
    /// Fighters on Go Direction wake when this much fuel is left.
    pub fighter_wake_fuel: u8,
    /// Maximum units on the map at once (all players). 0 = scale with map size.
    pub max_units: usize,
}

impl Default for Rules {
    fn default() -> Self {
        let s = |speed, hits, damage, attack, defense, build_first, build_next| UnitStats {
            speed,
            hits,
            damage,
            attack,
            defense,
            build_first,
            build_next,
            fuel: None,
            cargo_per_hit: 0,
            sees_subs: false,
            sees_land_and_air: true,
            attacks_armies: false,
            bombards: false,
        };
        let mut army = s(1, 1, 1, 1.0, 1.0, 6, 5);
        army.attacks_armies = true;
        let mut fighter = s(5, 1, 1, 1.0, 1.0, 12, 10);
        fighter.fuel = Some(20);
        fighter.attacks_armies = true;
        let mut destroyer = s(3, 3, 1, 1.0, 1.0, 24, 20);
        destroyer.sees_subs = true;
        let mut transport = s(2, 3, 1, 0.5, 0.5, 30, 25);
        transport.cargo_per_hit = 2;
        let mut submarine = s(2, 2, 3, 1.0, 0.5, 24, 20);
        submarine.sees_subs = true;
        submarine.sees_land_and_air = false;
        let mut cruiser = s(2, 8, 2, 1.0, 1.0, 36, 30);
        cruiser.sees_subs = true;
        cruiser.attacks_armies = true;
        cruiser.bombards = true;
        let mut carrier = s(2, 8, 1, 0.5, 1.0, 48, 40);
        carrier.cargo_per_hit = 1;
        let mut battleship = s(2, 12, 3, 1.0, 1.0, 60, 50);
        battleship.attacks_armies = true;
        battleship.bombards = true;
        Rules {
            units: vec![army, fighter, destroyer, transport, submarine, cruiser, carrier, battleship],
            damaged_ship_speed: 1,
            repair_per_turn: 1,
            bombarded_army_defense: 0.5,
            neutral_city_efficiency: 50.0,
            fighter_wake_fuel: 10,
            max_units: 0,
        }
    }
}

impl Rules {
    pub fn stats(&self, t: UnitType) -> &UnitStats {
        &self.units[t.index()]
    }

    pub fn fighter_fuel(&self) -> u8 {
        self.stats(UnitType::Fighter).fuel.unwrap_or(20)
    }

    /// Turns to build a unit, scaled by production efficiency (50% is normal).
    pub fn build_time(&self, t: UnitType, first: bool, efficiency: u8) -> u32 {
        let st = self.stats(t);
        let base = if first { st.build_first } else { st.build_next } as f64;
        let eff = (efficiency.max(1)) as f64;
        ((base * 50.0 / eff).round() as u32).max(1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Terrain {
    Sea,
    Land,
    City,
}

impl Terrain {
    pub fn is_land_like(self) -> bool {
        matches!(self, Terrain::Land | Terrain::City)
    }
}

/// What a player believes occupies a cell, packed into 16 bits:
///
/// * bits 0-1: terrain as seen (0 unknown, 1 sea, 2 land, 3 city)
/// * bits 2-4: owner of the city or unit shown (0 = nobody)
/// * bits 5-8: unit type shown, plus one (0 = no unit)
///
/// A unit is shown over the terrain it stands on, so a fighter over the sea
/// reads as sea. Units inside cities are hidden by the city. Each player keeps
/// a grid of these: their fog-of-war view of the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct MapVal(pub u16);

const T_UNKNOWN: u16 = 0;
const T_SEA: u16 = 1;
const T_LAND: u16 = 2;
const T_CITY: u16 = 3;

impl MapVal {
    pub const UNKNOWN: MapVal = MapVal(T_UNKNOWN);
    pub const NEUTRAL_CITY: MapVal = MapVal(T_CITY);
    pub const SEA: MapVal = MapVal(T_SEA);
    pub const LAND: MapVal = MapVal(T_LAND);

    fn terrain_bits(self) -> u16 {
        self.0 & 3
    }

    fn kind_bits(self) -> u16 {
        (self.0 >> 5) & 15
    }

    pub fn city(owner: PlayerId) -> MapVal {
        MapVal(T_CITY | ((owner as u16 & 7) << 2))
    }

    pub fn unit(owner: PlayerId, t: UnitType, over_sea: bool) -> MapVal {
        let terrain = if over_sea { T_SEA } else { T_LAND };
        MapVal(terrain | ((owner as u16 & 7) << 2) | ((t.index() as u16 + 1) << 5))
    }

    pub fn terrain(t: Terrain) -> MapVal {
        match t {
            Terrain::Sea => MapVal::SEA,
            Terrain::Land => MapVal::LAND,
            Terrain::City => MapVal::NEUTRAL_CITY,
        }
    }

    pub fn is_unknown(self) -> bool {
        self.terrain_bits() == T_UNKNOWN
    }

    /// Owner of the city or unit shown; 0 for plain terrain, unknown cells and neutral cities.
    pub fn owner(self) -> PlayerId {
        ((self.0 >> 2) & 7) as PlayerId
    }

    pub fn is_city(self) -> bool {
        self.terrain_bits() == T_CITY
    }

    /// The unit type shown, if the value is a unit.
    pub fn unit_type(self) -> Option<UnitType> {
        match self.kind_bits() {
            0 => None,
            k => UnitType::from_index(k as usize - 1),
        }
    }

    pub fn is_unit(self) -> bool {
        self.kind_bits() != 0
    }

    /// Sea, or a ship or fighter over the sea.
    pub fn is_sea(self) -> bool {
        self.terrain_bits() == T_SEA
    }

    /// Land without a city, or a unit standing on such land.
    pub fn is_land(self) -> bool {
        self.terrain_bits() == T_LAND
    }

    pub fn is_enemy_of(self, p: PlayerId) -> bool {
        let o = self.owner();
        o != 0 && o != p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapval_roundtrip() {
        for p in 1..=MAX_PLAYERS as u8 {
            assert!(MapVal::city(p).is_city());
            assert_eq!(MapVal::city(p).owner(), p);
            for t in UNIT_TYPES {
                for sea in [false, true] {
                    if t != UnitType::Fighter && sea != t.is_ship() {
                        continue;
                    }
                    let v = MapVal::unit(p, t, sea);
                    assert_eq!(v.unit_type(), Some(t), "{v:?}");
                    assert_eq!(v.owner(), p);
                    assert_eq!(v.is_sea(), sea);
                    assert_eq!(v.is_land(), !sea);
                    assert!(!v.is_city());
                }
            }
        }
        assert!(MapVal::NEUTRAL_CITY.is_city());
        assert!(MapVal::SEA.is_sea());
        assert!(MapVal::LAND.is_land());
        assert!(!MapVal::UNKNOWN.is_sea() && !MapVal::UNKNOWN.is_land());
    }

    #[test]
    fn build_times() {
        let r = Rules::default();
        assert_eq!(r.build_time(UnitType::Fighter, false, 50), 10);
        assert_eq!(r.build_time(UnitType::Fighter, false, 100), 5);
        assert_eq!(r.build_time(UnitType::Fighter, false, 25), 20);
        assert_eq!(r.build_time(UnitType::Cruiser, true, 50), 36);
    }
}
