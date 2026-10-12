//! Map geometry: locations, directions, distances.
//!
//! A location is a cell index `row * width + col`. The outer ring of cells is
//! the inaccessible border (shaded grey in the original), so a step from any
//! playable cell never leaves the grid.

use serde::{Deserialize, Serialize};

pub type Loc = usize;

/// One of the eight compass directions, numbered counter-clockwise from east:
/// 0=E, 1=NE, 2=N, 3=NW, 4=W, 5=SW, 6=S, 7=SE (counter-clockwise from east).
/// Rotations are `(d + 1) & 7` (counter-clockwise) and `(d + 7) & 7`.
pub type Dir = u8;

pub const ALL_DIRS: [Dir; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

const DROW: [i32; 8] = [0, -1, -1, -1, 0, 1, 1, 1];
const DCOL: [i32; 8] = [1, 1, 0, -1, -1, -1, 0, 1];

pub fn dir_delta(d: Dir) -> (i32, i32) {
    (DROW[d as usize & 7], DCOL[d as usize & 7])
}

pub fn rotate(d: Dir, by: i32) -> Dir {
    ((d as i32 + by).rem_euclid(8)) as Dir
}

pub fn opposite(d: Dir) -> Dir {
    rotate(d, 4)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Geometry {
    pub width: usize,
    pub height: usize,
}

impl Geometry {
    pub fn new(width: usize, height: usize) -> Self {
        assert!(width >= 5 && height >= 5, "map must be at least 5x5");
        Geometry { width, height }
    }

    pub fn size(&self) -> usize {
        self.width * self.height
    }

    /// Number of cells units can use (everything inside the border ring).
    pub fn playable_cells(&self) -> usize {
        (self.width - 2) * (self.height - 2)
    }

    pub fn row(&self, loc: Loc) -> usize {
        loc / self.width
    }

    pub fn col(&self, loc: Loc) -> usize {
        loc % self.width
    }

    pub fn loc(&self, row: usize, col: usize) -> Loc {
        row * self.width + col
    }

    pub fn in_bounds(&self, row: i32, col: i32) -> bool {
        row >= 0 && col >= 0 && (row as usize) < self.height && (col as usize) < self.width
    }

    pub fn is_border(&self, loc: Loc) -> bool {
        let r = self.row(loc);
        let c = self.col(loc);
        r == 0 || c == 0 || r == self.height - 1 || c == self.width - 1
    }

    /// True for a cell units may occupy.
    pub fn is_playable(&self, loc: Loc) -> bool {
        loc < self.size() && !self.is_border(loc)
    }

    /// The neighbour of `loc` in direction `d`, or None if it falls off the grid.
    pub fn step(&self, loc: Loc, d: Dir) -> Option<Loc> {
        let (dr, dc) = dir_delta(d);
        let r = self.row(loc) as i32 + dr;
        let c = self.col(loc) as i32 + dc;
        if self.in_bounds(r, c) {
            Some(self.loc(r as usize, c as usize))
        } else {
            None
        }
    }

    /// Chebyshev distance: the number of moves between two cells.
    pub fn dist(&self, a: Loc, b: Loc) -> usize {
        let dr = (self.row(a) as i64 - self.row(b) as i64).unsigned_abs() as usize;
        let dc = (self.col(a) as i64 - self.col(b) as i64).unsigned_abs() as usize;
        dr.max(dc)
    }

    /// Direction of the straight-line first step from `a` towards `b`,
    /// or None if they are the same cell.
    pub fn dir_towards(&self, a: Loc, b: Loc) -> Option<Dir> {
        let (r1, c1) = (self.row(a) as i64, self.col(a) as i64);
        let (r2, c2) = (self.row(b) as i64, self.col(b) as i64);
        let dr = (r2 - r1).signum();
        let dc = (c2 - c1).signum();
        match (dr, dc) {
            (0, 0) => None,
            (0, 1) => Some(0),
            (-1, 1) => Some(1),
            (-1, 0) => Some(2),
            (-1, -1) => Some(3),
            (0, -1) => Some(4),
            (1, -1) => Some(5),
            (1, 0) => Some(6),
            (1, 1) => Some(7),
            _ => unreachable!(),
        }
    }

    /// Direction from `a` to an adjacent cell `b`.
    pub fn dir_to_adjacent(&self, a: Loc, b: Loc) -> Option<Dir> {
        if self.dist(a, b) == 1 {
            self.dir_towards(a, b)
        } else {
            None
        }
    }

    pub fn neighbors(&self, loc: Loc) -> impl Iterator<Item = (Dir, Loc)> + '_ {
        ALL_DIRS.iter().filter_map(move |&d| self.step(loc, d).map(|l| (d, l)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_and_dirs() {
        let g = Geometry::new(100, 60);
        let l = g.loc(10, 10);
        assert_eq!(g.step(l, 0), Some(g.loc(10, 11)));
        assert_eq!(g.step(l, 2), Some(g.loc(9, 10)));
        assert_eq!(g.step(l, 5), Some(g.loc(11, 9)));
        for d in ALL_DIRS {
            let n = g.step(l, d).unwrap();
            assert_eq!(g.dir_to_adjacent(l, n), Some(d));
            assert_eq!(g.dist(l, n), 1);
        }
        assert_eq!(g.dist(g.loc(0, 0), g.loc(3, 7)), 7);
        assert!(g.is_border(g.loc(0, 5)));
        assert!(g.is_border(g.loc(59, 5)));
        assert!(!g.is_border(g.loc(1, 1)));
    }
}
