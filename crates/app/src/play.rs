//! The playing screen: map view, modes, and turning game events into messages.

use crate::commands::Cmd;
use crate::gfx::*;
use crate::input::{Key, KeyEvent};
use crate::layout::{PlayLayout, Zoom, TILE};
use crate::mapdraw::{cell_art, CellArt, MapPainter};
use crate::text::Font;
use crate::theme::Theme;
use crate::ui;
use empire_core::*;
use std::collections::VecDeque;

/// What a target-picking mode will do with the chosen square.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    MoveTo,
    Patrol,
    Escort,
    FlightPath,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Move,
    Survey,
    Direction { unit: UnitId, from_survey: bool },
    Target { kind: TargetKind, from: Loc, unit: Option<UnitId>, city: Option<usize>, from_survey: bool },
    GroupSurvey { anchor: Loc },
    ProductionMap,
    View,
    Computer,
}

impl Mode {
    pub fn label(&self) -> &'static str {
        match self {
            Mode::Move => "MOVE",
            Mode::Survey => "SURVEY",
            Mode::Direction { .. } => "DIRECTION",
            Mode::Target { kind: TargetKind::MoveTo, .. } => "MOVE TO",
            Mode::Target { kind: TargetKind::Patrol, .. } => "PATROL TO",
            Mode::Target { kind: TargetKind::Escort, .. } => "ESCORT",
            Mode::Target { kind: TargetKind::FlightPath, .. } => "MARK FLPTH",
            Mode::GroupSurvey { .. } => "GRP SURVEY",
            Mode::ProductionMap => "PRODUCTION",
            Mode::View => "VIEW",
            Mode::Computer => "COMPUTER",
        }
    }

    fn surveying(&self) -> bool {
        matches!(
            self,
            Mode::Survey
                | Mode::GroupSurvey { .. }
                | Mode::ProductionMap
                | Mode::View
                | Mode::Direction { from_survey: true, .. }
                | Mode::Target { from_survey: true, .. }
        )
    }
}

/// Requests from the play screen to the app.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayEffect {
    Production(usize),
    UnitProduced { city: usize, unit: UnitId },
    Info(Loc),
    WarReport,
    CombatReport,
    ShipReport,
    WorldMap,
    HandOff(PlayerId),
    Surrender(PlayerId),
    GameOver(Option<PlayerId>),
    ConfirmRisky(UnitId, Dir),
    /// Ending the turn would skip this many idle units: ask first.
    ConfirmEndTurn(usize),
    Alert(String),
    Sound(&'static str),
    TurnBegan(PlayerId),
}

#[derive(Clone, Debug)]
struct Msg {
    text: String,
    hold_ms: f64,
    flash: Option<Loc>,
}

pub struct PlayState {
    pub game: Game,
    pub viewer: PlayerId,
    pub mode: Mode,
    pub cursor: Loc,
    pub origin: (usize, usize),
    pub message: String,
    queue: VecDeque<Msg>,
    msg_left: f64,
    pub pending: Option<Pending>,
    pub reveal_all: bool,
    pub save_name: Option<String>,
    pub flash: Option<(Loc, f64)>,
    pub stop_demo: bool,
    pub message_delay: f64,
    pub battle_delay: f64,
    pub report_produced: bool,
    /// Time and on-screen square of the last click, to spot double-clicks.
    last_click: (f64, i32, i32),
    /// Text and map scale; the app keeps this current.
    pub zoom: Zoom,
}

impl PlayState {
    pub fn new(game: Game) -> PlayState {
        let viewer = game.players.iter().find(|p| p.is_human()).map(|p| p.id).unwrap_or(1);
        let start = game.player_cities(viewer).next().map(|c| game.cities[c].loc).unwrap_or(0);
        PlayState {
            game,
            viewer,
            mode: Mode::Move,
            cursor: start,
            origin: (0, 0),
            message: String::new(),
            queue: VecDeque::new(),
            msg_left: 0.0,
            pending: None,
            reveal_all: false,
            save_name: None,
            flash: None,
            stop_demo: false,
            message_delay: 900.0,
            battle_delay: 900.0,
            report_produced: true,
            last_click: (0.0, -1, -1),
            zoom: Zoom::default(),
        }
    }

