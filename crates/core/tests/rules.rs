//! Rules tests on small hand-made maps.

use empire_core::*;

/// Two human players on a text map, no random flips, empty board.
fn game(map: &str) -> Game {
    let config = GameConfig {
        seed: 7,
        map: MapSource::Text(map.to_string()),
        players: vec![PlayerConfig::human("A"), PlayerConfig::human("B")],
        randomize_orientation: false,
        ..Default::default()
    };
    let mut g = Game::new(&config).expect("game");
    g.clear_board();
    g.current = 1;
    g
}

const ISLANDS: &str = "\
............
.+++....+++.
.+*+....+*+.
.+++....+++.
............
............
";

fn loc(g: &Game, r: usize, c: usize) -> Loc {
    g.geom().loc(r, c)
}

#[test]
fn armies_walk_on_land_but_not_into_the_sea() {
    let mut g = game(ISLANDS);
    let a = g.place_unit(1, UnitType::Army, loc(&g, 1, 1)).unwrap();
    assert_eq!(g.step_unit(a, 0), Ok(StepOutcome::Moved)); // east to (1,2)
    assert_eq!(g.unit(a).loc, loc(&g, 1, 2));
    g.units[a].moves_left = 1;
    assert_eq!(g.classify_move(a, 2), Err(MoveError::OffMap)); // north is the border
    g.units[a].loc = loc(&g, 1, 3);
    g.rebuild_for_test();
    assert_eq!(g.classify_move(a, 0), Err(MoveError::WouldDrown));
}

#[test]
fn transports_carry_armies_and_drop_extras_when_damaged() {
    let mut g = game(ISLANDS);
    let t = g.place_unit(1, UnitType::Transport, loc(&g, 2, 4)).unwrap();
    let mut armies = Vec::new();
    for _ in 0..6 {
        let a = g.place_unit(1, UnitType::Army, loc(&g, 2, 3)).unwrap();
        // Walk aboard: (2,3) -> east is the transport at (2,4).
        assert_eq!(g.classify_move(a, 0), Ok(MoveKind::Board(t)));
        g.step_unit(a, 0).unwrap();
        armies.push(a);
    }
    assert_eq!(g.cargo(t).len(), 6);
    let extra = g.place_unit(1, UnitType::Army, loc(&g, 2, 3)).unwrap();
    assert_eq!(g.classify_move(extra, 0), Err(MoveError::TransportFull));
    // Sail east: cargo comes along.
    g.step_unit(t, 0).unwrap();
    assert!(armies.iter().all(|&a| g.unit(a).loc == loc(&g, 2, 5)));
    // Damage reduces capacity to 4; the next move loses two armies.
    g.units[t].hits = 2;
    g.units[t].moves_left = 1;
    g.step_unit(t, 0).unwrap();
    assert_eq!(g.cargo(t).len(), 4);
    assert_eq!(armies.iter().filter(|&&a| g.unit(a).alive).count(), 4);
}

#[test]
fn docking_and_repair_at_a_port() {
    let map = "\
..........
..........
...+*..+*.
..........
..........
";
    let mut g = game(map);
    let city = g.city_at(loc(&g, 2, 4)).unwrap();
    g.set_city_owner(city, 1);
    g.set_city_owner_production(city, UnitType::Army);
    let c = g.place_unit(1, UnitType::Cruiser, loc(&g, 2, 5)).unwrap();
    g.units[c].hits = 3;
    assert_eq!(g.classify_move(c, 4), Ok(MoveKind::Dock));
    g.step_unit(c, 4).unwrap();
    assert_eq!(g.unit(c).loc, loc(&g, 2, 4));
    assert_eq!(g.unit(c).moves_left, 0, "entering a city ends the move");
    // Run turns until it's player 1's turn again; each start repairs one hit.
    let before = g.unit(c).hits;
    g.end_turn_for_test();
    g.end_turn_for_test();
    assert!(g.unit(c).hits > before);
}

