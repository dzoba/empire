//! Headless tools: run computer-vs-computer games, dump maps, render screens.
//!
//! ```text
//! empire-cli sim [--seed N] [--players K] [--turns T] [--size WxH] [--every N]
//! empire-cli map [--seed N] [--size WxH]          print a generated world as text
//! empire-cli arena [--games N] [--turns T] [--size WxH] [--a expert] [--b standard] [--threads N]
//!                                                  play one computer style against another
//! empire-cli duel [--seed N] [--turns T] [--a expert] [--b standard] [--every N]
//!                                                  one game, with progress reports
//! ```

use empire_core::*;
use std::collections::HashMap;

/// Device pixel ratio used for screenshots (2 = a typical high-density screen).
const DPR: f64 = 2.0;

fn arg<T: std::str::FromStr>(args: &HashMap<String, String>, key: &str, default: T) -> T {
    args.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn parse_size(s: &str) -> Option<(usize, usize)> {
    let (w, h) = s.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let cmd = argv.get(1).cloned().unwrap_or_else(|| "help".into());
    let mut args = HashMap::new();
    let mut i = 2;
    while i < argv.len() {
        if let Some(k) = argv[i].strip_prefix("--") {
            let v = argv.get(i + 1).cloned().unwrap_or_default();
            args.insert(k.to_string(), v);
            i += 2;
        } else {
            i += 1;
        }
    }
    let seed: u64 = arg(&args, "seed", 1);
    let (w, h) = args.get("size").and_then(|s| parse_size(s)).unwrap_or((100, 60));
    let params = MapGenParams { width: w, height: h, ..Default::default() };
    match cmd.as_str() {
        "sim" => sim(seed, arg(&args, "players", 2), arg(&args, "turns", 500), params, arg(&args, "every", 50)),
        "arena" => arena(
            seed,
            arg(&args, "games", 40),
            arg(&args, "turns", 400),
            params,
            args.get("a").cloned().unwrap_or_else(|| "expert".into()),
            args.get("b").cloned().unwrap_or_else(|| "standard".into()),
            args.get("threads").and_then(|t| t.parse().ok()),
        ),
        "savesize" => savesize(seed, arg(&args, "turns", 200), params),
        "duel" => duel(
            seed,
            arg(&args, "turns", 400),
            params,
            args.get("a").cloned().unwrap_or_else(|| "expert".into()),
            args.get("b").cloned().unwrap_or_else(|| "standard".into()),
            arg(&args, "every", 25),
        ),
        "scenarios" => scenarios(args.get("out").cloned().unwrap_or_else(|| "target/scenarios".into()), args.get("assets").cloned()),
        "readme-shots" => readme_shots(args.get("out").cloned().unwrap_or_else(|| "docs/images".into())),
        "shots" => shots(args.get("out").cloned().unwrap_or_else(|| "target/shots".into()), seed, args.get("assets").cloned()),
        "map" => {
            let mut rng = Rng::new(seed);
            let m = empire_core::mapgen::generate(&params, &mut rng);
            print!("{}", m.to_text());
            eprintln!("{} cities", m.city_locs().len());
        }
        _ => {
            eprintln!("usage: empire-cli sim|map|shots|arena [--seed N] [--players K] [--turns T] [--size WxH] [--every N] [--out DIR] [--assets DIR] [--games N] [--a SKILL] [--b SKILL]");
        }
    }
}

fn parse_skill(s: &str) -> Skill {
    match s {
        "standard" => Skill::Standard,
        _ => Skill::Expert,
    }
}

/// Adjust a computer player after the game is created (a hook for
/// experimental opponents; the built-in styles need nothing).
fn prepare_side(_g: &mut Game, _id: PlayerId, _side: &str) {}

/// Play one game and print both sides' progress every `every` turns.
fn duel(seed: u64, turns: u32, params: MapGenParams, a: String, b: String, every: u32) {
    let pa = PlayerConfig::computer("A", parse_skill(&a));
    let pb = PlayerConfig::computer("B", parse_skill(&b));
    let config = GameConfig { seed, map: MapSource::Random(params), players: vec![pa, pb], ..Default::default() };
    let mut g = Game::new(&config).expect("new game");
    prepare_side(&mut g, 1, &a);
    prepare_side(&mut g, 2, &b);
    let mut last = 0;
    // Per player: cities taken from the enemy, from neutrals; assaults repelled;
    // attacks won and lost; armies lost with their ship.
    let mut tally = vec![[0u32; 6]; g.players.len() + 1];
    // Per player: which enemy unit types sank its transports.
    let mut sunk_by = vec![[0u32; 8]; g.players.len() + 1];
    loop {
        let pending = g.advance(1);
        for e in g.drain_events() {
            match e {
                Event::CityCaptured { by, from, .. } => tally[by as usize][if from == 0 { 1 } else { 0 }] += 1,
                Event::CityRepelled { attacker, .. } => tally[attacker as usize][2] += 1,
                Event::Combat { attacker, defender, attacker_won, .. } => {
                    tally[attacker.owner as usize][if attacker_won { 3 } else { 4 }] += 1;
                    if attacker_won && defender.kind == UnitType::Transport {
                        sunk_by[defender.owner as usize][attacker.kind.index()] += 1;
                    }
                }
                Event::CargoLost { owner, count, .. } => tally[owner as usize][5] += count,
                _ => {}
            }
        }
        if g.turn >= last + every {
            last = g.turn;
            println!("{}", summary(&g));
            for p in &g.players {
                let t = tally[p.id as usize];
                println!(
                    "      P{}: built {:?} lost {:?}; took {} enemy + {} neutral cities, {} assaults failed, attacks won {} lost {}, cargo lost {}",
                    p.id, p.stats.built, p.stats.lost, t[0], t[1], t[2], t[3], t[4], t[5]
                );
                println!("          transports sunk by {:?}", sunk_by[p.id as usize]);
            }
        }
        if let Pending::GameOver { winner } = pending {
            println!("game over at turn {}: winner {:?}", g.turn, winner);
            break;
        }
        if g.turn > turns {
            break;
        }
    }
    println!("{}", summary(&g));
}

/// What happened to one side in an arena game, up to the mid-game snapshot.
#[derive(Clone, Default)]
struct SideStats {
    built: [u32; 8],
    lost: [u32; 8],
    drowned: u32,
    took_enemy: u32,
    took_neutral: u32,
    repelled: u32,
    /// Our transports sunk, by the attacker's unit type.
    sunk_by: [u32; 8],
}

impl SideStats {
    fn add(&mut self, o: &SideStats) {
        for i in 0..8 {
            self.built[i] += o.built[i];
            self.lost[i] += o.lost[i];
            self.sunk_by[i] += o.sunk_by[i];
        }
        self.drowned += o.drowned;
        self.took_enemy += o.took_enemy;
        self.took_neutral += o.took_neutral;
        self.repelled += o.repelled;
    }
}

/// Turn at which arena games take their mid-game snapshot.
const SNAPSHOT_TURN: u32 = 150;

struct Outcome {
    /// A's score: 1 win, 0.5 draw, 0 loss.
    score: f64,
    turn: u32,
    cities: (usize, usize),
    at100: (usize, usize),
    stats: [SideStats; 2],
}

/// One arena game: side A against side B. `swap` seats B as player 1.
fn arena_game(seed: u64, a: &str, b: &str, swap: bool, turns: u32, params: MapGenParams) -> Outcome {
    let pa = PlayerConfig::computer("A", parse_skill(a));
    let pb = PlayerConfig::computer("B", parse_skill(b));
    let players = if swap { vec![pb, pa] } else { vec![pa, pb] };
    let config = GameConfig { seed, map: MapSource::Random(params), players, ..Default::default() };
    let mut g = Game::new(&config).expect("new game");
    let (ida, idb) = if swap { (2, 1) } else { (1, 2) };
    prepare_side(&mut g, ida, a);
    prepare_side(&mut g, idb, b);
    let side = |p: PlayerId| if p == ida { 0 } else { 1 };
    let mut at100 = (0, 0);
    let mut stats = [SideStats::default(), SideStats::default()];
    let mut snapped = false;
    let winner = loop {
        if g.turn >= 100 && at100 == (0, 0) {
            at100 = (g.player_cities(ida).count(), g.player_cities(idb).count());
        }
        if g.turn >= SNAPSHOT_TURN && !snapped {
            snapped = true;
            for (i, id) in [ida, idb].into_iter().enumerate() {
                stats[i].built = g.player(id).stats.built;
                stats[i].lost = g.player(id).stats.lost;
            }
        }
        match g.advance(200) {
            Pending::GameOver { winner } => break winner,
            Pending::SurrenderOffer { .. } => g.answer_surrender(true),
            _ => {}
        }
        for e in g.drain_events() {
            if snapped {
                continue;
            }
            match e {
                Event::CityCaptured { by, from, .. } => {
                    if from == 0 {
                        stats[side(by)].took_neutral += 1
                    } else {
                        stats[side(by)].took_enemy += 1
                    }
                }
                Event::CityRepelled { attacker, .. } => stats[side(attacker)].repelled += 1,
                Event::CargoLost { owner, count, kind: UnitType::Army, .. } => stats[side(owner)].drowned += count,
                Event::Combat { attacker, defender, attacker_won: true, .. } if defender.kind == UnitType::Transport => {
                    stats[side(defender.owner)].sunk_by[attacker.kind.index()] += 1
                }
                _ => {}
            }
        }
        if g.turn > turns {
            break None;
        }
    };
    let ca = g.player_cities(ida).count();
    let cb = g.player_cities(idb).count();
    let score = match winner {
        Some(w) if w == ida => 1.0,
        Some(_) => 0.0,
        None if ca > cb => 1.0,
        None if cb > ca => 0.0,
        None => 0.5,
    };
    Outcome { score, turn: g.turn, cities: (ca, cb), at100, stats }
}

fn arena(seed: u64, games: u64, turns: u32, params: MapGenParams, a: String, b: String, threads: Option<usize>) {
    let start = std::time::Instant::now();
    let threads = threads.unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)) as u64;
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for t in 0..threads {
            let results = &results;
            let params = params.clone();
            let (a, b) = (a.clone(), b.clone());
            scope.spawn(move || {
                let mut i = t;
                while i < games {
                    // Each map is played twice, once from each start.
                    let r = arena_game(seed + i / 2, &a, &b, i % 2 == 1, turns, params.clone());
                    println!(
                        "game {:>3} seed {:>4} {}: A {} ({} cities) vs B ({} cities) at turn {}; at turn 100 {}-{}",
                        i,
                        seed + i / 2,
                        if i % 2 == 1 { "B moves first" } else { "A moves first" },
                        if r.score >= 1.0 {
                            "wins  "
                        } else if r.score <= 0.0 {
                            "loses "
                        } else {
                            "draws "
                        },
                        r.cities.0,
                        r.cities.1,
                        r.turn,
                        r.at100.0,
                        r.at100.1
                    );
                    results.lock().unwrap().push(r);
                    i += threads;
                }
            });
        }
    });
    let results = results.into_inner().unwrap();
    let n = results.len().max(1);
    let score: f64 = results.iter().map(|r| r.score).sum();
    let wins = results.iter().filter(|r| r.score == 1.0).count();
    let draws = results.iter().filter(|r| r.score == 0.5).count();
    let (a100, b100) = results.iter().fold((0, 0), |(x, y), r| (x + r.at100.0, y + r.at100.1));
    println!("cities at turn 100: A {a100}, B {b100}");
    let mut totals = [SideStats::default(), SideStats::default()];
    for r in &results {
        totals[0].add(&r.stats[0]);
        totals[1].add(&r.stats[1]);
    }
    for (name, t) in ["A", "B"].iter().zip(&totals) {
        println!(
            "{name} by turn {SNAPSHOT_TURN}: built {:?} lost {:?}; took {} enemy + {} neutral cities, {} assaults failed, {} armies drowned, transports sunk by {:?}",
            t.built, t.lost, t.took_enemy, t.took_neutral, t.repelled, t.drowned, t.sunk_by
        );
    }
    println!(
        "A ({a}) vs B ({b}) on {}x{}, {} games: A won {wins}, drew {draws}, lost {}; score {:.1}%  ({:.1}s)",
        params.width,
        params.height,
        results.len(),
        results.len() - wins - draws,
        100.0 * score / n as f64,
        start.elapsed().as_secs_f64()
    );
}

