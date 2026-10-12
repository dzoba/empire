//! Breadth-first search on the 8-connected grid.
//!
//! Every move costs one, so a plain BFS gives exact step counts. The computer
//! opponent uses it two ways: outward from one unit to find and rank targets,
//! and outward from many cells at once to build a distance field (for
//! example, how far every square is from the nearest friendly airfield).

use crate::geom::{rotate, Dir, Geometry, Loc, ALL_DIRS};

/// How a search treats a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    /// Can't be entered.
    Block,
    /// Can be reached but not crossed: something to attack, a ship to board,
    /// a city to land in.
    Stop,
    /// Can be crossed.
    Open,
}

/// Reusable search buffers. Cells are stamped with a generation number, so
/// starting a new search never has to clear the arrays.
#[derive(Default, Debug)]
pub struct Bfs {
    stamp: Vec<u32>,
    dist: Vec<u32>,
    first: Vec<u8>,
    open: Vec<bool>,
    generation: u32,
    order: Vec<Loc>,
}

impl Clone for Bfs {
    /// Scratch space is rebuilt on demand, so a copy starts empty.
    fn clone(&self) -> Self {
        Bfs::default()
    }
}

const NO_DIR: u8 = 255;

impl Bfs {
    fn begin(&mut self, n: usize) {
        if self.stamp.len() != n {
            self.stamp = vec![0; n];
            self.dist = vec![0; n];
            self.first = vec![NO_DIR; n];
            self.open = vec![false; n];
            self.generation = 0;
        }
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.generation = 1;
        }
        self.order.clear();
    }

    /// Search outward from `sources` (distance 0) to at most `max` steps.
    /// `cell` decides how each newly met cell is treated; sources are always
    /// crossable. `spin` rotates the order neighbours are tried in, which
    /// only changes how ties between equally short routes are broken.
    pub fn run(&mut self, geom: &Geometry, sources: &[Loc], max: u32, spin: u8, mut cell: impl FnMut(Loc) -> Cell) {
        self.begin(geom.size());
        let generation = self.generation;
        for &s in sources {
            if s < geom.size() && self.stamp[s] != generation {
                self.stamp[s] = generation;
                self.dist[s] = 0;
                self.first[s] = NO_DIR;
                self.open[s] = true;
                self.order.push(s);
            }
        }
        let mut head = 0;
        while head < self.order.len() {
            let cur = self.order[head];
            head += 1;
            if !self.open[cur] {
                continue;
            }
            let d = self.dist[cur];
            if d >= max {
                continue;
            }
            for k in ALL_DIRS {
                let dir = rotate(k, spin as i32);
                let Some(nb) = geom.step(cur, dir) else { continue };
                if self.stamp[nb] == generation || geom.is_border(nb) {
                    continue;
                }
                let kind = cell(nb);
                if kind == Cell::Block {
                    continue;
                }
                self.stamp[nb] = generation;
                self.dist[nb] = d + 1;
                self.first[nb] = if self.first[cur] == NO_DIR { dir } else { self.first[cur] };
                self.open[nb] = kind == Cell::Open;
                self.order.push(nb);
            }
        }
    }

    /// Steps from the nearest source, if the search reached `loc`.
    pub fn dist(&self, loc: Loc) -> Option<u32> {
        (self.stamp.get(loc) == Some(&self.generation) && self.generation != 0).then(|| self.dist[loc])
    }

    /// The first step from the (single) source towards `loc`.
    pub fn first_step(&self, loc: Loc) -> Option<Dir> {
        self.dist(loc).and_then(|_| (self.first[loc] != NO_DIR).then_some(self.first[loc]))
    }

    /// Every reached cell with its distance, nearest first.
    pub fn reached(&self) -> impl Iterator<Item = (Loc, u32)> + '_ {
        self.order.iter().map(move |&l| (l, self.dist[l]))
    }

    /// Copy the distances into a field: `u16::MAX` where the search did not reach.
    pub fn to_field(&self, out: &mut Vec<u16>, n: usize) {
        out.clear();
        out.resize(n, u16::MAX);
        for &l in &self.order {
            out[l] = self.dist[l].min(u16::MAX as u32 - 1) as u16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_and_first_steps() {
        let g = Geometry::new(12, 8);
        let mut b = Bfs::default();
        let start = g.loc(3, 3);
        // A wall at column 5, rows 1..=5, with a gap at row 6.
        let wall = |l: Loc| g.col(l) == 5 && g.row(l) <= 5;
        b.run(&g, &[start], 100, 0, |l| if wall(l) { Cell::Block } else { Cell::Open });
        let target = g.loc(3, 7);
        assert_eq!(b.dist(target), Some(6));
        let d = b.first_step(target).unwrap();
        // The first step heads down towards the gap.
        assert!(matches!(d, 5..=7), "dir {d}");
        assert_eq!(b.dist(g.loc(3, 5)), None);
    }

    #[test]
    fn stop_cells_are_reached_but_not_crossed() {
        let g = Geometry::new(10, 5);
        let mut b = Bfs::default();
        let start = g.loc(2, 1);
        // Column 4 is a line of stop cells across the whole map.
        b.run(&g, &[start], 100, 0, |l| if g.col(l) == 4 { Cell::Stop } else { Cell::Open });
        assert_eq!(b.dist(g.loc(2, 4)), Some(3));
        assert_eq!(b.dist(g.loc(2, 6)), None);
    }

    #[test]
    fn multi_source_field() {
        let g = Geometry::new(10, 10);
        let mut b = Bfs::default();
        b.run(&g, &[g.loc(1, 1), g.loc(8, 8)], 100, 0, |_| Cell::Open);
        let mut f = Vec::new();
        b.to_field(&mut f, g.size());
        assert_eq!(f[g.loc(1, 1)], 0);
        assert_eq!(f[g.loc(4, 4)], 3);
        assert_eq!(f[g.loc(6, 6)], 2);
        assert_eq!(f[0], u16::MAX); // border
    }
}
