//! Default names for cities and ships.

use crate::rng::Rng;

const CITY_START: &[&str] = &[
    "Ar", "Bel", "Cor", "Dar", "El", "Fen", "Gal", "Har", "Is", "Jor", "Kel", "Lor", "Mar", "Nor", "Or", "Par",
    "Quin", "Ros", "Sel", "Tor", "Ul", "Val", "Wes", "Yar", "Zan", "Bram", "Cal", "Dun", "Ever", "Gren", "Hal",
    "Kar", "Lin", "Mor", "Nev", "Pel", "Ran", "Stan", "Tam", "Ver",
];
const CITY_END: &[&str] = &[
    "ton", "ford", "burg", "mouth", "dale", "port", "haven", "wick", "stead", "holm", "gate", "field", "bridge",
    "more", "ia", "ana", "ville", "berg", "chester", "minster", "by", "thorpe", "worth", "ness", "crest", "fall",
];

/// Generate `n` distinct city names.
pub fn city_names(n: usize, rng: &mut Rng) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(n);
    let mut seen = std::collections::HashSet::new();
    let mut tries = 0;
    while out.len() < n {
        tries += 1;
        let a = CITY_START[rng.below_usize(CITY_START.len())];
        let b = CITY_END[rng.below_usize(CITY_END.len())];
        let mut name = format!("{a}{b}");
        if tries > n * 40 {
            name = format!("{name} {}", out.len() + 1);
        }
        if seen.insert(name.clone()) {
            out.push(name);
        }
    }
    out
}

const SHIP_NAMES: &[&str] = &[
    "Defiant", "Resolute", "Vigilant", "Unyielding", "Relentless", "Audacious", "Valiant", "Dauntless", "Tenacious",
    "Stalwart", "Gallant", "Steadfast", "Invincible", "Implacable", "Formidable", "Ardent", "Glorious", "Furious",
    "Diligent", "Renown", "Sentinel", "Bastion", "Rampart", "Bulwark", "Hector", "Orion", "Perseus", "Theseus",
    "Triumph", "Venture", "Halberd", "Sea Vixen", "Salvation", "Destructor", "Claymore", "Tern", "Nemesis", "Skua",
    "Retribution", "Vengeance", "Thunderer", "Tempest", "Typhoon", "Cyclone", "Hurricane", "Monsoon", "Trident",
    "Harpoon", "Cutlass", "Sabre", "Rapier", "Lance", "Javelin", "Arrow", "Falcon", "Osprey", "Kestrel", "Puffin",
    "Petrel", "Cormorant", "Gannet", "Heron", "Kingfisher", "Shark", "Barracuda", "Marlin", "Swordfish", "Stingray",
    "Orca", "Narwhal", "Kraken", "Leviathan", "Poseidon", "Neptune", "Triton", "Nereid", "Calypso", "Circe",
    "Medusa", "Hydra", "Phoenix", "Griffin", "Wayfarer", "Lodestar", "Chimera", "Titan", "Atlas", "Meridian",
    "Goliath", "Hercules", "Spartan", "Trojan", "Viking", "Corsair", "Buccaneer", "Privateer", "Mariner", "Voyager",
    "Pathfinder", "Wanderer", "Endeavour", "Discovery", "Zephyr",
];

pub fn ship_name(index: u32) -> String {
    let n = SHIP_NAMES.len() as u32;
    let base = SHIP_NAMES[(index % n) as usize];
    if index < n {
        base.to_string()
    } else {
        format!("{base} {}", index / n + 1)
    }
}
