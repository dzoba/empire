//! Exact combat odds.
//!
//! A battle is a series of rounds. In each round the attacker lands a blow
//! with probability `a / (a + d)`, where `a` and `d` are the two sides'
//! combat efficiencies times their attack and defense factors; otherwise the
//! defender does. Each blow removes the striker's damage from the other side's
//! hits, and the fight ends when one side reaches zero. The attacker wins if
//! it lands enough blows before the defender does, which is a negative
//! binomial sum.

use crate::game::Game;
use crate::types::{PlayerId, UnitType};
use crate::unit::UnitId;

/// Probability of landing `a_needs` successes before `d_needs` failures, with
/// success probability `p` per trial.
pub fn duel(p: f64, a_needs: u32, d_needs: u32) -> f64 {
    if a_needs == 0 {
        return 1.0;
    }
    if d_needs == 0 {
        return 0.0;
    }
    let p = p.clamp(0.0, 1.0);
    let q = 1.0 - p;
    // Sum over k defender blows (k < d_needs) landed before the last attacker blow:
    // C(a_needs - 1 + k, k) p^a_needs q^k
    let pa = p.powi(a_needs as i32);
    let mut term = pa; // k = 0
    let mut total = term;
    for k in 1..d_needs {
        term *= q * (a_needs - 1 + k) as f64 / k as f64;
        total += term;
    }
    total.clamp(0.0, 1.0)
}

fn blows(hits: u32, damage: u32) -> u32 {
    hits.div_ceil(damage.max(1))
}

impl Game {
    /// Chance that unit `att` destroys an enemy of type `kind` owned by
    /// `owner`. The enemy is assumed to be at full strength, since that is
    /// all its opponent can see. `bombard` is a ship firing on an army ashore.
    pub fn win_chance(&self, att: UnitId, kind: UnitType, owner: PlayerId, bombard: bool) -> f64 {
        let u = &self.units[att];
        let ast = self.rules.stats(u.kind);
        let dst = self.rules.stats(kind);
        let a = self.player(u.owner).effective_combat() * ast.attack;
        let mut d = self.player(owner).effective_combat() * dst.defense;
        if bombard && kind == UnitType::Army {
            d *= self.rules.bombarded_army_defense;
        }
        let p = if a + d > 0.0 { a / (a + d) } else { 0.5 };
        duel(p, blows(dst.hits as u32, ast.damage as u32), blows(u.hits as u32, dst.damage as u32))
    }

    /// Chance that one army takes a city held by `defender` (0 = neutral).
    pub fn capture_chance(&self, attacker: PlayerId, defender: PlayerId) -> f64 {
        let a = self.player(attacker).effective_combat() * self.rules.stats(UnitType::Army).attack;
        let d = if defender == 0 { self.rules.neutral_city_efficiency } else { self.player(defender).effective_combat() };
        if a + d > 0.0 {
            a / (a + d)
        } else {
            0.5
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn simple_cases() {
        assert!(close(duel(0.5, 1, 1), 0.5));
        assert!(close(duel(0.3, 3, 1), 0.3f64.powi(3)));
        assert!(close(duel(0.3, 1, 3), 1.0 - 0.7f64.powi(3)));
    }

    #[test]
    fn sides_sum_to_one() {
        for &p in &[0.1, 0.33, 0.5, 0.8] {
            for a in 1..6 {
                for d in 1..6 {
                    let s = duel(p, a, d) + duel(1.0 - p, d, a);
                    assert!(close(s, 1.0), "p={p} a={a} d={d} sum={s}");
                }
            }
        }
    }

    #[test]
    fn matches_simulation() {
        // Two blows needed against three, p = 0.6, checked by enumeration.
        let mut rng = crate::rng::Rng::new(7);
        let trials = 200_000;
        let mut wins = 0;
        for _ in 0..trials {
            let (mut a, mut d) = (0, 0);
            loop {
                if rng.chance(0.6) {
                    a += 1;
                    if a == 2 {
                        wins += 1;
                        break;
                    }
                } else {
                    d += 1;
                    if d == 3 {
                        break;
                    }
                }
            }
        }
        let est = wins as f64 / trials as f64;
        assert!((est - duel(0.6, 2, 3)).abs() < 0.005, "{est} vs {}", duel(0.6, 2, 3));
    }
}