    /// A digest of everything visible, so the app redraws only on change.
    pub fn signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.game.player(self.viewer).view_rev.hash(&mut h);
        self.viewer.hash(&mut h);
        self.message.hash(&mut h);
        self.mode.label().hash(&mut h);
        self.cursor.hash(&mut h);
        self.origin.hash(&mut h);
        self.game.turn.hash(&mut h);
        self.flash.map(|(l, _)| l).hash(&mut h);
        self.reveal_all.hash(&mut h);
        match &self.pending {
            Some(Pending::UnitOrders { unit, .. }) => (1u8, *unit).hash(&mut h),
            Some(Pending::CityProduction { city, .. }) => (2u8, *city).hash(&mut h),
            Some(p) => (3u8, format!("{p:?}")).hash(&mut h),
            None => 0u8.hash(&mut h),
        }
        h.finish()
    }

    pub fn layout(&self, w: i32, h: i32) -> PlayLayout {
        let digits = (self.game.geom().height.max(self.game.geom().width) - 1).to_string().len() as i32;
        PlayLayout::new(w, h, digits.min(3), self.zoom)
    }

    fn all_computer(&self) -> bool {
        self.game.players.iter().all(|p| !p.is_human())
    }

    // ------------------------------------------------------------------
    // Viewport

    pub fn center_on(&mut self, loc: Loc, lay: &PlayLayout) {
        let g = self.game.geom();
        let (r, c) = (g.row(loc), g.col(loc));
        let oc = c as i64 - lay.cols as i64 / 2;
        let or = r as i64 - lay.rows as i64 / 2;
        self.origin = (self.clamp_col(oc, lay), self.clamp_row(or, lay));
    }

    fn clamp_col(&self, c: i64, lay: &PlayLayout) -> usize {
        let w = self.game.geom().width as i64;
        c.clamp(0, (w - lay.cols as i64).max(0)) as usize
    }

    fn clamp_row(&self, r: i64, lay: &PlayLayout) -> usize {
        let h = self.game.geom().height as i64;
        r.clamp(0, (h - lay.rows as i64).max(0)) as usize
    }

    /// Recentre if `loc` is outside the view or within two squares of its
    /// edge (unless the view already touches the edge of the world).
    pub fn ensure_visible(&mut self, loc: Loc, lay: &PlayLayout) {
        let g = self.game.geom();
        let (r, c) = (g.row(loc), g.col(loc));
        let (oc, or) = self.origin;
        let mc = if lay.cols > 10 { 2 } else { 0 };
        let mr = if lay.rows > 10 { 2 } else { 0 };
        let col_ok = c >= oc
            && c < oc + lay.cols
            && (c >= oc + mc || oc == 0)
            && (c + mc < oc + lay.cols || oc + lay.cols >= g.width);
        let row_ok = r >= or
            && r < or + lay.rows
            && (r >= or + mr || or == 0)
            && (r + mr < or + lay.rows || or + lay.rows >= g.height);
        if !(col_ok && row_ok) {
            self.center_on(loc, lay);
        }
    }

    pub fn scroll(&mut self, dc: i64, dr: i64, lay: &PlayLayout) {
        let (oc, or) = self.origin;
        self.origin = (self.clamp_col(oc as i64 + dc, lay), self.clamp_row(or as i64 + dr, lay));
    }

    pub fn loc_at_pixel(&self, x: i32, y: i32, lay: &PlayLayout) -> Option<Loc> {
        if !lay.map.contains(x, y) {
            return None;
        }
        let (dc, dr) = lay.cell_at(x, y)?;
        let (c, r) = (self.origin.0 + dc, self.origin.1 + dr);
        let g = self.game.geom();
        (c < g.width && r < g.height).then(|| g.loc(r, c))
    }

    /// Where a square is drawn in the offscreen map picture, if on screen.
    fn cell_xy(&self, loc: Loc, lay: &PlayLayout) -> Option<(i32, i32)> {
        let g = self.game.geom();
        let (r, c) = (g.row(loc), g.col(loc));
        if c < self.origin.0 || r < self.origin.1 {
            return None;
        }
        let (dc, dr) = (c - self.origin.0, r - self.origin.1);
        (dc < lay.cols && dr < lay.rows).then(|| (dc as i32 * TILE, dr as i32 * TILE))
    }

    // ------------------------------------------------------------------
    // Text

    pub fn unit_heading(&self, id: UnitId) -> String {
        let g = &self.game;
        let u = g.unit(id);
        let st = g.rules.stats(u.kind);
        let orders = match u.orders {
            Orders::MoveTo(t) => format!("MOVE TO {},{}", g.geom().row(t), g.geom().col(t)),
            Orders::Patrol { to, .. } => format!("PATROL TO {},{}", g.geom().row(to), g.geom().col(to)),
            o => o.describe().to_uppercase(),
        };
        match u.kind {
            UnitType::Army => format!("{}. ORDERS: {}", u.name, orders),
            UnitType::Fighter => format!("{}.  RANGE: {}. ORDERS: {}", u.name, u.fuel, orders),
            k => {
                let cargo = g.cargo(id).len();
                let carrying = match (k, cargo) {
                    (_, 0) => String::new(),
                    (UnitType::Transport, 1) => " W 1 ARMY".into(),
                    (UnitType::Transport, n) => format!(" W {n} ARMIES"),
                    (UnitType::Carrier, 1) => " W 1 FIGHTER".into(),
                    (UnitType::Carrier, n) => format!(" W {n} FIGHTERS"),
                    _ => String::new(),
                };
                format!(
                    "{} {}{}. HITS: {}/{} ORDERS: {}",
                    k.name(),
                    u.name,
                    carrying,
                    st.hits - u.hits,
                    st.hits,
                    orders
                )
            }
        }
        .to_uppercase()
    }

    pub fn describe_loc(&self, loc: Loc) -> String {
        let g = &self.game;
        let geom = g.geom();
        let pos = format!("LOC: {},{} - ", geom.row(loc), geom.col(loc));
        let v = if self.reveal_all { g.truth(loc) } else { g.player(self.viewer).view[loc] };
        if let Some(ci) = g.city_at(loc).filter(|_| v.is_city() || self.reveal_all) {
            let c = &g.cities[ci];
            let who = if c.owner == self.viewer {
                String::new()
            } else if c.owner == 0 {
                " (NEUTRAL)".into()
            } else {
                format!(" (PLAYER {})", c.owner)
            };
            return format!("{pos}CITY {}{}", c.name.to_uppercase(), who);
        }
        if v.owner() == self.viewer || self.reveal_all {
            if let Some(id) = g.top_unit(loc) {
                return pos + &self.unit_heading(id);
            }
        }
        if let Some(t) = v.unit_type() {
            return format!("{pos}ENEMY {}", t.name().to_uppercase());
        }
        pos + if geom.is_border(loc) {
            "EDGE OF THE WORLD"
        } else if v.is_unknown() {
            "UNEXPLORED"
        } else if v.is_sea() {
            "SEA"
        } else {
            "LAND"
        }
    }

    fn say(&mut self, text: impl Into<String>, hold_ms: f64, flash: Option<Loc>) {
        self.queue.push_back(Msg { text: text.into().to_uppercase(), hold_ms, flash });
    }

    /// The unit orders apply to: the prompted unit in Move mode, else the
    /// viewer's unit under the cursor.
    pub fn target_unit(&self) -> Option<UnitId> {
        let g = &self.game;
        if self.mode == Mode::Move {
            if let Some(Pending::UnitOrders { unit, .. }) = self.pending {
                return Some(unit);
            }
        }
        if self.mode.surveying() {
            if g.city_at(self.cursor).is_some() {
                return None;
            }
            return g.top_unit(self.cursor).filter(|&id| g.unit(id).owner == self.viewer);
        }
        None
    }

    pub fn target_city(&self) -> Option<usize> {
        let g = &self.game;
        let loc = match (self.mode, &self.pending) {
            (Mode::Move, Some(Pending::UnitOrders { unit, .. })) => g.unit(*unit).loc,
            _ => self.cursor,
        };
        g.city_at(loc).filter(|&c| g.cities[c].owner == self.viewer)
    }

    pub fn is_viewers_turn(&self) -> bool {
        self.game.current == self.viewer && self.game.player(self.viewer).is_human() && !self.game.game_over
    }

    // ------------------------------------------------------------------
    // Ticking: messages, then game progress

    pub fn tick(&mut self, dt: f64, lay: &PlayLayout) -> Vec<PlayEffect> {
        let mut fx = Vec::new();
        if let Some((_, t)) = &mut self.flash {
            *t -= dt;
            if *t <= 0.0 {
                self.flash = None;
            }
        }
        // Paced messages hold the game.
        if self.msg_left > 0.0 {
            self.msg_left -= dt;
            return fx;
        }
        if let Some(m) = self.queue.pop_front() {
            self.message = m.text;
            self.msg_left = m.hold_ms;
            if let Some(loc) = m.flash {
                self.flash = Some((loc, m.hold_ms.max(300.0)));
                if self.cell_xy(loc, lay).is_none() {
                    self.center_on(loc, lay);
                }
            }
            return fx;
        }
        if matches!(self.mode, Mode::View) {
            return fx;
        }
        let need_advance = matches!(self.pending, None | Some(Pending::Busy));
        if need_advance {
            if self.all_computer() && self.stop_demo {
                self.mode = Mode::View;
                self.stop_demo = false;
                return fx;
            }
            let budget = if self.all_computer() { 25 } else { 60 };
            let p = self.game.advance(budget);
            self.handle_events(&mut fx);
            self.pending = Some(p.clone());
            match p {
                Pending::Busy => {
                    if !self.mode.surveying() {
                        self.mode = Mode::Computer;
                    }
                }
                Pending::UnitOrders { player, unit, reason } => {
                    self.viewer = player;
                    if !matches!(self.mode, Mode::Survey | Mode::GroupSurvey { .. } | Mode::ProductionMap) {
                        self.mode = Mode::Move;
                    }
                    if self.mode == Mode::Move {
                        let loc = self.game.unit(unit).loc;
                        self.cursor = loc;
                        self.ensure_visible(loc, lay);
                        let heading = self.unit_heading(unit);
                        let r = reason.message();
                        self.message = if r.is_empty() { heading } else { format!("{} {}", r.to_uppercase(), heading) };
                    }
                }
                Pending::CityProduction { player, city } => {
                    self.viewer = player;
                    let loc = self.game.cities[city].loc;
                    self.cursor = loc;
                    self.ensure_visible(loc, lay);
                    fx.push(PlayEffect::Production(city));
                }
                Pending::HandOff { player } => {
                    self.viewer = player;
                    fx.push(PlayEffect::HandOff(player));
                }
                Pending::SurrenderOffer { from, .. } => fx.push(PlayEffect::Surrender(from)),
                Pending::GameOver { winner } => {
                    self.mode = Mode::View;
                    fx.push(PlayEffect::GameOver(winner));
                }
            }
        }
        fx
    }

    fn handle_events(&mut self, fx: &mut Vec<PlayEffect>) {
        let mut me = self.viewer;
        let events = self.game.drain_events();
        let demo = self.all_computer();
        for e in events {
            // In hot-seat games the view follows whichever human's turn begins,
            // so their production reports reach them.
            if let Event::TurnStarted { player, .. } = e {
                if self.game.player(player).is_human() && player != me {
                    me = player;
                    self.viewer = player;
                }
            }
            if !demo && !e.concerns(me) {
                continue;
            }
            match e {
                Event::TurnStarted { player, .. } => {
                    if player == me {
                        fx.push(PlayEffect::TurnBegan(player));
                    }
                }
                Event::Produced { player, city, unit, kind } => {
                    if player == me && self.game.player(me).is_human()
                        && (kind.is_ship() || self.report_produced) {
                            fx.push(PlayEffect::UnitProduced { city, unit });
                        }
                }
                Event::OverPopulation { .. } => fx.push(PlayEffect::Alert("OVER POPULATION! CITY PRODUCTION HAS STOPPED.".into())),
                Event::Combat { loc, attacker, defender, attacker_won, bombardment } => {
                    let hold = self.battle_delay;
                    let target = if defender.owner == me { defender.name.clone() } else { format!("an enemy {}", defender.kind.name()) };
                    let who = if attacker.owner == me { attacker.name.clone() } else { format!("An enemy {}", attacker.kind.name()) };
                    let verb = if bombardment { "is bombarding" } else { "is attacking" };
                    self.say(format!("{who} {verb} {target}"), hold, Some(loc));
                    let result = if attacker.owner == me {
                        if attacker_won {
                            format!("Your {} destroyed the enemy {}.", attacker.name, defender.kind.name())
                        } else {
                            format!("Your {} was destroyed.", attacker.name)
                        }
                    } else if defender.owner == me {
                        if attacker_won {
                            format!("Your {} was destroyed.", defender.name)
                        } else {
                            format!("Your {} destroyed the enemy {}.", defender.name, attacker.kind.name())
                        }
                    } else {
                        format!("Player {} {} {} player {} {}.", attacker.owner, attacker.kind.name(), if attacker_won { "destroyed" } else { "lost to" }, defender.owner, defender.kind.name())
                    };
                    self.say(result, self.message_delay, None);
                    fx.push(PlayEffect::Sound(if attacker_won == (attacker.owner == me) { "win" } else { "lose" }));
                }
                Event::CityCaptured { loc, city, by, from, unit } => {
                    let name = self.game.cities[city].name.clone();
                    if by == me {
                        self.say(format!("{unit} is attacking {name}"), self.battle_delay, Some(loc));
                        self.say(format!("Your {unit} captured {name}."), self.message_delay, None);
                        fx.push(PlayEffect::Sound("capture"));
                    } else if from == me {
                        self.say(format!("An enemy army is attacking {name}"), self.battle_delay, Some(loc));
                        self.say(format!("{name} has fallen to the enemy!"), self.message_delay, None);
                    } else if demo {
                        self.say(format!("{unit} is attacking {name}"), self.battle_delay / 3.0, Some(loc));
                    }
                }
                Event::CityRepelled { loc, city, attacker, unit, .. } => {
                    let name = self.game.cities[city].name.clone();
                    if attacker == me {
                        self.say(format!("{unit} is attacking {name}"), self.battle_delay, Some(loc));
                        self.say(format!("Your {unit} was crushed by the defenders of {name}."), self.message_delay, None);
                    } else {
                        self.say(format!("An enemy army is attacking {name}"), self.battle_delay, Some(loc));
                        self.say(format!("{name} repelled the invasion!"), self.message_delay, None);
                    }
                }
                Event::UnitLost { loc, unit, cause } => {
                    if unit.owner != me {
                        continue;
                    }
                    let why = match cause {
                        LossCause::Drowned => "drowned",
                        LossCause::OutOfFuel => "ran out of fuel and crashed",
                        LossCause::ShotDown => "was shot down",
                        LossCause::Overloaded => "was lost overboard",
                        LossCause::CityCaptured => "was lost when the city fell",
                        LossCause::WentDownWithShip => "went down with its ship",
                    };
                    self.say(format!("Your {} {why}.", unit.name), self.message_delay, Some(loc));
                }
                Event::CargoLost { loc, owner, kind, count, cause } => {
                    if owner != me {
                        continue;
                    }
                    let what = if kind == UnitType::Army { "armies" } else { "fighters" };
                    let how = if cause == LossCause::Overloaded { "lost: the ship can't carry them" } else { "lost with the ship" };
                    self.say(format!("{count} {what} {how}."), self.message_delay, Some(loc));
                }
                Event::Boarded { loc, unit } => {
                    if unit.owner == me && unit.kind == UnitType::Army && self.game.player(me).is_human() {
                        let carrier = self.game.top_unit(loc).map(|t| self.game.unit(t).name.clone()).unwrap_or_default();
                        self.message = format!("TRANSPORT {} CONFIRMS LOADING OF THE {}.", carrier, unit.name).to_uppercase();
                    }
                }
                Event::PhotoRecon { city, production, .. } => {
                    let name = self.game.cities[city].name.clone();
                    let what = production.map(|k| k.name().to_string()).unwrap_or_else(|| "nothing".into());
                    self.say(format!("Photo recon: {name} is producing {what}."), self.message_delay / 2.0, None);
                }
                Event::PlayerDefeated { player } => {
                    let text = if player == me { "You have been defeated!".to_string() } else { format!("Player {player} has been crushed!") };
                    self.say(text, self.message_delay * 2.0, None);
                }
                Event::SurrenderOffered { .. } | Event::GameOver { .. } => {}
                Event::Landed { loc, unit } => {
                    if unit.owner == me {
                        if let Some(ci) = self.game.city_at(loc) {
                            self.message = format!("LANDING CONFIRMED AT {}.", self.game.cities[ci].name.to_uppercase());
                        }
                    }
                }
                Event::Docked { .. } => {}
            }
        }
    }

    // ------------------------------------------------------------------
    // Input

    /// Skip the current paced message.
    pub fn skip_message(&mut self) -> bool {
        if self.msg_left > 0.0 {
            self.msg_left = 0.0;
            return true;
        }
        false
    }

    pub fn busy_with_messages(&self) -> bool {
        self.msg_left > 0.0 || !self.queue.is_empty()
    }

    pub fn key(&mut self, ev: KeyEvent, lay: &PlayLayout) -> Vec<PlayEffect> {
        let mut fx = Vec::new();
        if self.skip_message() {
            return fx;
        }
        if self.mode == Mode::Computer && self.all_computer() {
            self.stop_demo = true;
            return fx;
        }
        let dir = ev.direction();
        match self.mode {
            Mode::Move => {
                if let (Some(d), Some(Pending::UnitOrders { unit, .. })) = (dir, self.pending.clone()) {
                    self.try_move(unit, d, false, &mut fx);
                    return fx;
                }
            }
            Mode::Survey | Mode::ProductionMap | Mode::View | Mode::GroupSurvey { .. } => {
                if let Some(d) = dir {
                    let steps = if ev.shift { 5 } else { 1 };
                    self.move_cursor(d, steps, lay);
                    if !matches!(self.mode, Mode::GroupSurvey { .. }) {
                        self.message = self.describe_loc(self.cursor);
                    }
                    return fx;
                }
                if ev.key == Key::Enter {
                    if self.game.city_at(self.cursor).is_some() || self.game.top_unit(self.cursor).is_some() {
                        fx.push(PlayEffect::Info(self.cursor));
                    }
                    return fx;
                }
                if ev.key == Key::Esc && self.mode != Mode::View {
                    self.set_mode(Mode::Move);
                    return fx;
                }
            }
            Mode::Direction { unit, from_survey } => {
                if let Some(d) = dir {
                    match self.game.cmd_set_orders(unit, Orders::Direction(d)) {
                        Ok(()) => self.after_orders(unit),
                        Err(e) => self.error(&e, &mut fx),
                    }
                    self.mode = if from_survey { Mode::Survey } else { Mode::Move };
                    return fx;
                }
                if ev.key == Key::Esc {
                    self.mode = if from_survey { Mode::Survey } else { Mode::Move };
                    return fx;
                }
            }
            Mode::Target { kind, .. } => {
                if let Some(d) = dir {
                    let steps = if ev.shift { 5 } else { 1 };
                    self.move_cursor(d, steps, lay);
                    self.target_message();
                    return fx;
                }
                let confirm_key = match kind {
                    TargetKind::MoveTo => ev.letter() == Some('T') || ev.key == Key::PadPlus,
                    TargetKind::Patrol => ev.letter() == Some('P'),
                    TargetKind::Escort => ev.letter() == Some('E'),
                    TargetKind::FlightPath => ev.letter() == Some('F'),
                };
                if ev.key == Key::Enter || confirm_key {
                    self.finish_target(&mut fx);
                    return fx;
                }
                if ev.key == Key::Esc {
                    if kind == TargetKind::FlightPath {
                        if let Mode::Target { city: Some(ci), .. } = self.mode {
                            let _ = self.game.cmd_set_flight_path(ci, None);
                            self.message = "FLIGHT PATH REMOVED".into();
                        }
                    }
                    self.exit_target_mode();
                    return fx;
                }
            }
            Mode::Computer => {}
        }
        if let Some(cmd) = crate::commands::shortcut(&ev) {
            return self.command(cmd, lay);
        }
        fx
    }

    fn move_cursor(&mut self, d: Dir, steps: usize, lay: &PlayLayout) {
        let g = self.game.geom();
        for _ in 0..steps {
            if let Some(n) = g.step(self.cursor, d) {
                self.cursor = n;
            }
        }
        self.ensure_visible(self.cursor, lay);
    }

    fn target_message(&mut self) {
        if let Mode::Target { from, .. } = self.mode {
            let d = self.game.geom().dist(from, self.cursor);
            self.message = format!("DISTANCE = {d}");
        }
    }

    fn set_mode(&mut self, m: Mode) {
        self.mode = m;
        match m {
            Mode::Move => {
                if let Some(Pending::UnitOrders { unit, .. }) = self.pending {
                    if self.game.unit(unit).alive {
                        self.cursor = self.game.unit(unit).loc;
                        self.message = self.unit_heading(unit);
                    }
                }
            }
            Mode::Survey | Mode::ProductionMap => self.message = self.describe_loc(self.cursor),
            _ => {}
        }
    }

    fn exit_target_mode(&mut self) {
        if let Mode::Target { from_survey, .. } = self.mode {
            self.set_mode(if from_survey { Mode::Survey } else { Mode::Move });
        }
    }

    fn error(&mut self, e: &str, fx: &mut Vec<PlayEffect>) {
        self.message = e.to_uppercase();
        fx.push(PlayEffect::Sound("error"));
    }

    /// After orders to the prompted unit, let the game continue.
    fn after_orders(&mut self, unit: UnitId) {
        if let Some(Pending::UnitOrders { unit: u, .. }) = self.pending {
            if u == unit {
                self.pending = None;
            }
        }
        if self.game.unit(unit).alive {
            self.message = self.unit_heading(unit);
        }
    }

    pub fn try_move(&mut self, unit: UnitId, d: Dir, confirmed: bool, fx: &mut Vec<PlayEffect>) {
        if !confirmed && self.game.move_is_risky(unit, d) {
            fx.push(PlayEffect::ConfirmRisky(unit, d));
            return;
        }
        match self.game.cmd_move(unit, d) {
            Ok(_) => {
                self.pending = None;
                let mut sub = Vec::new();
                self.handle_events(&mut sub);
                fx.extend(sub);
                fx.push(PlayEffect::Sound("move"));
            }
            Err(e) => self.error(e.message(), fx),
        }
    }

    fn finish_target(&mut self, fx: &mut Vec<PlayEffect>) {
        let Mode::Target { kind, from, unit, city, .. } = self.mode else { return };
        let target = self.cursor;
        let result = match (kind, unit, city) {
            (TargetKind::FlightPath, _, Some(ci)) => {
                let t = if self.game.city_at(target).is_some() && target != from { Some(target) } else { None };
                let r = self.game.cmd_set_flight_path(ci, t);
                if r.is_ok() {
                    self.message = if t.is_some() { "FLIGHT PATH SET".into() } else { "FLIGHT PATH REMOVED".into() };
                }
                r
            }
            (_, Some(u), _) if target == from => {
                self.message = "ORDER CANCELLED".into();
                let _ = u;
                Ok(())
            }
            (TargetKind::MoveTo, Some(u), _) => self.game.cmd_set_orders(u, Orders::MoveTo(target)).map(|_| self.after_orders(u)),
            (TargetKind::Patrol, Some(u), _) => {
                self.game.cmd_set_orders(u, Orders::Patrol { from, to: target, outbound: true }).map(|_| self.after_orders(u))
            }
            (TargetKind::Escort, Some(u), _) => match self.game.top_unit(target) {
                Some(t) if self.game.unit(t).owner == self.viewer => {
                    let r = self.game.unit_ref(t);
                    self.game.cmd_set_orders(u, Orders::Escort(r)).map(|_| self.after_orders(u))
                }
                _ => Err("Pick one of your ships to escort".into()),
            },
            _ => Err("Nothing to give orders to".into()),
        };
        if let Err(e) = result {
            self.error(&e, fx);
            return;
        }
        self.exit_target_mode();
    }

    pub fn click(&mut self, x: i32, y: i32, right: bool, now: f64, lay: &PlayLayout) -> Vec<PlayEffect> {
        let mut fx = Vec::new();
        if self.skip_message() {
            return fx;
        }
        // Scroll bars.
        if lay.up_box.contains(x, y) {
            self.scroll(0, -1, lay);
            return fx;
        }
        if lay.down_box.contains(x, y) {
            self.scroll(0, 1, lay);
            return fx;
        }
        if lay.left_box.contains(x, y) {
            self.scroll(-1, 0, lay);
            return fx;
        }
        if lay.right_box.contains(x, y) {
            self.scroll(1, 0, lay);
            return fx;
        }
        if lay.vtrack.contains(x, y) {
            let h = self.game.geom().height as i64;
            let r = (y - lay.vtrack.y) as i64 * h / lay.vtrack.h.max(1) as i64;
            self.origin.1 = self.clamp_row(r - lay.rows as i64 / 2, lay);
            return fx;
        }
        if lay.htrack.contains(x, y) {
            let w = self.game.geom().width as i64;
            let c = (x - lay.htrack.x) as i64 * w / lay.htrack.w.max(1) as i64;
            self.origin.0 = self.clamp_col(c - lay.cols as i64 / 2, lay);
            return fx;
        }
        let Some(loc) = self.loc_at_pixel(x, y, lay) else { return fx };
        let (cx, cy) = lay.cell_at(x, y).map(|(c, r)| (c as i32, r as i32)).unwrap_or((-1, -1));
        let double = now - self.last_click.0 < 400.0 && self.last_click.1 == cx && self.last_click.2 == cy;
        self.last_click = (now, cx, cy);
        if right {
            match self.mode {
                Mode::Move => {
                    self.cursor = loc;
                    self.set_mode(Mode::Survey);
                }
                Mode::Survey | Mode::ProductionMap | Mode::GroupSurvey { .. } => self.set_mode(Mode::Move),
                _ => {}
            }
            return fx;
        }
        if double {
            fx.push(PlayEffect::Info(loc));
            return fx;
        }
        match self.mode {
            Mode::Move => {
                if let Some(Pending::UnitOrders { unit, .. }) = self.pending.clone() {
                    let ul = self.game.unit(unit).loc;
                    let g = self.game.geom();
                    if let Some(d) = g.dir_to_adjacent(ul, loc) {
                        self.try_move(unit, d, false, &mut fx);
                    } else if loc != ul {
                        match self.game.cmd_set_orders(unit, Orders::MoveTo(loc)) {
                            Ok(()) => self.after_orders(unit),
                            Err(e) => self.error(&e, &mut fx),
                        }
                    }
                }
            }
            Mode::Survey | Mode::ProductionMap | Mode::View => {
                self.cursor = loc;
                self.message = self.describe_loc(loc);
            }
            Mode::GroupSurvey { .. } => self.cursor = loc,
            Mode::Direction { unit, from_survey } => {
                let ul = self.game.unit(unit).loc;
                if let Some(d) = self.game.geom().dir_to_adjacent(ul, loc) {
                    match self.game.cmd_set_orders(unit, Orders::Direction(d)) {
                        Ok(()) => self.after_orders(unit),
                        Err(e) => self.error(&e, &mut fx),
                    }
                }
                self.mode = if from_survey { Mode::Survey } else { Mode::Move };
            }
            Mode::Target { .. } => {
                self.cursor = loc;
                self.finish_target(&mut fx);
            }
            Mode::Computer => {
                if self.all_computer() {
                    self.stop_demo = true;
                }
            }
        }
        fx
    }

    /// Skip the idle units and let the turn finish.
    pub fn end_turn_now(&mut self) {
        let n = self.game.cmd_end_turn();
        self.pending = None;
        self.set_mode(Mode::Move);
        self.message = match n {
            0 => "ENDING TURN".into(),
            1 => "ENDING TURN: 1 UNIT SKIPPED".into(),
            n => format!("ENDING TURN: {n} UNITS SKIPPED"),
        };
    }

    // ------------------------------------------------------------------
    // Commands

    pub fn command(&mut self, cmd: Cmd, lay: &PlayLayout) -> Vec<PlayEffect> {
        use Cmd::*;
        let mut fx = Vec::new();
        let unit = self.target_unit();
        let human_turn = self.is_viewers_turn();
        let order = |s: &mut Self, u: Option<UnitId>, o: Orders, fx: &mut Vec<PlayEffect>| match u {
            Some(u) => match s.game.cmd_set_orders(u, o) {
                Ok(()) => s.after_orders(u),
                Err(e) => s.error(&e, fx),
            },
            None => s.error("No unit selected", fx),
        };
        match cmd {
            ScrollLeft => self.scroll(-1, 0, lay),
            ScrollRight => self.scroll(1, 0, lay),
            ScrollUp => self.scroll(0, -1, lay),
            ScrollDown => self.scroll(0, 1, lay),
            PageLeft => self.scroll(-(lay.cols as i64) / 2, 0, lay),
            PageRight => self.scroll(lay.cols as i64 / 2, 0, lay),
            PageUp => self.scroll(0, -(lay.rows as i64) / 2, lay),
            PageDown => self.scroll(0, lay.rows as i64 / 2, lay),
            CenterScreen => self.center_on(self.cursor, lay),
            CenterCursor => {
                let g = self.game.geom();
                let r = (self.origin.1 + lay.rows / 2).min(g.height - 1);
                let c = (self.origin.0 + lay.cols / 2).min(g.width - 1);
                self.cursor = g.loc(r, c);
            }
            Info => fx.push(PlayEffect::Info(match (self.mode, &self.pending) {
                (Mode::Move, Some(Pending::UnitOrders { unit, .. })) => self.game.unit(*unit).loc,
                _ => self.cursor,
            })),
            Status => fx.push(PlayEffect::WarReport),
            CombatReport => fx.push(PlayEffect::CombatReport),
            ShipReport => fx.push(PlayEffect::ShipReport),
            WorldMap => fx.push(PlayEffect::WorldMap),
            SetProduction => match self.target_city() {
                Some(c) if human_turn => fx.push(PlayEffect::Production(c)),
                _ => self.error("Put the cursor on one of your cities", &mut fx),
            },
            ProductionMap => {
                self.set_mode(if self.mode == Mode::ProductionMap { Mode::Move } else { Mode::ProductionMap });
            }
            MoveMode => self.set_mode(Mode::Move),
            SurveyMode => {
                if self.mode == Mode::Survey {
                    self.set_mode(Mode::Move);
                } else {
                    self.set_mode(Mode::Survey);
                }
            }
            GroupSurvey => {
                self.mode = Mode::GroupSurvey { anchor: self.cursor };
                self.message = "GROUP SURVEY: MOVE THE CURSOR TO SIZE THE BOX, THEN GIVE AN ORDER".into();
            }
            _ if !human_turn => self.error("Wait for your turn", &mut fx),
            SkipMove => {
                if let (Mode::Move, Some(Pending::UnitOrders { unit, .. })) = (self.mode, self.pending.clone()) {
                    let _ = self.game.cmd_skip(unit);
                    self.pending = None;
                }
            }
            Wait => {
                if let (Mode::Move, Some(Pending::UnitOrders { unit, .. })) = (self.mode, self.pending.clone()) {
                    let _ = self.game.cmd_wait(unit);
                    self.pending = None;
                }
            }
            NextUnit => match (self.mode, self.pending.clone()) {
                (Mode::Move, Some(Pending::UnitOrders { unit, .. })) => {
                    let _ = self.game.cmd_wait(unit);
                    self.pending = None;
                }
                _ => {
                    // Surveying: put the cursor on the next unit without orders.
                    let g = &self.game;
                    let here = g.units_at(self.cursor).iter().copied().max().unwrap_or(0);
                    let idle: Vec<UnitId> = g
                        .player_units(self.viewer)
                        .filter(|&i| {
                            let u = g.unit(i);
                            !u.moved && u.moves_left > 0 && u.orders == Orders::None
                        })
                        .collect();
                    match idle.iter().find(|&&i| i > here).or(idle.first()) {
                        Some(&i) => {
                            self.cursor = g.unit(i).loc;
                            self.ensure_visible(self.cursor, lay);
                        }
                        None => self.message = "NO UNITS ARE WAITING FOR ORDERS".into(),
                    }
                }
            },
            EndTurn => {
                let n = self.game.idle_units();
                if n > 10 {
                    fx.push(PlayEffect::ConfirmEndTurn(n));
                } else {
                    self.end_turn_now();
                }
            }
            Activate => {
                if let Some(u) = unit {
                    match self.game.cmd_activate(u) {
                        Ok(()) => {
                            self.pending = None;
                            self.set_mode(Mode::Move);
                        }
                        Err(e) => self.error(&e, &mut fx),
                    }
                }
            }
            GoHome | GoRandom | Sentry | ClearOrders | LoadShip | UnloadShip if matches!(self.mode, Mode::GroupSurvey { .. }) => {
                self.group_order(cmd, &mut fx);
            }
            GoHome => order(self, unit, Orders::GoHome, &mut fx),
            GoRandom => order(self, unit, Orders::Random, &mut fx),
            Sentry => {
                if self.mode == Mode::Survey && unit.is_none() {
                    // Sentry everything in a city.
                    let ids: Vec<UnitId> = self.game.units_at(self.cursor).iter().copied().filter(|&i| self.game.unit(i).owner == self.viewer).collect();
                    for i in ids {
                        let _ = self.game.cmd_set_orders(i, Orders::Sentry);
                    }
                    self.message = "UNITS IN THE CITY ARE ON SENTRY".into();
                } else {
                    order(self, unit, Orders::Sentry, &mut fx)
                }
            }
            LoadShip => order(self, unit, Orders::Load, &mut fx),
            ClearOrders => {
                let loc = match (self.mode, &self.pending) {
                    (Mode::Move, Some(Pending::UnitOrders { unit, .. })) => self.game.unit(*unit).loc,
                    _ => self.cursor,
                };
                match self.game.cmd_clear_orders_at(loc) {
                    Ok(()) => self.message = "ORDERS CLEARED".into(),
                    Err(e) => self.error(&e, &mut fx),
                }
            }
            UnloadShip => {
                let u = unit.or_else(|| self.game.units_at(self.cursor).iter().copied().find(|&i| self.game.unit(i).owner == self.viewer));
                match u.map(|u| self.game.cmd_unload(u)) {
                    Some(Ok(n)) => {
                        self.message = format!("{n} UNITS PREPARING TO UNLOAD");
                        self.pending = None;
                    }
                    Some(Err(e)) => self.error(&e, &mut fx),
                    None => self.error("No transport or carrier here", &mut fx),
                }
            }
            GoDirection => match unit {
                Some(u) => {
                    self.mode = Mode::Direction { unit: u, from_survey: self.mode.surveying() };
                    self.message = "WHICH DIRECTION?".into();
                }
                None => self.error("No unit selected", &mut fx),
            },
            MoveTo | PatrolTo | EscortShip => match unit {
                Some(u) => {
                    let kind = match cmd {
                        MoveTo => TargetKind::MoveTo,
                        PatrolTo => TargetKind::Patrol,
                        _ => TargetKind::Escort,
                    };
                    if kind == TargetKind::Escort && !self.game.unit(u).kind.is_ship() {
                        self.error("Only ships can escort", &mut fx);
                    } else {
                        let from = self.game.unit(u).loc;
                        self.mode = Mode::Target { kind, from, unit: Some(u), city: None, from_survey: self.mode.surveying() };
                        self.target_message();
                    }
                }
                None => self.error("No unit selected", &mut fx),
            },
            FlightPaths => match self.target_city() {
                Some(ci) => {
                    let from = self.game.cities[ci].loc;
                    self.mode = Mode::Target { kind: TargetKind::FlightPath, from, unit: None, city: Some(ci), from_survey: true };
                    self.cursor = self.game.cities[ci].flight_path.unwrap_or(from);
                    self.message = "SELECT THE DESTINATION CITY (ESC REMOVES THE PATH)".into();
                }
                None => self.error("Flight paths start at one of your cities", &mut fx),
            },
            _ => {}
        }
        fx
    }

    fn group_order(&mut self, cmd: Cmd, fx: &mut Vec<PlayEffect>) {
        let Mode::GroupSurvey { anchor } = self.mode else { return };
        let g = self.game.geom();
        let (r0, r1) = (g.row(anchor).min(g.row(self.cursor)), g.row(anchor).max(g.row(self.cursor)));
        let (c0, c1) = (g.col(anchor).min(g.col(self.cursor)), g.col(anchor).max(g.col(self.cursor)));
        let ids: Vec<UnitId> = self
            .game
            .player_units(self.viewer)
            .filter(|&i| {
                let l = self.game.unit(i).loc;
                (r0..=r1).contains(&g.row(l)) && (c0..=c1).contains(&g.col(l))
            })
            .collect();
        let mut n = 0;
        for id in ids {
            let r = match cmd {
                Cmd::GoHome => self.game.cmd_set_orders(id, Orders::GoHome),
                Cmd::GoRandom => self.game.cmd_set_orders(id, Orders::Random),
                Cmd::Sentry => self.game.cmd_set_orders(id, Orders::Sentry),
                Cmd::LoadShip => self.game.cmd_set_orders(id, Orders::Load),
                Cmd::ClearOrders => {
                    self.game.units[id].orders = Orders::None;
                    Ok(())
                }
                Cmd::UnloadShip => self.game.cmd_unload(id).map(|_| ()),
                _ => Ok(()),
            };
            if r.is_ok() {
                n += 1;
            }
        }
        self.message = format!("ORDER GIVEN TO {n} UNITS");
        if n == 0 {
            fx.push(PlayEffect::Sound("error"));
        }
        self.pending = None;
        self.set_mode(Mode::Survey);
    }

    // ------------------------------------------------------------------
    // Drawing

    /// Draw the playing screen. The map is drawn into `mf` at its own scale
    /// and then copied into place.
    pub fn draw(&self, f: &mut Frame, mf: &mut Frame, font: &Font, theme: &Theme, painter: &MapPainter, lay: &PlayLayout, blink: bool) {
        let g = &self.game;
        let geom = g.geom();
        // Message line.
        let max_chars = ((lay.width - 8) / font.advance) as usize;
        let msg: String = self.message.chars().take(max_chars).collect();
        font.draw(f, 4, lay.msg_y, &msg, theme.text, None);
        f.hline(0, lay.width - 1, lay.rule2_y, theme.rule);

        // Frame.
        let fr = lay.frame;
        f.vline(fr.x, fr.y, fr.bottom() - 1, theme.rule);
        f.vline(fr.right() - 1, fr.y, fr.bottom() - 1, theme.rule);
        f.hline(fr.x, fr.right() - 1, fr.bottom() - 1, theme.rule);
        f.vline(lay.up_box.right(), fr.y, lay.down_box.bottom() - 1, theme.rule);
        f.hline(fr.x, lay.up_box.right(), lay.up_box.bottom(), theme.rule);
        f.hline(fr.x, lay.up_box.right(), lay.down_box.y - 1, theme.rule);
        f.hline(fr.x, fr.right() - 1, lay.left_box.y - 1, theme.rule);
        f.vline(lay.left_box.x - 1, lay.left_box.y, lay.left_box.bottom() - 1, theme.rule);
        f.vline(lay.left_box.right(), lay.left_box.y, lay.left_box.bottom() - 1, theme.rule);
        f.vline(lay.right_box.x - 1, lay.right_box.y, lay.right_box.bottom() - 1, theme.rule);
        font.draw(f, lay.up_box.x + 2, lay.up_box.y - 1, "3", theme.text, None);
        font.draw(f, lay.down_box.x + 2, lay.down_box.y - 1, "4", theme.text, None);
        font.draw(f, lay.left_box.x + 2, lay.left_box.y - 1, "1", theme.text, None);
        font.draw(f, lay.right_box.x + 2, lay.right_box.y - 1, "2", theme.text, None);
        ui::vscroll(f, lay.vtrack, self.origin.1, lay.rows, geom.height, BLACK, theme.rule);
        ui::hscroll(f, lay.htrack, self.origin.0, lay.cols, geom.width, BLACK, theme.rule);

        // Number strips: every even row and column, or sparser when zoomed out.
        f.fill(lay.row_strip, theme.strip_bg);
        f.fill(lay.col_strip, theme.strip_bg);
        draw_strip_numbers(f, font, theme, lay, self.origin, (geom.width, geom.height));

        // Map cells, drawn into the offscreen map picture.
        f.fill(lay.map, BLACK);
        let (mw, mh, mscale) = lay.map_picture();
        mf.resize(mw, mh, mscale);
        mf.clear(BLACK);
        let map_rect = Rect::new(0, 0, mw as i32, mh as i32);
        let prompt_unit = match (&self.pending, self.mode) {
            (Some(Pending::UnitOrders { unit, .. }), Mode::Move | Mode::Direction { .. } | Mode::Target { .. }) => Some(*unit),
            _ => None,
        };
        for dr in 0..lay.rows {
            for dc in 0..lay.cols {
                let (r, c) = (self.origin.1 + dr, self.origin.0 + dc);
                if r >= geom.height || c >= geom.width {
                    continue;
                }
                let loc = geom.loc(r, c);
                let x = dc as i32 * TILE;
                let y = dr as i32 * TILE;
                let mut art = cell_art(g, self.viewer, loc, self.reveal_all);
                if self.mode == Mode::ProductionMap {
                    if let CellArt::Unit { .. } = art {
                        art = MapPainter::terrain_art(g, self.viewer, loc);
                    }
                    if let Some(ci) = g.city_at(loc) {
                        let c = &g.cities[ci];
                        if c.owner == self.viewer {
                            if let Some(k) = c.production {
                                art = CellArt::Unit { kind: k, owner: self.viewer, over_sea: false, sentry: false, loaded: false };
                            }
                        }
                    }
                }
                painter.draw_art(mf, x, y, art);
            }
        }

        // The prompted unit blinks; it may be hidden inside a city or ship.
        if let Some(u) = prompt_unit {
            let unit = g.unit(u);
            if unit.alive {
                if let Some((x, y)) = self.cell_xy(unit.loc, lay) {
                    let over_sea = g.map.get(unit.loc) == Terrain::Sea;
                    let art = if blink {
                        CellArt::Unit {
                            kind: unit.kind,
                            owner: unit.owner,
                            over_sea,
                            sentry: false,
                            loaded: unit.kind.cargo().is_some() && !g.cargo(u).is_empty(),
                        }
                    } else {
                        MapPainter::terrain_art(g, self.viewer, unit.loc)
                    };
                    painter.draw_art(mf, x, y, art);
                }
            }
        }

        // Lines: flight paths, patrols and escorts while picking targets.
        if let Mode::Target { kind, from, .. } = self.mode {
            match kind {
                TargetKind::FlightPath => {
                    for c in g.cities.iter().filter(|c| c.owner == self.viewer) {
                        if let Some(t) = c.flight_path {
                            self.draw_link(mf, c.loc, t, lay, WHITE);
                        }
                    }
                }
                TargetKind::Patrol => {
                    for id in g.player_units(self.viewer) {
                        if let Orders::Patrol { from, to, .. } = g.unit(id).orders {
                            self.draw_link(mf, from, to, lay, CYAN);
                        }
                    }
                }
                TargetKind::Escort => {
                    for id in g.player_units(self.viewer) {
                        if let Orders::Escort(r) = g.unit(id).orders {
                            if g.unit_valid(r) {
                                self.draw_link(mf, g.unit(id).loc, g.unit(r.id).loc, lay, WHITE);
                            }
                        }
                    }
                }
                TargetKind::MoveTo => {}
            }
            self.draw_ants(mf, from, self.cursor, lay, blink);
        }

        // Cursors.
        match self.mode {
            Mode::Survey | Mode::ProductionMap | Mode::View | Mode::Target { .. }
                if blink => {
                    if let Some((x, y)) = self.cell_xy(self.cursor, lay) {
                        painter.draw_cursor(mf, x, y, "cursor_survey");
                    }
                }
            Mode::Direction { unit, .. } => {
                if let Some((x, y)) = self.cell_xy(g.unit(unit).loc, lay) {
                    if blink {
                        painter.draw_cursor(mf, x, y, "cursor_direction");
                    }
                }
            }
            Mode::GroupSurvey { anchor } => {
                let (r0, r1) = (geom.row(anchor).min(geom.row(self.cursor)), geom.row(anchor).max(geom.row(self.cursor)));
                let (c0, c1) = (geom.col(anchor).min(geom.col(self.cursor)), geom.col(anchor).max(geom.col(self.cursor)));
                let x0 = (c0 as i32 - self.origin.0 as i32) * TILE;
                let y0 = (r0 as i32 - self.origin.1 as i32) * TILE;
                let w = (c1 - c0 + 1) as i32 * TILE;
                let h = (r1 - r0 + 1) as i32 * TILE;
                let col = if blink { WHITE } else { MAGENTA };
                let clip = |v: i32, lo: i32, hi: i32| v.clamp(lo, hi);
                let (cx0, cy0) = (clip(x0, 0, map_rect.right() - 1), clip(y0, 0, map_rect.bottom() - 1));
                let (cx1, cy1) = (clip(x0 + w - 1, 0, map_rect.right() - 1), clip(y0 + h - 1, 0, map_rect.bottom() - 1));
                mf.frame(Rect::new(cx0, cy0, cx1 - cx0 + 1, cy1 - cy0 + 1), col);
            }
            _ => {}
        }

        if let Some((loc, _)) = self.flash {
            if blink {
                if let Some((x, y)) = self.cell_xy(loc, lay) {
                    let t = painter.tiles.get("explosion").cloned();
                    if let Some(t) = t {
                        let sx = if t.w >= 16 { 2 } else { 1 };
                        for ty in 0..8.min(t.h) {
                            for tx in 0..8 {
                                let p = t.px[ty * t.w + tx * sx];
                                if p != BLACK {
                                    // XOR in CGA terms: index bits flip.
                                    let under = mf.get(x + tx as i32, y + ty as i32);
                                    let cga = |c: u8| CGA1.iter().position(|&k| k == c).unwrap_or(0) as u8;
                                    mf.put(x + tx as i32, y + ty as i32, CGA1[((cga(under) ^ cga(p)) & 3) as usize]);
                                }
                            }
                        }
                    }
                }
            }
        }
            let u = lay.ui_scale;
        f.paste(mf, lay.map.x * u, lay.map.y * u, lay.map.w * u, lay.map.h * u);
    }

    /// Dashed "marching ants" line that inverts what it crosses.
    fn draw_ants(&self, f: &mut Frame, a: Loc, b: Loc, lay: &PlayLayout, phase: bool) {
        let g = self.game.geom();
        let to_px = |l: Loc| -> (i32, i32) {
            ((g.col(l) as i32 - self.origin.0 as i32) * TILE + 4, (g.row(l) as i32 - self.origin.1 as i32) * TILE + 4)
        };
        let area = Rect::new(0, 0, lay.cols as i32 * TILE, lay.rows as i32 * TILE);
        let (x0, y0) = to_px(a);
        let (x1, y1) = to_px(b);
        let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
        let shift = if phase { 0 } else { 2 };
        for i in 0..=steps {
            if (i + shift) % 4 >= 2 {
                continue;
            }
            let x = x0 + (x1 - x0) * i / steps;
            let y = y0 + (y1 - y0) * i / steps;
            if area.contains(x, y) {
                let p = f.get(x, y);
                f.put(x, y, if p == WHITE { BLACK } else { WHITE });
            }
        }
    }

    fn draw_link(&self, f: &mut Frame, a: Loc, b: Loc, lay: &PlayLayout, color: u8) {
        let g = self.game.geom();
        let to_px = |l: Loc| -> (i32, i32) {
            ((g.col(l) as i32 - self.origin.0 as i32) * TILE + 4, (g.row(l) as i32 - self.origin.1 as i32) * TILE + 4)
        };
        let area = Rect::new(0, 0, lay.cols as i32 * TILE, lay.rows as i32 * TILE);
        let (x0, y0) = to_px(a);
        let (x1, y1) = to_px(b);
        // Clip by plotting only inside the map rectangle.
        let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
        for i in 0..=steps {
            let x = x0 + (x1 - x0) * i / steps;
            let y = y0 + (y1 - y0) * i / steps;
            if area.contains(x, y) {
                f.put(x, y, color);
            }
        }
    }
}

/// Row numbers down the left strip and column numbers along the bottom.
pub fn draw_strip_numbers(f: &mut Frame, font: &Font, theme: &Theme, lay: &PlayLayout, origin: (usize, usize), size: (usize, usize)) {
    let digits = (size.0.max(size.1).saturating_sub(1)).to_string().len();
    let step = lay.label_step(digits);
    let row_step = lay.label_step(1).max(2).min(step);
    for i in 0..lay.rows {
        let r = origin.1 + i;
        if r.is_multiple_of(row_step) && r < size.1 {
            font.draw(f, lay.row_strip.x, lay.cell_pos(0, i).1, &r.to_string(), theme.strip_text, None);
        }
    }
    for i in 0..lay.cols {
        let c = origin.0 + i;
        if c.is_multiple_of(step) && c < size.0 {
            font.draw(f, lay.cell_pos(i, 0).0 + 1, lay.col_strip.y - 1, &c.to_string(), theme.strip_text, None);
        }
    }
}
