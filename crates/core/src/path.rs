//! Pathfinding over a player's view of the world: A* on the 8-connected grid
//! with unit step cost and a Chebyshev-distance heuristic.

use crate::geom::{rotate, Dir, Geometry, Loc};
use crate::types::{MapVal, PlayerId, UnitType};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Which cells a unit's route may cross, judged from what its owner knows.
/// The destination is always allowed. Unexplored cells are assumed passable,
/// so routes can lead into the unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    /// Armies: land and the owner's own cities.
    Army(PlayerId),
    /// Fighters: anywhere except cities they don't own and enemy units.
    Air(PlayerId),
    /// Ships: water and the owner's own ports.
    Ship(PlayerId),
}

impl Pass {
    pub fn for_unit(t: UnitType, owner: PlayerId) -> Pass {
        match t {
            UnitType::Army => Pass::Army(owner),
            UnitType::Fighter => Pass::Air(owner),
            _ => Pass::Ship(owner),
        }
    }

    pub fn allows(self, v: MapVal) -> bool {
        if v.is_unknown() {
            return true;
        }
        match self {
            Pass::Army(p) => (v.is_land() && !v.is_enemy_of(p)) || (v.is_city() && v.owner() == p),
            Pass::Air(p) => {
                if v.is_city() {
                    v.owner() == p
                } else {
                    !v.is_enemy_of(p)
                }
            }
            Pass::Ship(p) => (v.is_sea() && !v.is_enemy_of(p)) || (v.is_city() && v.owner() == p),
        }
    }
}

/// Reusable search buffers (generation-stamped so they never need clearing).
#[derive(Default, Debug)]
pub struct PathScratch {
    stamp: Vec<u32>,
    cost: Vec<u32>,
    first: Vec<u8>,
    generation: u32,
    heap: BinaryHeap<Reverse<(u32, u32, u32)>>,
}

impl Clone for PathScratch {
    /// Scratch buffers are not worth copying; a clone starts empty.
    fn clone(&self) -> Self {
        PathScratch::default()
    }
}

impl PathScratch {
    fn begin(&mut self, n: usize) {
        if self.stamp.len() != n {
            self.stamp = vec![0; n];
            self.cost = vec![0; n];
            self.first = vec![0; n];
            self.generation = 0;
        }
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.generation = 1;
        }
        self.heap.clear();
    }
}

pub struct Search<'a> {
    pub geom: &'a Geometry,
    pub view: &'a [MapVal],
    pub pass: Pass,
}

impl Search<'_> {
    fn ok(&self, loc: Loc, end: Loc) -> bool {
        loc == end || (!self.geom.is_border(loc) && self.pass.allows(self.view[loc]))
    }

    /// First step of a shortest path from `start` to `end`, preferring the
    /// straight-line direction and then rotating by `turn` (+1 or -1) on ties.
    /// Returns None if no path exists or `start == end`.
    pub fn first_step(&self, scratch: &mut PathScratch, start: Loc, end: Loc, turn: i32) -> Option<Dir> {
        self.run(scratch, start, end, turn, usize::MAX).map(|(d, _)| d)
    }

    /// Like `first_step`, also returning the path length.
    pub fn route(&self, scratch: &mut PathScratch, start: Loc, end: Loc, turn: i32) -> Option<(Dir, usize)> {
        self.run(scratch, start, end, turn, usize::MAX)
    }

    pub fn reachable(&self, scratch: &mut PathScratch, start: Loc, end: Loc) -> bool {
        start == end || self.run(scratch, start, end, 1, usize::MAX).is_some()
    }

    /// Path length if reachable within `limit` steps.
    pub fn distance_within(&self, scratch: &mut PathScratch, start: Loc, end: Loc, limit: usize) -> Option<usize> {
        if start == end {
            return Some(0);
        }
        self.run(scratch, start, end, 1, limit).map(|(_, n)| n)
    }

    fn run(&self, s: &mut PathScratch, start: Loc, end: Loc, turn: i32, limit: usize) -> Option<(Dir, usize)> {
        if start == end {
            return None;
        }
        let g = self.geom;
        s.begin(g.size());
        let generation = s.generation;
        let turn = if turn < 0 { -1 } else { 1 };
        s.stamp[start] = generation;
        s.cost[start] = 0;
        s.first[start] = 255;
        let h0 = g.dist(start, end) as u32;
        let mut tie = 0u32;
        s.heap.push(Reverse((h0, tie, start as u32)));
        while let Some(Reverse((f, _, cur))) = s.heap.pop() {
            let cur = cur as usize;
            let cost = s.cost[cur];
            if f > cost + g.dist(cur, end) as u32 {
                continue; // stale entry
            }
            if cur == end {
                return Some((s.first[cur], cost as usize));
            }
            if cost as usize >= limit {
                continue;
            }
            let base = g.dir_towards(cur, end).unwrap_or(0);
            for k in 0..8i32 {
                // Order: straight, then alternate sides starting with `turn`.
                let off = match k {
                    0 => 0,
                    k if k % 2 == 1 => turn * ((k + 1) / 2),
                    k => -turn * (k / 2),
                };
                let d = rotate(base, off);
                let Some(nb) = g.step(cur, d) else { continue };
                if !self.ok(nb, end) {
                    continue;
                }
                let nc = cost + 1;
                if s.stamp[nb] == generation && s.cost[nb] <= nc {
                    continue;
                }
                s.stamp[nb] = generation;
                s.cost[nb] = nc;
                s.first[nb] = if cur == start { d } else { s.first[cur] };
                tie += 1;
                s.heap.push(Reverse((nc + g.dist(nb, end) as u32, tie, nb as u32)));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_from(rows: &[&str]) -> (Geometry, Vec<MapVal>) {
        let g = Geometry::new(rows[0].len(), rows.len());
        let mut v = Vec::new();
        for r in rows {
            for ch in r.chars() {
                v.push(match ch {
                    '.' => MapVal::SEA,
                    '+' => MapVal::LAND,
                    '*' => MapVal::NEUTRAL_CITY,
                    _ => MapVal::UNKNOWN,
                });
            }
        }
        (g, v)
    }

    #[test]
    fn ship_goes_around_land() {
        let (g, v) = view_from(&[
            "..........",
            "....+.....",
            "....+.....",
            "....+.....",
            "..........",
            "..........",
        ]);
        let s = Search { geom: &g, view: &v, pass: Pass::Ship(1) };
        let mut sc = PathScratch::default();
        let start = g.loc(2, 2);
        let end = g.loc(2, 7);
        let (d, n) = s.route(&mut sc, start, end, 1).unwrap();
        assert!(n >= 5);
        let next = g.step(start, d).unwrap();
        assert!(v[next].is_sea());
    }

    #[test]
    fn army_cannot_cross_sea() {
        let (g, v) = view_from(&[
            "........",
            ".++..++.",
            ".++..++.",
            "........",
            "........",
        ]);
        let s = Search { geom: &g, view: &v, pass: Pass::Army(1) };
        let mut sc = PathScratch::default();
        assert!(!s.reachable(&mut sc, g.loc(1, 1), g.loc(1, 6)));
        assert!(s.reachable(&mut sc, g.loc(1, 1), g.loc(2, 2)));
    }
}