/// Play a computer game to a turn and report how big its save is.
fn savesize(seed: u64, turns: u32, params: MapGenParams) {
    let config = GameConfig {
        seed,
        map: MapSource::Random(params),
        players: vec![PlayerConfig::human("You"), PlayerConfig::computer("CPU", Skill::Expert)],
        ..Default::default()
    };
    let mut g = Game::new(&config).expect("new game");
    // Let the computer play both sides, then hand the game back to the human
    // at the start of a turn, the way a real save happens.
    g.players[0].control = Control::Computer(Skill::Expert);
    while g.turn < turns {
        g.advance(1000);
        g.drain_events();
    }
    g.players[0].control = Control::Human;
    while !matches!(g.advance(1000), Pending::CityProduction { .. } | Pending::UnitOrders { .. } | Pending::GameOver { .. }) {}
    let json = g.to_json();
    let save = g.to_save();
    let back = Game::from_save(&save).expect("save loads");
    assert_eq!(back.to_json(), json, "save round trip");
    println!(
        "{}x{} turn {}: JSON {} bytes, save {} bytes ({:.1}x smaller)",
        g.geom().width,
        g.geom().height,
        g.turn,
        json.len(),
        save.len(),
        json.len() as f64 / save.len() as f64
    );
}

fn summary(g: &Game) -> String {
    let mut s = format!("turn {:4}:", g.turn);
    for p in &g.players {
        let cities = g.player_cities(p.id).count();
        let mut counts = [0u32; 8];
        for id in g.player_units(p.id) {
            counts[g.units[id].kind.index()] += 1;
        }
        let ships: u32 = counts[2..].iter().sum();
        let kills: u32 = p.stats.destroyed.iter().sum();
        s += &format!(
            "  P{} {:>3}c {:>3}A {:>2}F {:>2}T {:>2}S{}",
            p.id,
            cities,
            counts[0],
            counts[1],
            counts[3],
            ships - counts[3],
            if p.defeated { " (out)".to_string() } else { format!(" k{kills}") }
        );
    }
    let neutral = g.cities.iter().filter(|c| c.owner == 0).count();
    s += &format!("  neutral {neutral}");
    s
}