#[test]
fn fighters_crash_when_out_of_fuel() {
    let map = "\
..............................
.+*........................*+.
..............................
..............................
..............................
";
    let mut g = game(map);
    let f = g.place_unit(1, UnitType::Fighter, loc(&g, 2, 1)).unwrap();
    g.units[f].fuel = 3;
    for _ in 0..2 {
        g.units[f].moves_left = 1;
        assert_eq!(g.step_unit(f, 0), Ok(StepOutcome::Moved));
    }
    g.units[f].moves_left = 1;
    assert!(g.move_is_risky(f, 0));
    assert_eq!(g.step_unit(f, 0), Ok(StepOutcome::Destroyed(LossCause::OutOfFuel)));
    assert!(!g.unit(f).alive);
}

#[test]
fn fighters_refuel_in_cities() {
    let mut g = game(ISLANDS);
    let city = g.city_at(loc(&g, 2, 2)).unwrap();
    g.set_city_owner(city, 1);
    let f = g.place_unit(1, UnitType::Fighter, loc(&g, 2, 3)).unwrap();
    g.units[f].fuel = 4;
    assert_eq!(g.step_unit(f, 4), Ok(StepOutcome::Landed));
    assert_eq!(g.unit(f).fuel, 20);
}

#[test]
fn equal_destroyers_win_about_half_the_time() {
    let mut wins = 0;
    let trials = 400;
    for seed in 0..trials {
        let mut g = game(ISLANDS);
        g.rng = Rng::new(seed);
        let a = g.place_unit(1, UnitType::Destroyer, loc(&g, 4, 5)).unwrap();
        let _b = g.place_unit(2, UnitType::Destroyer, loc(&g, 4, 6)).unwrap();
        if g.step_unit(a, 0) == Ok(StepOutcome::WonBattle) {
            wins += 1;
        }
    }
    let rate = wins as f64 / trials as f64;
    assert!((0.4..0.6).contains(&rate), "win rate {rate}");
}

#[test]
fn battleships_usually_sink_transports() {
    let mut wins = 0;
    for seed in 0..200 {
        let mut g = game(ISLANDS);
        g.rng = Rng::new(seed);
        let a = g.place_unit(1, UnitType::Battleship, loc(&g, 4, 5)).unwrap();
        g.place_unit(2, UnitType::Transport, loc(&g, 4, 6)).unwrap();
        if g.step_unit(a, 0) == Ok(StepOutcome::WonBattle) {
            wins += 1;
        }
    }
    assert!(wins > 190, "battleship won {wins}/200");
}

#[test]
fn neutral_cities_fall_about_half_the_time_and_always_use_the_army() {
    let mut captured = 0;
    for seed in 0..400 {
        let mut g = game(ISLANDS);
        g.rng = Rng::new(seed);
        let a = g.place_unit(1, UnitType::Army, loc(&g, 2, 3)).unwrap();
        let out = g.step_unit(a, 4).unwrap();
        assert!(!g.unit(a).alive);
        if out == StepOutcome::CapturedCity {
            captured += 1;
            let c = g.city_at(loc(&g, 2, 2)).unwrap();
            assert_eq!(g.cities[c].owner, 1);
            assert_eq!(g.cities[c].production, None, "the new owner must choose production");
        }
    }
    assert!((150..250).contains(&captured), "captured {captured}/400");
}

#[test]
fn armies_that_attack_ships_drown_even_when_they_win() {
    for seed in 0..50 {
        let mut g = game(ISLANDS);
        g.rng = Rng::new(seed);
        let a = g.place_unit(1, UnitType::Army, loc(&g, 2, 3)).unwrap();
        g.place_unit(2, UnitType::Transport, loc(&g, 2, 4)).unwrap();
        let out = g.step_unit(a, 0).unwrap();
        assert!(!g.unit(a).alive, "seed {seed}: {out:?}");
    }
}

