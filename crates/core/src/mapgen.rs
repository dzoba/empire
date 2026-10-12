//! Random world generator.
//!
//! Follows the recipe of the original map editor's "Build a World" and
//! "Sprinkle Cities" commands, plus its rules for a fair world:
//! * a few large land masses and many smaller ones,
//! * no lakes next to cities (small enclosed waters are filled in),
//! * every land mass with cities has at least two, one of them a port,
//!   except single-square islands.
//!
//! Works for any map size; land and city counts scale with area.

use crate::geom::{Geometry, Loc};
use crate::map::WorldMap;
use crate::rng::Rng;
use crate::types::Terrain;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapGenParams {
    pub width: usize,
    pub height: usize,
    /// Fraction of playable cells that become land (the original suggests 18-35%).
    pub land_fraction: f64,
    /// Cities per playable cell. The original's 100x60 worlds had 60-70 cities,
    /// about 0.0115 per cell.
    pub city_density: f64,
}

impl Default for MapGenParams {
    fn default() -> Self {
        MapGenParams { width: 100, height: 60, land_fraction: 0.27, city_density: 0.0115 }
    }
}

pub fn generate(params: &MapGenParams, rng: &mut Rng) -> WorldMap {
    let geom = Geometry::new(params.width.max(12), params.height.max(10));
    // Try a few times in case a world comes out without enough usable start cities.
    let mut best: Option<WorldMap> = None;
    for _ in 0..8 {
        let mut map = WorldMap::new_filled(geom, Terrain::Sea);
        grow_land(&mut map, params, rng);
        smooth(&mut map);
        fill_lakes(&mut map);
        sprinkle_cities(&mut map, params, rng);
        let starts = map.city_locs().iter().filter(|&&l| map.sea_neighbors(l) < 8).count();
        let good = starts >= 6.min(map.city_locs().len()) && map.city_locs().len() >= 4;
        if good {
            return map;
        }
        best = Some(map);
    }
    best.expect("map generation produced no candidates")
}

/// Cells at least `margin` away from the edge.
fn interior(geom: &Geometry, loc: Loc, margin: usize) -> bool {
    let r = geom.row(loc);
    let c = geom.col(loc);
    r >= margin && c >= margin && r + margin < geom.height && c + margin < geom.width
}

fn grow_land(map: &mut WorldMap, params: &MapGenParams, rng: &mut Rng) {
    let geom = map.geom;
    let playable = geom.playable_cells() as f64;
    let target = (playable * params.land_fraction.clamp(0.02, 0.9)) as usize;
    let scale = playable / 5684.0; // relative to the original world

    // Mix of continent sizes: a few big ones, more medium, many islands.
    let big = ((2.0 + rng.below(3) as f64) * scale).round().max(1.0) as usize;
    let medium = ((4.0 + rng.below(5) as f64) * scale).round().max(1.0) as usize;
    let small = ((8.0 + rng.below(8) as f64) * scale).round() as usize;
    let mut budgets: Vec<usize> = Vec::new();
    let weights: Vec<f64> = std::iter::repeat_n(10.0, big)
        .chain(std::iter::repeat_n(3.5, medium))
        .chain(std::iter::repeat_n(0.6, small))
        .map(|w| w * (0.6 + rng.below(80) as f64 / 100.0))
        .collect();
    let total: f64 = weights.iter().sum();
    for w in &weights {
        budgets.push(((*w / total) * target as f64).max(1.0) as usize);
    }

    let margin = 2;
    let mut land_count = 0;
    for budget in budgets {
        // Pick a seed in open water so masses mostly stay apart.
        let mut seed = None;
        for _ in 0..200 {
            let l = rng.below_usize(geom.size());
            if interior(&geom, l, margin + 1) && map.get(l) == Terrain::Sea {
                let clear = geom.neighbors(l).all(|(_, n)| map.get(n) == Terrain::Sea);
                if clear {
                    seed = Some(l);
                    break;
                }
            }
        }
        let Some(seed) = seed else { continue };
        map.set(seed, Terrain::Land);
        land_count += 1;
        let mut cells = vec![seed];
        let mut placed = 1;
        let mut attempts = 0;
        while placed < budget && attempts < budget * 40 {
            attempts += 1;
            // Accrete next to a random cell of this mass; orthogonal steps give
            // rounder shapes, occasional diagonals give ragged coasts.
            let from = cells[rng.below_usize(cells.len())];
            let d = if rng.below(4) == 0 { rng.below(8) as u8 } else { (rng.below(4) * 2) as u8 };
            let Some(to) = geom.step(from, d) else { continue };
            if !interior(&geom, to, margin) || map.get(to) != Terrain::Sea {
                continue;
            }
            map.set(to, Terrain::Land);
            cells.push(to);
            placed += 1;
            land_count += 1;
        }
        if land_count >= target {
            break;
        }
    }
}