fn sim(seed: u64, players: usize, turns: u32, params: MapGenParams, every: u32) {
    let config = GameConfig {
        seed,
        map: MapSource::Random(params),
        players: (0..players).map(|i| PlayerConfig::computer(&format!("CPU {}", i + 1), Skill::Expert)).collect(),
        ..Default::default()
    };
    let mut g = Game::new(&config).expect("new game");
    println!("{} cities on a {}x{} map", g.cities.len(), g.geom().width, g.geom().height);
    let start = std::time::Instant::now();
    let mut last_report = 0;
    let mut combats = 0u32;
    let mut captures = 0u32;
    loop {
        let pending = g.advance(1000);
        for e in g.drain_events() {
            match e {
                Event::Combat { .. } => combats += 1,
                Event::CityCaptured { from, .. } if from != 0 => captures += 1,
                Event::PlayerDefeated { player } => println!("turn {}: player {player} defeated", g.turn),
                _ => {}
            }
        }
        if g.turn >= last_report + every {
            last_report = g.turn;
            println!("{}  battles {combats} captures-from-enemy {captures}", summary(&g));
        }
        if let Pending::GameOver { winner } = pending {
            println!("game over at turn {}: winner {:?}", g.turn, winner);
            break;
        }
        if g.turn > turns {
            println!("stopped at turn {}", g.turn);
            break;
        }
    }
    println!("{}", summary(&g));
    println!("elapsed {:.2}s", start.elapsed().as_secs_f64());
}