#[test]
fn battleships_bombard_armies_without_landing() {
    let mut g = game(ISLANDS);
    let b = g.place_unit(1, UnitType::Battleship, loc(&g, 2, 4)).unwrap();
    g.place_unit(2, UnitType::Army, loc(&g, 2, 3)).unwrap();
    assert!(matches!(g.classify_move(b, 4), Ok(MoveKind::Bombard(_))));
    g.step_unit(b, 4).unwrap();
    assert_eq!(g.unit(b).loc, loc(&g, 2, 4), "the ship stays at sea");
    let d = g.place_unit(1, UnitType::Destroyer, loc(&g, 1, 4)).unwrap();
    g.place_unit(2, UnitType::Army, loc(&g, 1, 3)).unwrap();
    assert_eq!(g.classify_move(d, 4), Err(MoveError::CannotAttack), "destroyers can't hit armies");
}

#[test]
fn submarines_are_invisible_to_battleships_but_not_destroyers() {
    let mut g = game(ISLANDS);
    let s = g.place_unit(2, UnitType::Submarine, loc(&g, 4, 5)).unwrap();
    let sub_loc = g.unit(s).loc;
    g.place_unit(1, UnitType::Battleship, loc(&g, 4, 4)).unwrap();
    assert_eq!(g.player(1).view[sub_loc], MapVal::SEA, "battleship sees only sea");
    g.place_unit(1, UnitType::Destroyer, loc(&g, 4, 6)).unwrap();
    assert_eq!(g.player(1).view[sub_loc].unit_type(), Some(UnitType::Submarine));
}

#[test]
fn production_follows_efficiency() {
    let mut g = game(ISLANDS);
    let c = g.city_at(loc(&g, 2, 2)).unwrap();
    g.set_city_owner(c, 1);
    g.cmd_set_production(c, UnitType::Army).unwrap();
    assert_eq!(g.cities[c].ready_turn, g.turn + 6);
    g.players[0].production_eff = 100;
    g.cmd_set_production(c, UnitType::Fighter).unwrap();
    assert_eq!(g.cities[c].ready_turn, g.turn + 6, "fighters: 12 turns at 50%, 6 at 100%");
    // Inland cities can't build ships.
    assert!(g.cmd_set_production(c, UnitType::Destroyer).is_err());
}

#[test]
fn losing_every_city_and_army_ends_the_game() {
    let mut g = game(ISLANDS);
    let c1 = g.city_at(loc(&g, 2, 2)).unwrap();
    let c2 = g.city_at(loc(&g, 2, 9)).unwrap();
    g.set_city_owner(c1, 1);
    g.set_city_owner(c2, 2);
    g.set_city_owner(c2, 1);
    g.check_victory_for_test();
    assert!(g.game_over);
    assert_eq!(g.winner, Some(1));
}

#[test]
fn move_to_orders_route_around_water() {
    let map = "\
............
.+++++++++*.
.++.....++..
.+++++++++*.
............
";
    let mut g = game(map);
    let a = g.place_unit(1, UnitType::Army, loc(&g, 2, 1)).unwrap();
    let target = loc(&g, 2, 9);
    // Learn the land first so the route is known.
    for l in 0..g.geom().size() {
        g.players[0].view[l] = MapVal::terrain(g.map.get(l));
    }
    g.cmd_set_orders(a, Orders::MoveTo(target)).unwrap();
    let mut guard = 0;
    while g.unit(a).loc != target && guard < 40 {
        guard += 1;
        g.units[a].moves_left = 1;
        g.units[a].moved = false;
        let _ = g.advance_unit_for_test(a);
    }
    assert_eq!(g.unit(a).loc, target, "army should walk around the lake");
}

#[test]
fn sentries_wake_when_an_enemy_comes_near() {
    let mut g = game(ISLANDS);
    let a = g.place_unit(1, UnitType::Army, loc(&g, 1, 1)).unwrap();
    g.cmd_set_orders(a, Orders::Sentry).unwrap();
    g.place_unit(2, UnitType::Army, loc(&g, 1, 2)).unwrap();
    g.observe_for_test(1, loc(&g, 1, 1));
    let woke = g.advance_unit_for_test(a);
    assert_eq!(woke, Some(WakeReason::EnemyNear));
    assert_eq!(g.unit(a).orders, Orders::None);
}

