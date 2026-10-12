//! The world map: terrain for every cell.
//!
//! Maps load from and save to a plain-text format (`.emap`): one line per row,
//! `.` sea, `+` land, `*` city, with optional `#` comment lines. Any size works;
//! the original game used 100 columns by 60 rows.

use crate::geom::{Geometry, Loc};
use crate::types::Terrain;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldMap {
    pub geom: Geometry,
    pub terrain: Vec<Terrain>,
    /// Optional city names keyed by location, e.g. from an imported map.
    #[serde(default)]
    pub city_names: Vec<(Loc, String)>,
}

impl WorldMap {
    pub fn new_filled(geom: Geometry, t: Terrain) -> Self {
        WorldMap { geom, terrain: vec![t; geom.size()], city_names: Vec::new() }
    }

    pub fn get(&self, loc: Loc) -> Terrain {
        self.terrain[loc]
    }

    pub fn set(&mut self, loc: Loc, t: Terrain) {
        self.terrain[loc] = t;
    }

    pub fn city_locs(&self) -> Vec<Loc> {
        (0..self.geom.size()).filter(|&l| self.terrain[l] == Terrain::City).collect()
    }

    /// Number of sea cells around `loc` (0..=8). A city with 8 is a one-square island;
    /// a city with at least one is a port.
    pub fn sea_neighbors(&self, loc: Loc) -> usize {
        self.geom.neighbors(loc).filter(|&(_, n)| self.terrain[n] == Terrain::Sea).count()
    }

    pub fn is_port(&self, loc: Loc) -> bool {
        self.terrain[loc] == Terrain::City && self.sea_neighbors(loc) > 0
    }

    /// Parse the `.emap` text format.
    pub fn parse(text: &str) -> Result<WorldMap, String> {
        let mut rows: Vec<Vec<Terrain>> = Vec::new();
        let mut names = Vec::new();
        let mut pending_names: Vec<(usize, usize, String)> = Vec::new();
        for (lineno, raw) in text.lines().enumerate() {
            let line = raw.trim_end_matches(['\r', '\n']);
            if let Some(rest) = line.strip_prefix('#') {
                // "# city <row> <col> <name...>" lines may carry city names.
                let mut it = rest.split_whitespace();
                if it.next() == Some("city") {
                    if let (Some(r), Some(c)) = (it.next(), it.next()) {
                        if let (Ok(r), Ok(c)) = (r.parse::<usize>(), c.parse::<usize>()) {
                            let name: Vec<&str> = it.collect();
                            if !name.is_empty() {
                                pending_names.push((r, c, name.join(" ")));
                            }
                        }
                    }
                }
                continue;
            }
            if line.trim().is_empty() {
                continue;
            }
            let mut row = Vec::with_capacity(line.len());
            for (i, ch) in line.chars().enumerate() {
                row.push(match ch {
                    '.' | '~' | ' ' => Terrain::Sea,
                    '+' | '#' => Terrain::Land,
                    '*' | 'X' | 'x' | 'O' | 'o' => Terrain::City,
                    other => {
                        return Err(format!("line {}: unexpected character {other:?} at column {i}", lineno + 1))
                    }
                });
            }
            rows.push(row);
        }
        if rows.is_empty() {
            return Err("map has no rows".into());
        }
        let width = rows[0].len();
        if rows.iter().any(|r| r.len() != width) {
            return Err("all map rows must be the same length".into());
        }
        if width < 5 || rows.len() < 5 {
            return Err("map must be at least 5x5".into());
        }
        let geom = Geometry::new(width, rows.len());
        let mut map = WorldMap { geom, terrain: rows.into_iter().flatten().collect(), city_names: Vec::new() };
        for (r, c, name) in pending_names {
            if r < geom.height && c < geom.width {
                names.push((geom.loc(r, c), name));
            }
        }
        map.city_names = names;
        map.sanitize();
        Ok(map)
    }