/// Drive the user interface through a short scripted session and write a
/// PPM screenshot after each step (convert with any image tool).
fn shots(out: String, seed: u64, assets: Option<String>) {
    use empire_app::{App, Key, KeyEvent};
    std::fs::create_dir_all(&out).expect("create output dir");
    let mut app = App::new();
    app.set_viewport(960, 600, DPR);
    if let Some(dir) = assets {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if name.ends_with(".tiles") || name.ends_with(".font") {
                if let Ok(t) = std::fs::read_to_string(&path) {
                    if let Err(e) = app.load_asset_text(&name, &t) {
                        eprintln!("{name}: {e}");
                    }
                }
            } else if name.ends_with(".img") {
                if let Ok(b) = std::fs::read(&path) {
                    let _ = app.load_asset_bytes(&name, &b);
                }
            }
        }
        let maps = std::path::Path::new(&dir).join("maps");
        for entry in std::fs::read_dir(maps).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if let Ok(t) = std::fs::read_to_string(&path) {
                let _ = app.load_asset_text(&name, &t);
            }
        }
    }
    let mut t = 1000.0 + seed as f64;
    let mut n = 0;
    let mut snap = |app: &mut App, label: &str| {
        n += 1;
        app.draw();
        let path = format!("{out}/{n:02}_{label}.ppm");
        std::fs::write(&path, app.frame.to_ppm()).expect("write ppm");
        println!("{path}");
    };
    let mut run = |app: &mut App, ms: f64| {
        let end = t + ms;
        while t < end {
            t += 16.0;
            app.tick(t);
        }
    };
    let key = |app: &mut App, k: Key| {
        app.key_down(KeyEvent::plain(k));
    };
    run(&mut app, 100.0);
    snap(&mut app, "title");
    key(&mut app, Key::Space);
    run(&mut app, 100.0);
    snap(&mut app, "desktop");
    app.key_down(KeyEvent { key: Key::Char('n'), ctrl: true, alt: false, shift: false });
    run(&mut app, 50.0);
    snap(&mut app, "opponents");
    key(&mut app, Key::Enter);
    run(&mut app, 50.0);
    snap(&mut app, "options");
    key(&mut app, Key::Enter);
    run(&mut app, 50.0);
    snap(&mut app, "worldmap_choice");
    key(&mut app, Key::Char('r'));
    run(&mut app, 50.0);
    snap(&mut app, "name");
    for c in "TESTER".chars() {
        key(&mut app, Key::Char(c));
    }
    key(&mut app, Key::Enter);
    run(&mut app, 50.0);
    snap(&mut app, "report");
    key(&mut app, Key::Enter);
    run(&mut app, 300.0);
    snap(&mut app, "production");
    key(&mut app, Key::Char('f'));
    key(&mut app, Key::Enter);
    run(&mut app, 3000.0);
    snap(&mut app, "playing");
    // Play several turns: skip every unit, answer production popups with OK.
    for i in 0..40 {
        key(&mut app, Key::Space);
        key(&mut app, Key::Enter);
        run(&mut app, 400.0);
        if i == 20 {
            snap(&mut app, "turns");
        }
    }
    snap(&mut app, "later");
    key(&mut app, Key::F(5));
    run(&mut app, 50.0);
    snap(&mut app, "war_report");
    key(&mut app, Key::Enter);
    key(&mut app, Key::F(7));
    run(&mut app, 50.0);
    snap(&mut app, "world_map");
    key(&mut app, Key::Enter);
    key(&mut app, Key::Char('v'));
    run(&mut app, 50.0);
    snap(&mut app, "survey");
    app.mouse_down(30, 4, 0);
    run(&mut app, 50.0);
    snap(&mut app, "menu_reports");
    app.key_down(KeyEvent::plain(Key::Esc));

    // Map editor: exit the game, open the editor, build a world.
    app.key_down(KeyEvent { key: Key::Char('g'), ctrl: true, alt: false, shift: false });
    key(&mut app, Key::Char('y'));
    run(&mut app, 50.0);
    app.key_down(KeyEvent { key: Key::Char('e'), ctrl: true, alt: false, shift: false });
    run(&mut app, 50.0);
    snap(&mut app, "editor_blank");
    key(&mut app, Key::Char('w'));
    run(&mut app, 50.0);
    snap(&mut app, "editor_sprinkle_prompt");
    key(&mut app, Key::Char('y'));
    run(&mut app, 600.0);
    snap(&mut app, "editor_world");
    key(&mut app, Key::Char('v'));
    run(&mut app, 50.0);
    snap(&mut app, "editor_validate");
    key(&mut app, Key::Enter);
    app.key_down(KeyEvent { key: Key::Char('w'), ctrl: false, alt: true, shift: false });
    run(&mut app, 50.0);
    snap(&mut app, "editor_world_view");
    key(&mut app, Key::Enter);
    key(&mut app, Key::Char('g'));
    run(&mut app, 1500.0);
    key(&mut app, Key::Space);
    snap(&mut app, "editor_grown");
    app.mouse_down(70, 4, 0);
    run(&mut app, 50.0);
    snap(&mut app, "editor_menu");
}