#[test]
fn larger_maps_play() {
    let config = GameConfig {
        seed: 9,
        map: MapSource::Random(MapGenParams { width: 200, height: 120, ..Default::default() }),
        players: vec![PlayerConfig::computer("A", Skill::Expert), PlayerConfig::computer("B", Skill::Expert)],
        ..Default::default()
    };
    let mut g = Game::new(&config).unwrap();
    while g.turn < 60 {
        if let Pending::GameOver { .. } = g.advance(1000) {
            break;
        }
    }
    assert!(g.player_cities(1).count() > 1 && g.player_cities(2).count() > 1);
}

#[test]
fn hot_seat_games_hand_off_between_humans() {
    let config = GameConfig {
        seed: 3,
        players: vec![PlayerConfig::human("A"), PlayerConfig::human("B")],
        ..Default::default()
    };
    let mut g = Game::new(&config).unwrap();
    assert_eq!(g.advance(100), Pending::HandOff { player: 1 });
    g.ack_handoff();
    let Pending::CityProduction { player: 1, city } = g.advance(100) else { panic!("expected production prompt") };
    g.cmd_set_production(city, UnitType::Army).unwrap();
    // Nothing to move yet: the turn passes to player 2, who must be called to the screen.
    assert_eq!(g.advance(100), Pending::HandOff { player: 2 });
    g.ack_handoff();
    assert!(matches!(g.advance(100), Pending::CityProduction { player: 2, .. }));
}

#[test]
fn saved_games_keep_human_orders() {
    let mut g = game(ISLANDS);
    let a = g.place_unit(1, UnitType::Army, loc(&g, 1, 1)).unwrap();
    g.cmd_set_orders(a, Orders::Sentry).unwrap();
    let restored = Game::from_json(&g.to_json()).unwrap();
    assert_eq!(restored.unit(a).orders, Orders::Sentry);
    assert_eq!(restored.units_at(loc(&g, 1, 1)), &[a]);
}

#[test]
fn end_turn_skips_idle_units_but_standing_orders_still_run() {
    let mut g = game(ISLANDS);
    let c1 = g.city_at(loc(&g, 2, 2)).unwrap();
    let c2 = g.city_at(loc(&g, 2, 9)).unwrap();
    g.set_city_owner(c1, 1);
    g.set_city_owner(c2, 2);
    g.set_city_owner_production(c1, UnitType::Army);
    g.set_city_owner_production(c2, UnitType::Army);
    let idle1 = g.place_unit(1, UnitType::Army, loc(&g, 1, 1)).unwrap();
    let idle2 = g.place_unit(1, UnitType::Army, loc(&g, 3, 1)).unwrap();
    let walker = g.place_unit(1, UnitType::Army, loc(&g, 1, 3)).unwrap();
    g.cmd_set_orders(walker, Orders::MoveTo(loc(&g, 3, 3))).unwrap();
    // Start player 1's turn: the first idle army asks for orders.
    loop {
        match g.advance(100) {
            Pending::HandOff { .. } => g.ack_handoff(),
            Pending::UnitOrders { player: 1, .. } => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(g.idle_units(), 2);
    assert_eq!(g.cmd_end_turn(), 2);
    assert_eq!(g.idle_units(), 0);
    // Player 1's turn finishes without asking about the idle armies; the
    // walker carries out its order on the way.
    let start = loc(&g, 1, 3);
    loop {
        match g.advance(100) {
            Pending::UnitOrders { player: 1, unit, .. } => panic!("asked about unit {unit}"),
            Pending::HandOff { player: 2 } | Pending::UnitOrders { player: 2, .. } | Pending::CityProduction { player: 2, .. } => break,
            Pending::HandOff { .. } => g.ack_handoff(),
            Pending::GameOver { .. } => panic!("game over"),
            _ => {}
        }
    }
    assert_ne!(g.unit(walker).loc, start);
    assert_eq!(g.unit(idle1).loc, loc(&g, 1, 1));
    assert_eq!(g.unit(idle2).loc, loc(&g, 3, 1));
}