    /// Serialize to the `.emap` text format.
    pub fn to_text(&self) -> String {
        let mut s = String::with_capacity(self.geom.size() + self.geom.height * 2 + 64);
        s.push_str(&format!("# Empire map {}x{}\n", self.geom.width, self.geom.height));
        for (loc, name) in &self.city_names {
            s.push_str(&format!("# city {} {} {}\n", self.geom.row(*loc), self.geom.col(*loc), name));
        }
        for r in 0..self.geom.height {
            for c in 0..self.geom.width {
                s.push(match self.terrain[self.geom.loc(r, c)] {
                    Terrain::Sea => '.',
                    Terrain::Land => '+',
                    Terrain::City => '*',
                });
            }
            s.push('\n');
        }
        s
    }

    /// Enforce invariants: no cities on the inaccessible border ring.
    pub fn sanitize(&mut self) {
        for loc in 0..self.geom.size() {
            if self.geom.is_border(loc) && self.terrain[loc] == Terrain::City {
                self.terrain[loc] = Terrain::Land;
            }
        }
        let geom = self.geom;
        let terrain = &self.terrain;
        self.city_names.retain(|(l, _)| *l < geom.size() && terrain[*l] == Terrain::City);
    }

    /// Mirror top-to-bottom and left-to-right (the original's random map "flip").
    pub fn rotate_180(&mut self) {
        let n = self.geom.size();
        self.terrain.reverse();
        for (l, _) in self.city_names.iter_mut() {
            *l = n - 1 - *l;
        }
    }

    /// Mirror left-to-right (the original's "klip").
    pub fn mirror_horizontal(&mut self) {
        let g = self.geom;
        for r in 0..g.height {
            let row = &mut self.terrain[r * g.width..(r + 1) * g.width];
            row.reverse();
        }
        for (l, _) in self.city_names.iter_mut() {
            let (r, c) = (g.row(*l), g.col(*l));
            *l = g.loc(r, g.width - 1 - c);
        }
    }

    /// Label each cell with the id of its 8-connected land mass (cities count as
    /// land). Sea cells get `usize::MAX`. Returns (labels, sizes).
    pub fn land_masses(&self) -> (Vec<usize>, Vec<usize>) {
        self.components(|t| t.is_land_like())
    }

    /// Label each cell with the id of its 8-connected body of water.
    pub fn water_bodies(&self) -> (Vec<usize>, Vec<usize>) {
        self.components(|t| t == Terrain::Sea)
    }

    fn components(&self, pred: impl Fn(Terrain) -> bool) -> (Vec<usize>, Vec<usize>) {
        let n = self.geom.size();
        let mut label = vec![usize::MAX; n];
        let mut sizes = Vec::new();
        let mut stack = Vec::new();
        for start in 0..n {
            if label[start] != usize::MAX || !pred(self.terrain[start]) || self.geom.is_border(start) {
                continue;
            }
            let id = sizes.len();
            let mut size = 0;
            label[start] = id;
            stack.push(start);
            while let Some(l) = stack.pop() {
                size += 1;
                for (_, nb) in self.geom.neighbors(l) {
                    if label[nb] == usize::MAX && !self.geom.is_border(nb) && pred(self.terrain[nb]) {
                        label[nb] = id;
                        stack.push(nb);
                    }
                }
            }
            sizes.push(size);
        }
        (label, sizes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roundtrip() {
        let text = ".......\n..+++..\n..+*+..\n..+++..\n.......\n";
        let m = WorldMap::parse(text).unwrap();
        assert_eq!(m.geom.width, 7);
        assert_eq!(m.geom.height, 5);
        assert_eq!(m.city_locs(), vec![m.geom.loc(2, 3)]);
        let again = WorldMap::parse(&m.to_text()).unwrap();
        assert_eq!(again.terrain, m.terrain);
    }

    #[test]
    fn border_cities_removed() {
        let text = "*......\n.......\n...*...\n.......\n.......\n";
        let m = WorldMap::parse(text).unwrap();
        assert_eq!(m.city_locs().len(), 1);
    }

    #[test]
    fn components() {
        let text = ".......\n.++..+.\n.++..+.\n.......\n.......\n";
        let m = WorldMap::parse(text).unwrap();
        let (_, sizes) = m.land_masses();
        let mut s = sizes.clone();
        s.sort();
        assert_eq!(s, vec![2, 4]);
    }
}
