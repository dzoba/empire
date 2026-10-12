//! Small deterministic random number generator.
//!
//! Games are reproducible from their seed, and the generator state is part of
//! the save file, so a loaded game continues exactly as it would have.

use serde::{Deserialize, Serialize};

/// PCG-XSH-RR 32-bit output, 64-bit state.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rng {
    state: u64,
    inc: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut r = Rng { state: 0, inc: (seed.rotate_left(17) << 1) | 1 };
        r.next_u32();
        r.state = r.state.wrapping_add(seed);
        r.next_u32();
        r
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform integer in `0..n`. Returns 0 when `n == 0`.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        // Lemire's multiply-shift; the tiny bias is irrelevant for a game.
        ((self.next_u32() as u64 * n as u64) >> 32) as u32
    }

    pub fn below_usize(&mut self, n: usize) -> usize {
        self.below(n as u32) as usize
    }

    pub fn coin(&mut self) -> bool {
        self.next_u32() & 1 == 1
    }

    /// True with probability `p` (0.0..=1.0).
    pub fn chance(&mut self, p: f64) -> bool {
        (self.next_u32() as f64) < p * 4294967296.0
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below_usize(i + 1);
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn below_in_range() {
        let mut r = Rng::new(7);
        for n in 1..50 {
            for _ in 0..50 {
                assert!(r.below(n) < n);
            }
        }
    }
}
