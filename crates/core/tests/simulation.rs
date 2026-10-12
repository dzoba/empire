//! Whole-game smoke tests: computer players play each other.

use empire_core::*;

fn check_invariants(g: &Game) {
    let geom = g.geom();
    for (id, u) in g.units.iter().enumerate() {
        if !u.alive {
            continue;
        }
        assert!(geom.is_playable(u.loc), "unit {id} on border/out of map");
        assert!(g.units_at(u.loc).contains(&id), "index missing unit {id}");
        let t = g.map.get(u.loc);
        match u.kind {
            UnitType::Army
                if t == Terrain::Sea => {
                    assert!(g.carrier_of(id).is_some(), "army {id} at sea without transport");
                }
            UnitType::Fighter => {
                assert!(u.fuel > 0, "fighter {id} with empty tank still alive");
            }
            k if k.is_ship() => {
                assert!(t != Terrain::Land, "ship {id} on land");
                if t == Terrain::City {
                    let c = g.city_at(u.loc).unwrap();
                    assert_eq!(g.cities[c].owner, u.owner, "ship {id} in foreign city");
                }
            }
            _ => {}
        }
        if u.kind.cargo().is_some() && t == Terrain::Sea {
            assert!(g.cargo(id).len() <= g.capacity(id), "overloaded ship {id}");
        }
    }
    for loc in 0..geom.size() {
        for &id in g.units_at(loc) {
            assert!(g.units[id].alive && g.units[id].loc == loc, "stale index at {loc}");
        }
        // At most one unit per non-city square, except cargo aboard a ship.
        if g.map.get(loc) != Terrain::City {
            let here = g.units_at(loc);
            if here.len() > 1 {
                let ships: Vec<_> = here.iter().filter(|&&i| g.units[i].kind.cargo().is_some()).collect();
                assert_eq!(ships.len(), 1, "stacked units at {loc} without a carrier: {here:?}");
                let s = &g.units[*ships[0]];
                for &i in here {
                    if i != *ships[0] {
                        assert_eq!(Some(g.units[i].kind), s.kind.cargo(), "wrong cargo at {loc}");
                        assert_eq!(g.units[i].owner, s.owner);
                    }
                }
            }
        }
    }
}

fn run_ai_game(seed: u64, players: usize, max_turns: u32) -> Game {
    let config = GameConfig {
        seed,
        players: (0..players).map(|i| PlayerConfig::computer(&format!("CPU {}", i + 1), Skill::Expert)).collect(),
        ..Default::default()
    };
    let mut g = Game::new(&config).expect("new game");
    let mut steps = 0u64;
    while g.turn <= max_turns {
        match g.advance(500) {
            Pending::Busy => {}
            Pending::GameOver { .. } => break,
            other => panic!("unexpected pending in AI-only game: {other:?}"),
        }
        steps += 1;
        if steps.is_multiple_of(50) {
            check_invariants(&g);
        }
        g.drain_events();
    }
    check_invariants(&g);
    g
}

#[test]
fn computer_players_expand() {
    let g = run_ai_game(11, 2, 150);
    for p in &g.players {
        let cities = g.player_cities(p.id).count();
        assert!(cities >= 3, "player {} only has {cities} cities after 150 turns", p.id);
    }
}

#[test]
fn long_ai_games_hold_together() {
    for seed in [1u64, 2, 3] {
        let g = run_ai_game(seed, 3, 400);
        let owned: usize = g.cities.iter().filter(|c| c.owner != 0).count();
        assert!(owned > g.cities.len() / 3, "seed {seed}: only {owned} cities owned");
    }
}

#[test]
fn save_and_restore_continue_identically() {
    let config = GameConfig {
        seed: 5,
        players: vec![PlayerConfig::computer("A", Skill::Expert), PlayerConfig::computer("B", Skill::Standard)],
        ..Default::default()
    };
    let mut a = Game::new(&config).unwrap();
    while a.turn < 60 {
        a.advance(1000);
    }
    let saved = a.to_json();
    let mut b = Game::from_json(&saved).unwrap();
    for _ in 0..200 {
        a.advance(100);
        b.advance(100);
    }
    assert_eq!(a.to_json(), b.to_json());
}