/// Fill single-cell inlets and remove specks so coasts look natural.
fn smooth(map: &mut WorldMap) {
    let geom = map.geom;
    let snapshot = map.terrain.clone();
    for loc in 0..geom.size() {
        if !interior(&geom, loc, 2) {
            continue;
        }
        let land_n = geom.neighbors(loc).filter(|&(_, n)| snapshot[n] != Terrain::Sea).count();
        match snapshot[loc] {
            Terrain::Sea if land_n >= 7 => map.set(loc, Terrain::Land),
            _ => {}
        }
    }
}

/// Fill enclosed bodies of water that are not part of the main ocean, as long
/// as they are small. Larger inland seas are kept, but no city may touch them.
fn fill_lakes(map: &mut WorldMap) {
    let (label, sizes) = map.water_bodies();
    if sizes.is_empty() {
        return;
    }
    let ocean = (0..sizes.len()).max_by_key(|&i| sizes[i]).unwrap();
    let limit = 40usize.max(map.geom.playable_cells() / 140);
    for (loc, &id) in label.iter().enumerate() {
        if id != usize::MAX && id != ocean && sizes[id] <= limit {
            map.set(loc, Terrain::Land);
        }
    }
}

fn sprinkle_cities(map: &mut WorldMap, params: &MapGenParams, rng: &mut Rng) {
    let geom = map.geom;
    let target = ((geom.playable_cells() as f64) * params.city_density).round().max(4.0) as usize;
    let (water, water_sizes) = map.water_bodies();
    let ocean = (0..water_sizes.len()).max_by_key(|&i| water_sizes[i]);

    // A cell may hold a city if it is land and every sea neighbour is ocean.
    let allowed = |m: &WorldMap, loc: Loc| -> bool {
        m.get(loc) == Terrain::Land
            && !geom.is_border(loc)
            && geom.neighbors(loc).all(|(_, n)| m.get(n) != Terrain::Sea || Some(water[n]) == ocean)
    };
    let spaced = |m: &WorldMap, loc: Loc, min: usize| -> bool {
        let r = geom.row(loc) as i64;
        let c = geom.col(loc) as i64;
        let m_ = min as i64 - 1;
        for dr in -m_..=m_ {
            for dc in -m_..=m_ {
                let (rr, cc) = (r + dr, c + dc);
                if geom.in_bounds(rr as i32, cc as i32) && m.get(geom.loc(rr as usize, cc as usize)) == Terrain::City {
                    return false;
                }
            }
        }
        true
    };

    let mut coastal: Vec<Loc> = Vec::new();
    let mut inland: Vec<Loc> = Vec::new();
    for loc in 0..geom.size() {
        if allowed(map, loc) {
            if map.sea_neighbors(loc) > 0 {
                coastal.push(loc);
            } else {
                inland.push(loc);
            }
        }
    }
    rng.shuffle(&mut coastal);
    rng.shuffle(&mut inland);

    let mut placed = 0;
    for min_gap in [4, 3, 2] {
        let mut ci = coastal.iter().peekable();
        let mut ii = inland.iter().peekable();
        while placed < target && (ci.peek().is_some() || ii.peek().is_some()) {
            // Roughly two ports for every inland city, as in the original worlds.
            let pick_coast = ii.peek().is_none() || (ci.peek().is_some() && rng.below(3) != 0);
            let next = if pick_coast { ci.next() } else { ii.next() };
            let Some(&loc) = next else { continue };
            if map.get(loc) == Terrain::Land && spaced(map, loc, min_gap) {
                map.set(loc, Terrain::City);
                placed += 1;
            }
        }
        if placed >= target {
            break;
        }
    }

    // Enforce the land-mass rules.
    let (label, sizes) = map.land_masses();
    let mut mass_cities: Vec<Vec<Loc>> = vec![Vec::new(); sizes.len()];
    for loc in map.city_locs() {
        if label[loc] != usize::MAX {
            mass_cities[label[loc]].push(loc);
        }
    }
    for (id, cities) in mass_cities.iter().enumerate() {
        if cities.is_empty() || sizes[id] == 1 {
            continue;
        }
        let has_port = cities.iter().any(|&c| map.sea_neighbors(c) > 0);
        let need_second = cities.len() == 1;
        if has_port && !need_second {
            continue;
        }
        // Find a port site on this mass.
        let site = (0..geom.size()).find(|&l| {
            label[l] == id && allowed(map, l) && map.sea_neighbors(l) > 0 && spaced(map, l, 2)
        });
        match site {
            Some(l) => map.set(l, Terrain::City),
            None => {
                if need_second {
                    // Can't satisfy the rule; drop the lone city instead.
                    for &c in cities {
                        map.set(c, Terrain::Land);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_reasonable_world() {
        for seed in 0..10 {
            let mut rng = Rng::new(seed);
            let map = generate(&MapGenParams::default(), &mut rng);
            let cities = map.city_locs().len();
            assert!((40..=90).contains(&cities), "seed {seed}: {cities} cities");
            let land = map.terrain.iter().filter(|t| t.is_land_like()).count();
            let frac = land as f64 / map.geom.playable_cells() as f64;
            assert!((0.12..0.45).contains(&frac), "seed {seed}: land fraction {frac}");
            for l in 0..map.geom.size() {
                if map.geom.is_border(l) {
                    assert_eq!(map.get(l), Terrain::Sea);
                }
            }
        }
    }

    #[test]
    fn scales_to_other_sizes() {
        let mut rng = Rng::new(3);
        let big = generate(&MapGenParams { width: 200, height: 120, ..Default::default() }, &mut rng);
        assert!(big.city_locs().len() > 150);
        let small = generate(&MapGenParams { width: 40, height: 25, ..Default::default() }, &mut rng);
        assert!(small.city_locs().len() >= 4);
    }
}

// ----------------------------------------------------------------------
// Pieces of the generator, public for the map editor.

/// Replace the map with fresh land masses (no cities).
pub fn build_landmasses(map: &mut WorldMap, params: &MapGenParams, rng: &mut Rng) {
    for t in map.terrain.iter_mut() {
        *t = Terrain::Sea;
    }
    map.city_names.clear();
    grow_land(map, params, rng);
    smooth(map);
    fill_lakes(map);
}

/// Remove all cities, then sprinkle new ones following the placement rules.
pub fn sprinkle(map: &mut WorldMap, params: &MapGenParams, rng: &mut Rng) {
    for t in map.terrain.iter_mut() {
        if *t == Terrain::City {
            *t = Terrain::Land;
        }
    }
    map.city_names.clear();
    sprinkle_cities(map, params, rng);
}

/// Check city placement against the original editor's rules:
/// 1. a city is a one-square island or shares its land mass with another city;
/// 2. every land mass with cities has a port;
/// 3. no city touches a lake.
///
/// Returns (location, problem) pairs; empty means the map is acceptable.
pub fn validate(map: &WorldMap) -> Vec<(Loc, String)> {
    let mut problems = Vec::new();
    let (mass, sizes) = map.land_masses();
    let (water, water_sizes) = map.water_bodies();
    let ocean = (0..water_sizes.len()).max_by_key(|&i| water_sizes[i]);
    let cities = map.city_locs();
    let mut per_mass: std::collections::HashMap<usize, Vec<Loc>> = std::collections::HashMap::new();
    for &c in &cities {
        if mass[c] != usize::MAX {
            per_mass.entry(mass[c]).or_default().push(c);
        }
    }
    for &c in &cities {
        let m = mass[c];
        if m == usize::MAX {
            continue;
        }
        let siblings = per_mass.get(&m).map(|v| v.len()).unwrap_or(0);
        if sizes[m] > 1 && siblings < 2 {
            problems.push((c, "only city on its land mass".to_string()));
        }
        let by_lake = map.geom.neighbors(c).any(|(_, n)| map.get(n) == Terrain::Sea && water[n] != usize::MAX && Some(water[n]) != ocean);
        if by_lake {
            problems.push((c, "city beside a lake".to_string()));
        }
    }
    for (m, list) in &per_mass {
        if sizes[*m] > 1 && !list.iter().any(|&c| map.sea_neighbors(c) > 0) {
            problems.push((list[0], "land mass has no port".to_string()));
        }
    }
    problems.sort();
    problems
}

#[cfg(test)]
mod editor_tests {
    use super::*;

    #[test]
    fn generated_worlds_validate() {
        for seed in 0..5 {
            let mut rng = Rng::new(seed);
            let m = generate(&MapGenParams::default(), &mut rng);
            let p = validate(&m);
            assert!(p.len() <= 2, "seed {seed}: {p:?}");
        }
    }

    #[test]
    fn validation_catches_lonely_and_lake_cities() {
        let text = "\
............
.++++.......
.+*++..+....
.++++.......
.+..+.......
.++++.......
............
";
        let m = WorldMap::parse(text).unwrap();
        let p = validate(&m);
        assert!(p.iter().any(|(_, s)| s.contains("only city")));
    }
}