/// Screenshots for the README, using only the built-in (original) artwork.
fn readme_shots(out: String) {
    use empire_app::{App, Key, KeyEvent};
    std::fs::create_dir_all(&out).expect("create output dir");
    let write = |app: &mut App, name: &str| {
        app.draw();
        let path = format!("{out}/{name}.ppm");
        std::fs::write(&path, app.frame.to_ppm()).expect("write");
        println!("{path}");
    };
    let mut app = App::new();
    app.set_viewport(960, 600, DPR);
    app.tick(1000.0);
    write(&mut app, "title");

    // A game well under way: player 1 human, played by the computer until turn 140.
    let config = GameConfig {
        seed: 21,
        players: vec![PlayerConfig::computer("You", Skill::Expert), PlayerConfig::computer("Enemy", Skill::Expert)],
        ..Default::default()
    };
    let mut g = Game::new(&config).expect("game");
    while g.turn < 140 {
        if let Pending::GameOver { .. } = g.advance(1000) {
            break;
        }
    }
    g.players[0].control = Control::Human;
    g.players[0].name = "TESTER".into();
    let mut app = App::new();
    app.set_viewport(960, 600, DPR);
    app.key_down(KeyEvent::plain(Key::Space));
    app.settings.report_armies_fighters = false;
    app.start_game(g);
    let mut t = 2000.0;
    for _ in 0..200 {
        t += 16.0;
        app.tick(t);
    }
    // Accept any production and ship-name prompts, and let queued battle
    // messages finish, so the screenshot shows the map.
    for _ in 0..20 {
        app.key_down(KeyEvent::plain(Key::Enter));
        for _ in 0..100 {
            t += 16.0;
            app.tick(t);
        }
    }
    write(&mut app, "playing");
    app.set_hd(false);
    write(&mut app, "playing_cga");
    app.set_hd(true);
    app.key_down(KeyEvent::plain(Key::F(7)));
    app.tick(t + 16.0);
    write(&mut app, "world_map");
}

fn load_assets(app: &mut empire_app::App, dir: &str) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.ends_with(".tiles") || name.ends_with(".font") {
            if let Ok(t) = std::fs::read_to_string(&path) {
                let _ = app.load_asset_text(&name, &t);
            }
        }
    }
}

/// UI scenarios that the scripted session doesn't reach: a human capturing a
/// city, and the hot-seat hand-off screen.
fn scenarios(out: String, assets: Option<String>) {
    use empire_app::{App, Key, KeyEvent};
    std::fs::create_dir_all(&out).expect("create output dir");
    let mut t = 5000.0;
    let mut run = |app: &mut App, ms: f64| {
        let end = t + ms;
        while t < end {
            t += 16.0;
            app.tick(t);
        }
    };
    let snap = |app: &mut App, name: &str| {
        app.draw();
        let path = format!("{out}/{name}.ppm");
        std::fs::write(&path, app.frame.to_ppm()).expect("write");
        println!("{path}");
    };

    // 1. Capture: an army next to a neutral city, with the odds stacked.
    let map = "\
..............................
.+++++++++++..................
.+*++++++*++..................
.+++++++++++..........+*+.....
..............................
..............................
";
    let config = GameConfig {
        seed: 4,
        map: MapSource::Text(map.into()),
        players: vec![PlayerConfig::human("TESTER"), PlayerConfig::computer("CPU", Skill::Expert)],
        randomize_orientation: false,
        ..Default::default()
    };
    let mut g = Game::new(&config).expect("game");
    g.clear_board();
    let geom = g.geom();
    let home = g.city_at(geom.loc(2, 2)).unwrap();
    let far = g.city_at(geom.loc(3, 23)).unwrap();
    g.set_city_owner(home, 1);
    g.set_city_owner_production(home, UnitType::Army);
    g.set_city_owner(far, 2);
    g.set_city_owner_production(far, UnitType::Army);
    g.rules.neutral_city_efficiency = 1.0;
    g.players[0].combat_eff = 100;
    g.place_unit(1, UnitType::Army, geom.loc(2, 8)).unwrap();
    let mut app = App::new();
    app.set_viewport(960, 600, DPR);
    if let Some(d) = &assets {
        load_assets(&mut app, d);
    }
    app.key_down(KeyEvent::plain(Key::Space));
    app.settings.report_armies_fighters = false;
    app.start_game(g);
    run(&mut app, 500.0);
    snap(&mut app, "capture_1_prompt");
    app.key_down(KeyEvent::plain(Key::Right));
    run(&mut app, 300.0);
    snap(&mut app, "capture_2_attacking");
    run(&mut app, 2500.0);
    snap(&mut app, "capture_3_result");
    run(&mut app, 2000.0);
    snap(&mut app, "capture_4_production");

    // 2. Hot seat: two humans.
    let config = GameConfig {
        seed: 8,
        players: vec![PlayerConfig::human("ALPHA"), PlayerConfig::human("BRAVO")],
        ..Default::default()
    };
    let g = Game::new(&config).expect("game");
    let mut app = App::new();
    app.set_viewport(960, 600, DPR);
    if let Some(d) = &assets {
        load_assets(&mut app, d);
    }
    app.key_down(KeyEvent::plain(Key::Space));
    app.start_game(g);
    run(&mut app, 300.0);
    snap(&mut app, "hotseat_1_report_p1");
    app.key_down(KeyEvent::plain(Key::Enter));
    run(&mut app, 300.0);
    snap(&mut app, "hotseat_2_p1_production");
    app.key_down(KeyEvent::plain(Key::Enter));
    run(&mut app, 600.0);
    snap(&mut app, "hotseat_3_report_p2");
}
