//! The application: title, desktop and playing screens, menus, dialogs, the
//! new-game sequence, saves and settings. The host (browser or CLI) feeds it
//! input and time, and reads back a framebuffer and a list of requests.

use crate::commands::{self, Cmd, CmdContext};
use crate::dialog::{Dialog, DlgEvent, DlgStyle, Elem};
use crate::editor::{EditEffect, EditorState};
use crate::gfx::*;
use crate::input::{Key, KeyEvent};
use crate::mapdraw::{CellArt, MapPainter};
use crate::layout::Zoom;
use crate::play::{Mode, PlayEffect, PlayState};
use crate::text::{small_font, Font};
use crate::theme::Theme;
use crate::tiles::TileSet;
use crate::ui::{self, ButtonLook, Menu};
use empire_core::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: &str = "0.1.0";

/// Logical screen sizes. The original ran at 320x200; larger sizes show more map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution {
    Original,
    Large,
    Huge,
    Fit,
}

impl Resolution {
    pub fn label(self) -> &'static str {
        match self {
            Resolution::Original => "320X200",
            Resolution::Large => "480X300",
            Resolution::Huge => "640X400",
            Resolution::Fit => "FIT",
        }
    }
    fn next(self) -> Self {
        match self {
            Resolution::Original => Resolution::Large,
            Resolution::Large => Resolution::Huge,
            Resolution::Huge => Resolution::Fit,
            Resolution::Fit => Resolution::Original,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub resolution: Resolution,
    pub sound: bool,
    /// 0..=100, as in the original's Display dialog.
    pub message_delay: u8,
    pub battle_delay: u8,
    pub report_armies_fighters: bool,
    /// 0 = built-in icons, 1..=4 = imported original CGA icon set.
    pub icon_set: u8,
    pub original_font: bool,
    pub map_width: usize,
    pub map_height: usize,
    pub land_percent: u8,
    /// Stretch pixels to 1.2:1 like a CGA monitor (4:3 picture at 320x200).
    pub cga_aspect: bool,
    /// Draw at the screen's real resolution with smooth text and icons.
    /// Off gives the authentic CGA picture, pixel for pixel.
    pub hd: bool,
    /// High-resolution text size: index into `TEXT_SIZES`.
    pub text_size: u8,
    /// High-resolution mode uses the smooth font (else the pixel font, enlarged).
    pub smooth_text: bool,
    /// High-resolution mode rounds off icon edges.
    pub smooth_icons: bool,
    /// Map squares in CSS pixels (high-resolution look); 0 follows the text size.
    pub map_square: u8,
}

/// CSS pixels per layout pixel for each text size. At "Medium" capitals are
/// about 12 CSS pixels tall.
pub const TEXT_SIZES: [(f64, &str); 4] = [(2.0, "SMALL"), (2.5, "MEDIUM"), (3.0, "LARGE"), (3.5, "HUGE")];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            resolution: Resolution::Original,
            sound: true,
            message_delay: 30,
            battle_delay: 50,
            report_armies_fighters: true,
            icon_set: 1,
            original_font: true,
            map_width: 100,
            map_height: 60,
            land_percent: 27,
            cga_aspect: true,
            hd: true,
            text_size: 1,
            smooth_text: true,
            smooth_icons: true,
            map_square: 0,
        }
    }
}

/// Map square sizes offered by the Display dialog, in CSS pixels (0 = follow the text size).
pub const MAP_SQUARES: [u8; 8] = [0, 8, 12, 16, 20, 24, 32, 40];

/// Something the host should do.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HostRequest {
    /// Persist (or remove, if `value` is null) a key in local storage.
    Store { key: String, value: Option<String> },
    Sound { name: String },
    /// Offer a file for download.
    Download { filename: String, text: String },
    /// Ask the user to pick a file; reply with `file_loaded`.
    Upload { accept: String },
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PType {
    Human,
    Standard,
    Expert,
    Nobody,
}

#[derive(Clone, Debug)]
struct SetupPlayer {
    kind: PType,
    name: String,
    production: u8,
    combat: u8,
}

#[derive(Clone, Debug)]
struct Setup {
    kinds: Vec<PType>,
    players: Vec<SetupPlayer>,
    game: Option<Game>,
    name_idx: usize,
}

#[derive(Clone, Debug)]
enum Dk {
    Alert,
    Opponents,
    Options { sel: usize },
    WorldMapChoice,
    MapSelect { names: Vec<String> },
    Name { idx: usize },
    Report { idx: usize },
    Production { city: usize },
    UnitProduced { city: usize, unit: UnitId },
    Info,
    WarReport,
    ShipReport,
    CombatReport,
    WorldMap,
    Display { s: Settings },
    ConfirmRisky { unit: UnitId, dir: Dir },
    ConfirmExit,
    ConfirmEndTurn,
    SaveAs,
    LoadList { keys: Vec<String> },
    DeleteList { keys: Vec<String> },
    HandOff,
    Surrender,
    GameOver,
    Commanders,
    About,
    View,
    EdLoadList { names: Vec<String> },
    EdSaveAs,
    EdValidate,
    EdWorldView,
    EdSprinkle,
    EdExit,
}

const B_OK: u32 = 1;
const B_CANCEL: u32 = 2;
const B_PROD: u32 = 3;
const B_RANDOM: u32 = 4;
const B_SELECT: u32 = 5;
const B_DOWNLOAD: u32 = 6;
const B_UPLOAD: u32 = 7;
const B_ACCEPT: u32 = 8;
const B_REFUSE: u32 = 9;
const B_SIZE: u32 = 10;
const B_LAND: u32 = 11;
const B_RES: u32 = 12;
const B_ICONS: u32 = 13;
const B_FONT: u32 = 14;
const B_REPORT_ON: u32 = 15;
const B_REPORT_OFF: u32 = 16;
const B_WORLD: u32 = 17;
const B_CONTINUE: u32 = 18;
const B_ASPECT: u32 = 19;
const B_LOOK: u32 = 20;
const B_TEXT: u32 = 21;
const B_SMOOTH: u32 = 22;
const B_ZOOM: u32 = 23;
/// Opponent grid: 100 + player*10 + kind.
const B_OPP: u32 = 100;
/// View: 200 + player.
const B_VIEWP: u32 = 200;
const L_MAIN: u32 = 1;
const S_MSG: u32 = 1;
const S_BATTLE: u32 = 2;
const S_PROD: u32 = 3;
const S_COMBAT: u32 = 4;

const MAP_SIZES: [(usize, usize); 6] = [(60, 36), (80, 48), (100, 60), (140, 84), (200, 120), (300, 180)];

#[derive(Clone, Debug)]
struct Image {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

enum Screen {
    Title,
    Desktop,
    Play(Box<PlayState>),
    Editor(Box<EditorState>),
}

pub struct App {
    pub frame: Frame,
    /// The map is drawn here at its own scale, then copied into `frame`.
    map_frame: Frame,
    /// Current text and map scale.
    zoom: Zoom,
    theme: Theme,
    font: Font,
    font_builtin: Font,
    font_original: Option<Font>,
    tiles: TileSet,
    tiles_builtin: TileSet,
    tiles_original: [Option<TileSet>; 4],
    title_image: Option<Image>,
    /// Maps available for "Select": (name, .emap text).
    maps: Vec<(String, String)>,
    pub settings: Settings,
    storage: BTreeMap<String, String>,
    requests: Vec<HostRequest>,
    screen: Screen,
    dialogs: Vec<Dialog<Dk>>,
    menu: Option<(usize, Option<usize>)>,
    setup: Option<Setup>,
    now: f64,
    last_tick: f64,
    blink: bool,
    blink_at: f64,
    /// Window size in CSS pixels and the device pixel ratio.
    viewport: (u32, u32),
    dpr: f64,
    /// Display size of the canvas in CSS pixels.
    display: (f64, f64),
    font_smooth: Font,
    dirty: bool,
    last_sig: u64,
    desktop_message: String,
    seed_counter: u64,
    last_click: (f64, i32, i32),
}

fn rank_planet(seed: u64) -> String {
    const P: [&str; 12] = [
        "ARABELLAN", "UBAIDIAN", "KRELLAN", "VEGAN", "ALTAIRAN", "RIGELLIAN", "DENEBIAN", "ORIONIAN", "SIRIAN", "CAPELLAN",
        "ANTAREAN", "POLLUXIAN",
    ];
    P[(seed % P.len() as u64) as usize].to_string()
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> App {
        let font = small_font();
        let tiles = TileSet::builtin();
        let mut app = App {
            frame: Frame::new(320, 200),
            map_frame: Frame::new(8, 8),
            zoom: Zoom::default(),
            theme: Theme::cga(),
            font: font.clone(),
            font_builtin: font,
            font_original: None,
            tiles: tiles.clone(),
            tiles_builtin: tiles,
            tiles_original: [None, None, None, None],
            title_image: None,
            maps: Vec::new(),
            settings: Settings::default(),
            storage: BTreeMap::new(),
            requests: Vec::new(),
            screen: Screen::Title,
            dialogs: Vec::new(),
            menu: None,
            setup: None,
            now: 0.0,
            last_tick: 0.0,
            blink: true,
            blink_at: 0.0,
            viewport: (960, 600),
            dpr: 1.0,
            display: (960.0, 600.0),
            font_smooth: crate::text::smooth_font(),
            dirty: true,
            last_sig: 0,
            desktop_message: "WELCOME TO EMPIRE".into(),
            seed_counter: 0,
            last_click: (0.0, -1, -1),
        };
        app.apply_settings();
        app
    }

    // ------------------------------------------------------------------
    // Host interface

    pub fn width(&self) -> usize {
        self.frame.width
    }

    /// Vertical stretch the host should apply to pixels (1.2 for a CGA monitor).
    pub fn aspect(&self) -> f64 {
        if self.settings.cga_aspect && !self.settings.hd {
            1.2
        } else {
            1.0
        }
    }
    pub fn height(&self) -> usize {
        self.frame.height
    }

    /// Tell the app the browser window size in CSS pixels and the device pixel ratio.
    pub fn set_viewport(&mut self, w: u32, h: u32, dpr: f64) {
        self.viewport = (w.max(320), h.max(200));
        self.dpr = if dpr.is_finite() && dpr > 0.0 { dpr } else { 1.0 };
        self.apply_resolution();
    }

    /// Canvas buffer size in device pixels.
    pub fn pixel_width(&self) -> usize {
        self.frame.device_width()
    }
    pub fn pixel_height(&self) -> usize {
        self.frame.device_height()
    }

    /// How big the host should show the canvas, in CSS pixels.
    pub fn display_size(&self) -> (f64, f64) {
        self.display
    }

    /// Device pixels per map-picture pixel. The CGA look keeps the original
    /// 8-pixel squares; the high-resolution look follows the setting.
    fn map_scale(&self, ui_scale: usize) -> i32 {
        if !self.settings.hd {
            return 1;
        }
        match self.settings.map_square {
            0 => ui_scale as i32,
            css => ((css as f64 * self.dpr / 8.0).round() as i32).max(1),
        }
    }

    /// Zoom the map in (+1) or out (-1) one step.
    fn zoom_map(&mut self, dir: i32) {
        if !self.settings.hd {
            self.alert("MAP ZOOM NEEDS THE HIGH-RES LOOK (MISC > DISPLAY)");
            return;
        }
        let cur = self.zoom.map.max(1);
        let step = (cur / 4).max(1);
        let next = (cur + dir * step).clamp(1, (8.0 * self.dpr).round() as i32);
        let css = (next as f64 * 8.0 / self.dpr).round().clamp(4.0, 64.0) as u8;
        self.settings.map_square = css;
        self.save_settings();
        self.apply_resolution();
    }

    fn apply_resolution(&mut self) {
        let (vw, vh) = (self.viewport.0 as f64, self.viewport.1 as f64);
        let dpr = self.dpr;
        let (w, h, scale) = if self.settings.hd {
            // Same layout grid, drawn at the monitor's real resolution.
            let css_per_unit = TEXT_SIZES[(self.settings.text_size as usize).min(TEXT_SIZES.len() - 1)].0;
            let mut scale = (css_per_unit * dpr).round().max(1.0);
            // A small window shrinks the text so the whole 320x200 layout still fits.
            let fit = (vw / 320.0).min(vh / 200.0);
            if scale / dpr > fit {
                scale = (fit * dpr).floor().max(1.0);
            }
            let unit = scale / dpr;
            let w = ((vw / unit).floor() as usize).clamp(320, 2000);
            let h = ((vh / unit).floor() as usize).clamp(200, 1200);
            (w, h, scale as usize)
        } else {
            let (w, h) = match self.settings.resolution {
                Resolution::Original => (320, 200),
                Resolution::Large => (480, 300),
                Resolution::Huge => (640, 400),
                Resolution::Fit => {
                    let s = (((vw as u32) / 320).min((vh as u32) / 200)).max(2) - 1;
                    (((vw as u32) / s.max(1)).clamp(320, 1600) as usize, ((vh as u32) / s.max(1)).clamp(200, 1000) as usize)
                }
            };
            (w, h, 1)
        };
        self.frame.resize(w, h, scale);
        self.display = if self.settings.hd {
            (self.frame.device_width() as f64 / dpr, self.frame.device_height() as f64 / dpr)
        } else {
            // Largest whole-number enlargement that fits, with the CGA pixel shape.
            let aspect = self.aspect();
            let sx = ((vw * dpr) / w as f64).floor();
            let sy = ((vh * dpr) / (h as f64 * aspect)).floor();
            let s = sx.min(sy).max(1.0) / dpr;
            (w as f64 * s, h as f64 * s * aspect)
        };
        self.zoom = Zoom { ui: scale as i32, map: self.map_scale(scale) };
        // Re-centre the map for the new layout.
        if let Screen::Play(ps) = &mut self.screen {
            ps.zoom = self.zoom;
            let lay = ps.layout(w as i32, h as i32);
            let c = ps.cursor;
            ps.center_on(c, &lay);
        }
        if let Screen::Editor(es) = &mut self.screen {
            es.zoom = self.zoom;
            let lay = es.layout(w as i32, h as i32);
            let c = es.cursor;
            es.center_on(c, &lay);
        }
        for d in &mut self.dialogs {
            // Keep dialogs on screen.
            d.rect.x = d.rect.x.min(w as i32 - d.rect.w - 2).max(0);
        }
        self.dirty = true;
    }

    pub fn take_requests(&mut self) -> Vec<HostRequest> {
        std::mem::take(&mut self.requests)
    }

    pub fn take_requests_json(&mut self) -> String {
        serde_json::to_string(&self.take_requests()).unwrap_or_else(|_| "[]".into())
    }

    /// The host passes stored values at startup (and whenever it reads one).
    pub fn storage_value(&mut self, key: &str, value: &str) {
        self.storage.insert(key.to_string(), value.to_string());
        if key == "empire.settings" {
            if let Ok(s) = serde_json::from_str::<Settings>(value) {
                self.settings = s;
                self.apply_settings();
            }
        }
    }

    fn store(&mut self, key: &str, value: Option<String>) {
        match &value {
            Some(v) => {
                self.storage.insert(key.to_string(), v.clone());
            }
            None => {
                self.storage.remove(key);
            }
        }
        self.requests.push(HostRequest::Store { key: key.to_string(), value });
    }

    fn sound(&mut self, name: &str) {
        if self.settings.sound {
            self.requests.push(HostRequest::Sound { name: name.into() });
        }
    }

    /// Optional assets converted from an original disk (see tools/interstel_import).
    pub fn load_asset_text(&mut self, name: &str, text: &str) -> Result<(), String> {
        self.dirty = true;
        let base = name.rsplit('/').next().unwrap_or(name);
        if let Some(n) = base.strip_prefix("tiles_cga").and_then(|r| r.strip_suffix(".tiles")) {
            let idx: usize = n.parse().map_err(|_| "bad tile set name")?;
            if (1..=4).contains(&idx) {
                let mut set = TileSet::parse(text)?;
                set.recolor_units = false;
                self.tiles_original[idx - 1] = Some(set);
                self.apply_settings();
            }
        } else if base == "font_cga.font" {
            let mut f = Font::parse(text)?;
            f.advance = 4;
            // The original font has no space glyph.
            f.glyphs[' ' as usize] = Some(vec![0; f.cell_h as usize]);
            self.font_original = Some(f);
            self.apply_settings();
        } else if let Some(map) = base.strip_suffix(".emap") {
            WorldMap::parse(text)?;
            if !self.maps.iter().any(|(n, _)| n == map) {
                self.maps.push((map.to_string(), text.to_string()));
                self.maps.sort();
            }
        }
        Ok(())
    }

    /// Binary assets: `.img` images ("EIMG", u16 w, u16 h, one CGA index per pixel).
    pub fn load_asset_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        self.dirty = true;
        let base = name.rsplit('/').next().unwrap_or(name);
        if base == "title2.img" {
            if bytes.len() < 8 || &bytes[0..4] != b"EIMG" {
                return Err("not an EIMG image".into());
            }
            let w = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
            let h = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
            if bytes.len() < 8 + w * h {
                return Err("image truncated".into());
            }
            let px = bytes[8..8 + w * h].iter().map(|&b| CGA1[(b & 3) as usize]).collect();
            self.title_image = Some(Image { w, h, px });
        }
        Ok(())
    }

    /// A file the user picked after an `Upload` request.
    pub fn file_loaded(&mut self, name: &str, text: &str) {
        self.dirty = true;
        if let Ok(g) = Game::from_save(text) {
            self.start_play(g, None);
            self.desktop_message = format!("LOADED {}", name.to_uppercase());
            return;
        }
        match WorldMap::parse(text) {
            Ok(m) if matches!(self.screen, Screen::Editor(_)) => {
                let base = name.trim_end_matches(".emap").trim_end_matches(".txt").to_uppercase();
                let (w, h) = (self.frame.width as i32, self.frame.height as i32);
                let params = MapGenParams {
                    width: m.geom.width,
                    height: m.geom.height,
                    land_fraction: self.settings.land_percent as f64 / 100.0,
                    ..Default::default()
                };
                let mut es = EditorState::from_map(m, Some(base), params, self.now as u64 + 3);
                es.dirty = true;
                es.zoom = self.zoom;
                let lay = es.layout(w, h);
                let c = es.cursor;
                es.center_on(c, &lay);
                self.screen = Screen::Editor(Box::new(es));
            }
            Ok(m) => {
                let base = name.trim_end_matches(".emap").trim_end_matches(".txt").to_uppercase();
                self.store(&format!("empire.map.{base}"), Some(text.to_string()));
                let n = m.city_locs().len();
                self.alert(&format!("MAP {base} ADDED: {}X{} WITH {n} CITIES.", m.geom.width, m.geom.height));
                if let Some(Dk::MapSelect { .. }) = self.dialogs.last().map(|d| d.kind.clone()) {
                    self.dialogs.pop();
                    self.open_map_select();
                }
            }
            Err(e) => self.alert(&format!("COULDN'T READ {}: {e}", name.to_uppercase())),
        }
    }

    fn apply_settings(&mut self) {
        let mut tiles = self.tiles_builtin.clone();
        let set = self.settings.icon_set as usize;
        if (1..=4).contains(&set) {
            if let Some(t) = &self.tiles_original[set - 1] {
                tiles.overlay(t.clone());
            }
        }
        self.tiles = tiles;
        self.font = if self.settings.hd && self.settings.smooth_text {
            self.font_smooth.clone()
        } else {
            match (&self.font_original, self.settings.original_font) {
                (Some(f), true) => f.clone(),
                _ => self.font_builtin.clone(),
            }
        };
        self.dirty = true;
        if let Screen::Play(ps) = &mut self.screen {
            ps.message_delay = self.settings.message_delay as f64 * 30.0;
            ps.battle_delay = self.settings.battle_delay as f64 * 30.0;
            ps.report_produced = self.settings.report_armies_fighters;
        }
        self.apply_resolution();
    }

    fn save_settings(&mut self) {
        let s = serde_json::to_string(&self.settings).unwrap_or_default();
        self.store("empire.settings", Some(s));
    }

    /// Switch between high-resolution drawing and the authentic CGA picture.
    pub fn set_hd(&mut self, hd: bool) {
        self.settings.hd = hd;
        self.apply_settings();
    }

    pub fn has_original_icons(&self) -> bool {
        self.tiles_original.iter().any(|t| t.is_some())
    }

    // ------------------------------------------------------------------
    // Time

    /// Advance time. Returns true if the screen should be redrawn.
    pub fn tick(&mut self, now_ms: f64) -> bool {
        let dt = if self.last_tick == 0.0 { 16.0 } else { (now_ms - self.last_tick).clamp(0.0, 250.0) };
        self.last_tick = now_ms;
        self.now = now_ms;
        if now_ms - self.blink_at >= 550.0 {
            self.blink = !self.blink;
            self.blink_at = now_ms;
            self.dirty = true;
        }
        if self.dialogs.is_empty() && self.menu.is_none() {
            let (w, h) = (self.frame.width as i32, self.frame.height as i32);
            if let Screen::Editor(es) = &mut self.screen {
                let lay = es.layout(w, h);
                if es.growing() {
                    self.dirty = true;
                }
                es.tick(&lay);
            }
            if let Screen::Play(ps) = &mut self.screen {
                let lay = ps.layout(w, h);
                let fx = ps.tick(dt, &lay);
                if !fx.is_empty() {
                    self.dirty = true;
                }
                self.apply_effects(fx);
            }
        }
        // Redraw when anything visible changed.
        if let Screen::Play(ps) = &self.screen {
            let sig = ps.signature();
            if sig != self.last_sig {
                self.last_sig = sig;
                self.dirty = true;
            }
        }
        self.dirty
    }

    /// Mark the screen as needing a redraw.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    // ------------------------------------------------------------------
    // Rendering

    pub fn render(&mut self) -> &[u8] {
        self.draw();
        self.dirty = false;
        self.frame.to_rgba()
    }

    pub fn draw(&mut self) {
        let theme = self.theme.clone();
        let font = self.font.clone();
        self.frame.clear(theme.bg);
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let smooth = self.settings.hd && self.settings.smooth_icons;
        let painter = MapPainter { tiles: &self.tiles, theme: &theme, smooth };
        match &self.screen {
            Screen::Title => {
                Self::draw_title(&mut self.frame, &font, &theme, self.title_image.as_ref(), &painter);
                return;
            }
            Screen::Desktop => {
                font.draw(&mut self.frame, 4, 13, &self.desktop_message, theme.text, None);
                self.frame.hline(0, w - 1, 22, theme.rule);
            }
            Screen::Play(ps) => {
                // Hot-seat: the screen stays blank until the next player reports.
                let handing_off = matches!(self.dialogs.last().map(|d| &d.kind), Some(Dk::HandOff));
                if !handing_off {
                    let lay = ps.layout(w, h);
                    ps.draw(&mut self.frame, &mut self.map_frame, &font, &theme, &painter, &lay, self.blink);
                }
            }
            Screen::Editor(es) => {
                let lay = es.layout(w, h);
                es.draw(&mut self.frame, &mut self.map_frame, &font, &theme, &painter, &lay, self.blink);
            }
        }
        // Menu bar with mode and turn.
        let menus = self.menus();
        ui::draw_menu_bar(&mut self.frame, &font, &theme, &menus, self.menu.map(|m| m.0));
        self.frame.hline(0, w - 1, 10, theme.rule);
        if let Screen::Play(ps) = &self.screen {
            // MODE at x=200 and TURN at x=276 on a 320-wide screen.
            if ps.mode == Mode::Computer {
                font.draw(&mut self.frame, w - 120 + 24, 1, "COMPUTER", theme.text, None);
            } else {
                font.draw(&mut self.frame, w - 120, 1, &format!("MODE: {}", ps.mode.label()), theme.text, None);
            }
            font.draw(&mut self.frame, w - 44, 1, &format!("TURN: {}", ps.game.turn), theme.text, None);
        } else if matches!(self.screen, Screen::Desktop) {
            font.draw(&mut self.frame, w - 120, 1, "MODE:", theme.text, None);
        } else if matches!(self.screen, Screen::Editor(_)) {
            font.draw(&mut self.frame, w - 120, 1, "MODE: MAP EDITOR", theme.text, None);
        }
        if let Some((m, sel)) = self.menu {
            ui::draw_dropdown(&mut self.frame, &font, &theme, &menus, m, sel);
        }
        for d in &self.dialogs {
            d.draw(&mut self.frame, &font, &theme, &painter, self.blink);
            for e in &d.elems {
                if let Elem::Custom { r, .. } = e {
                    match &self.screen {
                        Screen::Play(ps) => Self::draw_world_map(&mut self.frame, ps, *r, &theme),
                        Screen::Editor(es) => Self::draw_editor_world(&mut self.frame, es, *r),
                        _ => {}
                    }
                }
            }
        }
    }

    fn draw_title(f: &mut Frame, font: &Font, theme: &Theme, img: Option<&Image>, painter: &MapPainter) {
        let (w, h) = (f.width as i32, f.height as i32);
        if let Some(img) = img {
            let ox = (w - img.w as i32) / 2;
            let oy = (h - img.h as i32) / 2;
            for y in 0..img.h {
                for x in 0..img.w {
                    f.put(ox + x as i32, oy + y as i32, img.px[y * img.w + x]);
                }
            }
            return;
        }
        // Built-in title card.
        let cx = w / 2;
        let big = "EMPIRE";
        let scale = 4;
        let bw = big.len() as i32 * 8 * scale;
        let bx = cx - bw / 2;
        let by = h / 2 - 60;
        for (i, ch) in big.chars().enumerate() {
            let glyph = crate::font::FONT8X8[ch as usize];
            for (ry, bits) in glyph.iter().enumerate() {
                for rx in 0..8 {
                    if bits & (1 << rx) != 0 {
                        let r = Rect::new(bx + (i as i32 * 8 + rx) * scale, by + ry as i32 * scale, scale, scale);
                        f.fill(r, if ry < 4 { CYAN } else { MAGENTA });
                    }
                }
            }
        }
        let lines = [
            ("A TRIBUTE TO THE 1987 CLASSIC", theme.text),
            ("", theme.text),
            ("ORIGINAL GAME BY WALTER BRIGHT AND MARK BALDWIN", CYAN),
            ("NOT AFFILIATED WITH THEM OR WITH INTERSTEL", CYAN),
            ("", theme.text),
            ("PRESS ANY KEY", MAGENTA),
        ];
        let mut y = by + 8 * scale + 8;
        for (s, c) in lines {
            let x = cx - font.width(s) / 2;
            font.draw(f, x, y, s, c, None);
            y += 10;
        }
        // A row of the unit icons along the bottom.
        let kinds = UNIT_TYPES;
        let total = kinds.len() as i32 * 12;
        let mut x = cx - total / 2;
        for (i, k) in kinds.iter().enumerate() {
            let owner = (i % 3) as u8 + 1;
            painter.draw_art(f, x, h - 24, CellArt::Unit { kind: *k, owner, over_sea: false, sentry: false, loaded: false });
            x += 12;
        }
    }

    fn draw_editor_world(f: &mut Frame, es: &EditorState, r: Rect) {
        let geom = es.map.geom;
        f.fill(r, BLACK);
        let s = ((r.w / geom.width as i32).min(r.h / geom.height as i32)).max(1);
        let ox = r.x + (r.w - s * geom.width as i32) / 2;
        let oy = r.y + (r.h - s * geom.height as i32) / 2;
        for loc in 0..geom.size() {
            let c = match es.map.get(loc) {
                Terrain::Sea => CYAN,
                Terrain::Land => MAGENTA,
                Terrain::City => WHITE,
            };
            f.fill(Rect::new(ox + geom.col(loc) as i32 * s, oy + geom.row(loc) as i32 * s, s, s), c);
        }
    }

    fn draw_world_map(f: &mut Frame, ps: &PlayState, r: Rect, theme: &Theme) {
        let g = &ps.game;
        let geom = g.geom();
        f.fill(r, BLACK);
        let s = ((r.w / geom.width as i32).min(r.h / geom.height as i32)).max(1);
        let ox = r.x + (r.w - s * geom.width as i32) / 2;
        let oy = r.y + (r.h - s * geom.height as i32) / 2;
        for loc in 0..geom.size() {
            let v = if ps.reveal_all { g.truth(loc) } else { g.player(ps.viewer).view[loc] };
            let _ = theme;
            let c = if v.is_unknown() {
                continue;
            } else if v.is_city() || v.is_unit() {
                WHITE
            } else if v.is_sea() {
                CYAN
            } else {
                MAGENTA
            };
            let x = ox + geom.col(loc) as i32 * s;
            let y = oy + geom.row(loc) as i32 * s;
            if s == 1 && v.is_city() {
                f.put(x, y, c);
            } else {
                f.fill(Rect::new(x, y, s, s), c);
            }

        }
        // Frame the visible part of the map.
        let lay = ps.layout(f.width as i32, f.height as i32);
        let vx = ox + ps.origin.0 as i32 * s;
        let vy = oy + ps.origin.1 as i32 * s;
        let vw = (lay.cols.min(geom.width) as i32) * s;
        let vh = (lay.rows.min(geom.height) as i32) * s;
        f.frame(Rect::new(vx, vy, vw, vh), WHITE);
    }

    // ------------------------------------------------------------------
    // Input

    fn menus(&self) -> Vec<Menu<Cmd>> {
        let ctx = self.cmd_context();
        if matches!(self.screen, Screen::Editor(_)) {
            commands::editor_menus(&ctx)
        } else {
            commands::menus(&ctx)
        }
    }

    fn cmd_context(&self) -> CmdContext {
        let mut c = CmdContext { sound: self.settings.sound, ..Default::default() };
        if let Screen::Editor(es) = &self.screen {
            c.editor = true;
            c.watch_build = es.watch;
        }
        if let Screen::Play(ps) = &self.screen {
            c.in_game = true;
            c.game_over = ps.game.game_over;
            c.viewer_human = ps.game.player(ps.viewer).is_human() && ps.game.current == ps.viewer;
            c.survey = !matches!(ps.mode, Mode::Move);
            if let Some(u) = ps.target_unit() {
                let k = ps.game.unit(u).kind;
                c.has_unit = true;
                c.unit_is_fighter = k == UnitType::Fighter;
                c.unit_is_ship = k.is_ship();
                c.unit_is_army = k == UnitType::Army;
                c.unit_carries = k.cargo().is_some();
            }
            c.on_own_city = ps.target_city().is_some();
            c.can_move_unit = matches!(ps.pending, Some(Pending::UnitOrders { .. }));
        }
        c
    }

    pub fn key_down(&mut self, ev: KeyEvent) -> bool {
        self.dirty = true;
        if let Screen::Title = self.screen {
            self.screen = Screen::Desktop;
            return true;
        }
        // Open menus take the keyboard.
        if let Some((m, sel)) = self.menu {
            self.menu_key(m, sel, ev);
            return true;
        }
        if let Some(mut d) = self.dialogs.pop() {
            let before = self.dialogs.len();
            let event = d.key(ev);
            let keep = match event {
                Some(e) => self.dialog_event(&mut d, e),
                None => true,
            };
            if keep {
                self.dialogs.insert(before, d);
            }
            return true;
        }
        // Ctrl + menu letter drops a menu.
        if ev.ctrl && !ev.alt {
            if let Key::Char(c) = ev.key {
                let menus = self.menus();
                if let Some(i) = menus.iter().position(|m| ui::hot_char(m.title) == Some(c.to_ascii_uppercase())) {
                    let editor = matches!(self.screen, Screen::Editor(_));
                    let taken = if editor { commands::editor_shortcut(&ev).is_some() } else { commands::shortcut(&ev).is_some() };
                    if !taken || "ROMFPG".contains(c.to_ascii_uppercase()) {
                        self.menu = Some((i, first_enabled(&menus[i])));
                        return true;
                    }
                }
            }
        }
        if ev.key == Key::F(10) && ev.alt {
            self.menu = Some((0, None));
            return true;
        }
        if let Screen::Editor(_) = self.screen {
            if let Some(z @ (Cmd::ZoomIn | Cmd::ZoomOut)) = commands::editor_shortcut(&ev) {
                self.run_command(z);
                return true;
            }
        }
        if let Screen::Editor(es) = &mut self.screen {
            let lay = es.layout(self.frame.width as i32, self.frame.height as i32);
            let fx = es.key(ev, &lay);
            self.apply_edit_effects(fx);
            return true;
        }
        if let Some(cmd) = commands::shortcut(&ev) {
            if self.global_command(cmd) {
                return true;
            }
        }
        if let Screen::Play(ps) = &mut self.screen {
            let lay = ps.layout(self.frame.width as i32, self.frame.height as i32);
            let fx = ps.key(ev, &lay);
            self.apply_effects(fx);
            return true;
        }
        if matches!(self.screen, Screen::Desktop) && matches!(ev.key, Key::Enter | Key::Char('n') | Key::Char('N')) {
            self.run_command(Cmd::NewGame);
        }
        true
    }

    fn menu_key(&mut self, m: usize, sel: Option<usize>, ev: KeyEvent) {
        let menus = self.menus();
        let items = &menus[m].items;
        let step = |from: Option<usize>, dir: i32| -> Option<usize> {
            let n = items.len() as i32;
            let mut i = from.map(|i| i as i32).unwrap_or(if dir > 0 { -1 } else { n });
            for _ in 0..n {
                i = (i + dir).rem_euclid(n);
                if !items[i as usize].is_divider() && items[i as usize].enabled {
                    return Some(i as usize);
                }
            }
            from
        };
        match ev.key {
            Key::Esc => self.menu = None,
            Key::Left | Key::Pad(4) => {
                let nm = (m + menus.len() - 1) % menus.len();
                self.menu = Some((nm, first_enabled(&menus[nm])));
            }
            Key::Right | Key::Pad(6) => {
                let nm = (m + 1) % menus.len();
                self.menu = Some((nm, first_enabled(&menus[nm])));
            }
            Key::Up | Key::Pad(8) => self.menu = Some((m, step(sel, -1))),
            Key::Down | Key::Pad(2) => self.menu = Some((m, step(sel, 1))),
            Key::Enter => {
                if let Some(i) = sel {
                    if let (true, Some(c)) = (items[i].enabled, items[i].command) {
                        self.menu = None;
                        self.run_command(c);
                    }
                }
            }
            _ => {
                if let Some(letter) = ev.letter() {
                    if let Some(it) = items.iter().find(|it| it.enabled && ui::hot_char(it.label) == Some(letter)) {
                        if let Some(c) = it.command {
                            self.menu = None;
                            self.run_command(c);
                        }
                    }
                } else {
                    self.menu = None;
                }
            }
        }
    }

    pub fn mouse_down(&mut self, x: i32, y: i32, button: u8) {
        self.dirty = true;
        if let Screen::Title = self.screen {
            self.screen = Screen::Desktop;
            return;
        }
        let double = self.now - self.last_click.0 < 400.0 && (self.last_click.1 - x).abs() < 3 && (self.last_click.2 - y).abs() < 3;
        self.last_click = (self.now, x, y);
        let font = self.font.clone();
        let menus = self.menus();
        // Menu bar and drop-downs.
        if let Some((m, _)) = self.menu {
            let r = ui::dropdown_rect(&font, &menus, m);
            if r.contains(x, y) {
                let ys = ui::dropdown_item_ys(&menus[m], r.y);
                for (i, iy) in ys.iter().enumerate() {
                    let it = &menus[m].items[i];
                    if !it.is_divider() && y >= *iy && y < iy + 8 {
                        if let (true, Some(c)) = (it.enabled, it.command) {
                            self.menu = None;
                            self.run_command(c);
                        }
                        return;
                    }
                }
                return;
            }
            self.menu = None;
            if y >= 10 {
                return;
            }
        }
        if y < 10 && self.dialogs.is_empty() {
            let rects = ui::menu_title_rects(&font, &menus);
            if let Some(i) = rects.iter().position(|r| r.contains(x, y)) {
                self.menu = Some((i, first_enabled(&menus[i])));
            }
            return;
        }
        if let Some(mut d) = self.dialogs.pop() {
            let before = self.dialogs.len();
            let event = d.click(x, y, double);
            let keep = match event {
                Some(e) => self.dialog_event(&mut d, e),
                None => true,
            };
            if keep {
                self.dialogs.insert(before, d);
            }
            return;
        }
        if let Screen::Editor(es) = &mut self.screen {
            let lay = es.layout(self.frame.width as i32, self.frame.height as i32);
            es.click(x, y, &lay);
            return;
        }
        if let Screen::Play(ps) = &mut self.screen {
            let lay = ps.layout(self.frame.width as i32, self.frame.height as i32);
            let fx = ps.click(x, y, button == 2, self.now, &lay);
            self.apply_effects(fx);
        }
    }

    pub fn mouse_move(&mut self, x: i32, y: i32) {
        if let Some((m, _)) = self.menu {
            self.dirty = true;
            let menus = self.menus();
            let font = &self.font;
            if y < 10 {
                let rects = ui::menu_title_rects(font, &menus);
                if let Some(i) = rects.iter().position(|r| r.contains(x, y)) {
                    if i != m {
                        self.menu = Some((i, first_enabled(&menus[i])));
                    }
                }
                return;
            }
            let r = ui::dropdown_rect(font, &menus, m);
            if r.contains(x, y) {
                let ys = ui::dropdown_item_ys(&menus[m], r.y);
                let sel = ys.iter().position(|iy| y >= *iy && y < iy + 8).filter(|&i| !menus[m].items[i].is_divider());
                self.menu = Some((m, sel));
            }
        }
    }

    pub fn wheel(&mut self, dx: i32, dy: i32) {
        self.dirty = true;
        if !self.dialogs.is_empty() {
            return;
        }
        if let Screen::Editor(es) = &mut self.screen {
            let lay = es.layout(self.frame.width as i32, self.frame.height as i32);
            es.scroll(dx.signum() as i64 * 2, dy.signum() as i64 * 2, &lay);
            return;
        }
        if let Screen::Play(ps) = &mut self.screen {
            let lay = ps.layout(self.frame.width as i32, self.frame.height as i32);
            ps.scroll(dx.signum() as i64 * 2, dy.signum() as i64 * 2, &lay);
        }
    }

    // ------------------------------------------------------------------
    // Commands

    /// Commands handled at the app level. Returns true if handled.
    fn global_command(&mut self, cmd: Cmd) -> bool {
        let ctx = self.cmd_context();
        use Cmd::*;
        match cmd {
            NewGame | Load | Save | SaveAs | Delete | ExitGame | MapEditor | Quit | Sound | Display | ZoomIn | ZoomOut
            | Commanders | About | View => {
                if ctx.enabled(cmd) {
                    self.run_command(cmd);
                } else {
                    self.sound("error");
                }
                true
            }
            _ => false,
        }
    }

    fn run_command(&mut self, cmd: Cmd) {
        use Cmd::*;
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        match cmd {
            ZoomIn => return self.zoom_map(1),
            ZoomOut => return self.zoom_map(-1),
            _ => {}
        }
        if let Screen::Editor(es) = &mut self.screen {
            match cmd {
                Delete => self.open_delete(),
                About => self.open_about(),
                Sound | Display | Commanders | Quit => {}
                other => {
                    let lay = es.layout(w, h);
                    let fx = es.command(other, &lay);
                    self.apply_edit_effects(fx);
                }
            }
            return;
        }
        match cmd {
            NewGame => self.begin_new_game(),
            Load => self.open_load(),
            Save => {
                let name = match &self.screen {
                    Screen::Play(ps) => ps.save_name.clone(),
                    _ => None,
                };
                match name {
                    Some(n) => self.save_game(&n),
                    None => self.open_save_as(),
                }
            }
            SaveAs => self.open_save_as(),
            Delete => self.open_delete(),
            ExitGame => {
                let mut d = Dialog::centered(Dk::ConfirmExit, DlgStyle::Box, 200, 50, w, h);
                d.centered_text(&self.font, 8, "EXIT THIS GAME WITHOUT SAVING?", BLACK);
                d.button(30, 28, 50, "&Yes", B_OK, ButtonLook::Normal);
                d.button(120, 28, 50, "&No", B_CANCEL, ButtonLook::Normal);
                d.cancel_button = Some(B_CANCEL);
                self.dialogs.push(d);
            }
            MapEditor => {
                self.seed_counter += 1;
                let s = &self.settings;
                let seed = (self.now as u64) ^ (self.seed_counter * 7919);
                let mut es = EditorState::new(s.map_width, s.map_height, s.land_percent as f64 / 100.0, seed);
                es.zoom = self.zoom;
                let lay = es.layout(w, h);
                let c = es.cursor;
                es.center_on(c, &lay);
                self.dialogs.clear();
                self.screen = Screen::Editor(Box::new(es));
            }
            Quit => self.requests.push(HostRequest::Quit),
            Sound => {
                self.settings.sound = !self.settings.sound;
                self.save_settings();
            }
            Display => self.open_display(),
            Commanders => self.open_commanders(),
            About => self.open_about(),
            View => self.open_view(),
            other => {
                if let Screen::Play(ps) = &mut self.screen {
                    let lay = ps.layout(w, h);
                    let fx = ps.command(other, &lay);
                    self.apply_effects(fx);
                }
            }
        }
    }

    fn apply_effects(&mut self, fx: Vec<PlayEffect>) {
        for e in fx {
            match e {
                PlayEffect::Production(c) => self.open_production(c),
                PlayEffect::UnitProduced { city, unit } => self.open_unit_produced(city, unit),
                PlayEffect::Info(loc) => self.open_info(loc),
                PlayEffect::WarReport => self.open_war_report(),
                PlayEffect::ShipReport => self.open_ship_report(),
                PlayEffect::CombatReport => self.open_combat_report(),
                PlayEffect::WorldMap => self.open_world_map(),
                PlayEffect::HandOff(p) => self.open_handoff(p),
                PlayEffect::Surrender(p) => self.open_surrender(p),
                PlayEffect::GameOver(w) => self.open_game_over(w),
                PlayEffect::ConfirmRisky(u, d) => self.open_confirm_risky(u, d),
                PlayEffect::ConfirmEndTurn(n) => {
                    let (w, h) = (self.frame.width as i32, self.frame.height as i32);
                    let mut d = Dialog::centered(Dk::ConfirmEndTurn, DlgStyle::Box, 220, 50, w, h);
                    d.centered_text(&self.font, 8, format!("{n} UNITS HAVE NO ORDERS. END THE TURN?"), BLACK);
                    d.button(40, 28, 50, "&Yes", B_OK, ButtonLook::Normal);
                    d.button(130, 28, 50, "&No", B_CANCEL, ButtonLook::Normal);
                    d.default_button = Some(B_OK);
                    d.cancel_button = Some(B_CANCEL);
                    self.dialogs.push(d);
                }
                PlayEffect::Alert(s) => self.alert(&s),
                PlayEffect::Sound(s) => self.sound(s),
                PlayEffect::TurnBegan(_) => self.autosave(),
            }
        }
    }

    fn apply_edit_effects(&mut self, fx: Vec<EditEffect>) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        for e in fx {
            match e {
                EditEffect::Sound(s) => self.sound(s),
                EditEffect::Validate(problems) => {
                    let Screen::Editor(es) = &self.screen else { continue };
                    let g = es.map.geom;
                    if problems.is_empty() {
                        self.alert("ALL CITIES ARE ACCEPTABLE.");
                        continue;
                    }
                    let rows: Vec<String> = problems
                        .iter()
                        .map(|(l, p)| format!("{:>3},{:<3} {}", g.row(*l), g.col(*l), p.to_uppercase()))
                        .collect();
                    let mut d = Dialog::centered(Dk::EdValidate, DlgStyle::Report, 230, 120, w, h);
                    d.title = Some("VALIDATE CITIES".into());
                    d.button(10, 6, 40, "&OK", B_OK, ButtonLook::Normal);
                    d.list(Rect::new(4, 24, 222, 90), rows, 0, L_MAIN, false);
                    d.default_button = Some(B_OK);
                    d.cancel_button = Some(B_OK);
                    self.dialogs.push(d);
                }
                EditEffect::WorldView => {
                    let Screen::Editor(es) = &self.screen else { continue };
                    let (sea, land, cities) = es.counts();
                    let dw = (w - 40).min(420);
                    let dh = (h - 40).min(260);
                    let mut d = Dialog::centered(Dk::EdWorldView, DlgStyle::MapWindow, dw, dh, w, h);
                    d.title = Some("WORLD VIEW".into());
                    d.button(14, 6, 40, "&OK", B_OK, ButtonLook::Normal);
                    d.text(60, 24, format!("SEA: {sea}  LAND: {land}  CITIES: {cities}"), BLACK);
                    d.elems.push(Elem::Frame { r: d.rel(4, 33, dw - 8, dh - 37), color: CYAN });
                    d.elems.push(Elem::Custom { r: d.rel(5, 34, dw - 10, dh - 39), id: 1 });
                    d.default_button = Some(B_OK);
                    d.cancel_button = Some(B_OK);
                    self.dialogs.push(d);
                }
                EditEffect::AskSprinkle => {
                    let mut d = Dialog::centered(Dk::EdSprinkle, DlgStyle::Box, 170, 46, w, h);
                    d.centered_text(&self.font, 8, "SPRINKLE CITIES ON THE NEW WORLD?", BLACK);
                    d.button(25, 26, 50, "&Yes", B_OK, ButtonLook::Normal);
                    d.button(95, 26, 50, "&No", B_CANCEL, ButtonLook::Normal);
                    d.default_button = Some(B_OK);
                    d.cancel_button = Some(B_CANCEL);
                    self.dialogs.push(d);
                }
                EditEffect::LoadMap => {
                    let names = self.available_map_names();
                    let rows = if names.is_empty() { vec!["(NO MAPS)".to_string()] } else { names.clone() };
                    let mut d = Dialog::centered(Dk::EdLoadList { names }, DlgStyle::Panel, 200, 130, w, h);
                    d.title = Some("LOAD MAP".into());
                    d.button(10, 9, 40, "&OK", B_OK, ButtonLook::Normal);
                    d.button(148, 9, 42, "&Cancel", B_CANCEL, ButtonLook::Normal);
                    d.list(Rect::new(10, 28, 180, 76), rows, 0, L_MAIN, false);
                    d.button(10, 110, 180, "&Upload a .emap file", B_UPLOAD, ButtonLook::Normal);
                    d.default_button = Some(B_OK);
                    d.cancel_button = Some(B_CANCEL);
                    self.dialogs.push(d);
                }
                EditEffect::SaveMap => {
                    let name = match &self.screen {
                        Screen::Editor(es) => es.name.clone(),
                        _ => None,
                    };
                    match name {
                        Some(n) => self.save_map(&n),
                        None => self.apply_edit_effects(vec![EditEffect::SaveMapAs]),
                    }
                }
                EditEffect::SaveMapAs => {
                    let current = match &self.screen {
                        Screen::Editor(es) => es.name.clone().unwrap_or_default(),
                        _ => continue,
                    };
                    let mut d = Dialog::centered(Dk::EdSaveAs, DlgStyle::Panel, 200, 76, w, h);
                    d.title = Some("SAVE MAP AS".into());
                    d.text(10, 22, "NAME:", WHITE);
                    d.elems.push(Elem::Input { r: d.rel(40, 19, 150, 13), text: current, max: 16 });
                    d.button(10, 42, 40, "&OK", B_OK, ButtonLook::Normal);
                    d.button(60, 42, 70, "Down&load", B_DOWNLOAD, ButtonLook::Normal);
                    d.button(140, 42, 50, "&Cancel", B_CANCEL, ButtonLook::Normal);
                    d.text(10, 62, "SAVED MAPS APPEAR UNDER SELECT", WHITE);
                    d.cancel_button = Some(B_CANCEL);
                    self.dialogs.push(d);
                }
                EditEffect::Exit => {
                    let dirty = matches!(&self.screen, Screen::Editor(es) if es.dirty);
                    if !dirty {
                        self.screen = Screen::Desktop;
                        continue;
                    }
                    let mut d = Dialog::centered(Dk::EdExit, DlgStyle::Box, 180, 46, w, h);
                    d.centered_text(&self.font, 8, "LEAVE WITHOUT SAVING THE MAP?", BLACK);
                    d.button(30, 26, 50, "&Yes", B_OK, ButtonLook::Normal);
                    d.button(100, 26, 50, "&No", B_CANCEL, ButtonLook::Normal);
                    d.cancel_button = Some(B_CANCEL);
                    self.dialogs.push(d);
                }
            }
        }
    }

    fn save_map(&mut self, name: &str) {
        let Screen::Editor(es) = &mut self.screen else { return };
        let text = es.map.to_text();
        es.name = Some(name.to_string());
        es.dirty = false;
        es.status();
        es.message = format!("MAP SAVED AS {name}");
        let key = format!("empire.map.{name}");
        self.store(&key, Some(text));
    }

    // ------------------------------------------------------------------
    // Dialog builders

    fn alert(&mut self, text: &str) {
        let font = &self.font;
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let max_chars = ((w - 60) / font.advance).min(56) as usize;
        let lines = ui::wrap(&text.to_uppercase(), max_chars);
        let tw = lines.iter().map(|l| font.width(l)).max().unwrap_or(40).max(60);
        let dh = 30 + lines.len() as i32 * 9;
        let mut d = Dialog::centered(Dk::Alert, DlgStyle::Box, tw + 24, dh, w, h);
        for (i, l) in lines.iter().enumerate() {
            d.centered_text(font, 7 + i as i32 * 9, l.clone(), BLACK);
        }
        let bw = 40;
        d.button((d.rect.w - bw) / 2, dh - 18, bw, "&OK", B_OK, ButtonLook::Normal);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_OK);
        self.dialogs.push(d);
    }

    fn begin_new_game(&mut self) {
        self.setup = Some(Setup {
            kinds: vec![PType::Human, PType::Standard, PType::Nobody],
            players: Vec::new(),
            game: None,
            name_idx: 0,
        });
        self.open_opponents();
    }

    fn open_opponents(&mut self) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let font = self.font.clone();
        let kinds = self.setup.as_ref().map(|s| s.kinds.clone()).unwrap_or_default();
        let mut d = Dialog::centered(Dk::Opponents, DlgStyle::Panel, 262, 118, w, h);
        d.title = Some("TODAY'S OPPONENTS".into());
        d.button(10, 9, 40, "&OK", B_OK, ButtonLook::Normal);
        d.button(210, 9, 42, "&Cancel", B_CANCEL, ButtonLook::Normal);
        d.text(126, 28, "COMPUTER OPPONENTS", WHITE);
        for (p, k) in kinds.iter().enumerate() {
            let y = 38 + p as i32 * 20;
            d.hot_text(6, y + 3, format!("PLAYER #{} [&{}]", p + 1, p + 1), WHITE);
            let opts: &[(PType, &str, i32)] = if p == 0 {
                &[(PType::Human, "HUMAN", 70), (PType::Standard, "STANDARD", 128), (PType::Expert, "EXPERT", 172)]
            } else {
                &[(PType::Human, "HUMAN", 70), (PType::Standard, "STANDARD", 128), (PType::Expert, "EXPERT", 172), (PType::Nobody, "NONE", 216)]
            };
            for (i, (pt, label, x)) in opts.iter().enumerate() {
                if p == 1 && *pt == PType::Nobody {
                    continue;
                }
                let look = if k == pt { ButtonLook::Selected } else { ButtonLook::Normal };
                let bw = font.width(label) + 10;
                d.button(*x, y, bw, label, B_OPP + p as u32 * 10 + i as u32, look);
            }
        }
        let _ = font;
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_CANCEL);
        self.dialogs.push(d);
    }

    fn opponent_kind_for(p: usize, i: u32) -> PType {
        match i {
            0 => PType::Human,
            1 => PType::Standard,
            2 => PType::Expert,
            _ => {
                let _ = p;
                PType::Nobody
            }
        }
    }

    fn open_options(&mut self, sel: usize) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let players = self.setup.as_ref().map(|s| s.players.clone()).unwrap_or_default();
        let n = players.len() as i32;
        let col_w = 100;
        let dw = (col_w * n + 20).max(220);
        let mut d = Dialog::centered(Dk::Options { sel }, DlgStyle::Box, dw, 150, w, h);
        d.title = Some("NEW GAME OPTIONS".into());
        d.button(10, 9, 40, "&OK", B_OK, ButtonLook::Normal);
        d.button(dw - 52, 9, 42, "&Cancel", B_CANCEL, ButtonLook::Normal);
        let ratings = ratings(&players);
        for (i, p) in players.iter().enumerate() {
            let x = 10 + i as i32 * col_w + (dw - 20 - col_w * n) / 2;
            let color = if i == sel { MAGENTA } else { BLACK };
            let kind = match p.kind {
                PType::Human => "",
                PType::Standard => " STD",
                PType::Expert => " EXP",
                PType::Nobody => "",
            };
            d.text(x + 6, 28, format!("PLAYER #{} [{}]{}", i + 1, i + 1, kind), color);
            d.elems.push(Elem::Slider { r: d.rel(x + 6, 48, 80, 9), value: p.production, id: S_PROD + i as u32 * 10 });
            d.text(x + 30, 60, format!("{}%", p.production), BLACK);
            d.elems.push(Elem::Slider { r: d.rel(x + 6, 84, 80, 9), value: p.combat, id: S_COMBAT + i as u32 * 10 });
            d.text(x + 30, 96, format!("{}%", p.combat), BLACK);
            d.text(x + 30, 112, "RATING", BLACK);
            d.text(x + 34, 122, format!("{}%", ratings[i]), BLACK);
        }
        d.centered_text(&self.font, 38, "--- PRODUCTION EFFICIENCY [F1-F2] ---", BLACK);
        d.centered_text(&self.font, 74, "--- COMBAT EFFICIENCY [F3-F4] ---", BLACK);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_CANCEL);
        self.dialogs.push(d);
    }

    fn open_world_map_choice(&mut self) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let s = &self.settings;
        let mut d = Dialog::centered(Dk::WorldMapChoice, DlgStyle::Box, 190, 78, w, h);
        d.centered_text(&self.font, 6, "WORLD MAP?", BLACK);
        d.button(20, 18, 60, "&Random", B_RANDOM, ButtonLook::Normal);
        d.button(110, 18, 60, "&Select", B_SELECT, ButtonLook::Normal);
        d.text(10, 40, "NEW WORLD SIZE:", BLACK);
        d.button(100, 37, 80, &format!("&Z {}X{}", s.map_width, s.map_height), B_SIZE, ButtonLook::Normal);
        d.text(10, 57, "LAND:", BLACK);
        d.button(100, 54, 80, &format!("&L {}%", s.land_percent), B_LAND, ButtonLook::Normal);
        d.default_button = Some(B_RANDOM);
        d.cancel_button = Some(B_CANCEL);
        self.dialogs.push(d);
    }

    fn available_map_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.maps.iter().map(|(n, _)| n.clone()).collect();
        for k in self.storage.keys() {
            if let Some(n) = k.strip_prefix("empire.map.") {
                names.push(format!("{n} (YOURS)"));
            }
        }
        names
    }

    fn map_text(&self, name: &str) -> Option<String> {
        if let Some(n) = name.strip_suffix(" (YOURS)") {
            return self.storage.get(&format!("empire.map.{n}")).cloned();
        }
        self.maps.iter().find(|(n, _)| n == name).map(|(_, t)| t.clone())
    }

    fn open_map_select(&mut self) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let names = self.available_map_names();
        let mut d = Dialog::centered(Dk::MapSelect { names: names.clone() }, DlgStyle::Panel, 200, 140, w, h);
        d.title = Some("SELECT A WORLD".into());
        d.button(10, 9, 40, "&OK", B_OK, ButtonLook::Normal);
        d.button(148, 9, 42, "&Cancel", B_CANCEL, ButtonLook::Normal);
        let rows = if names.is_empty() { vec!["(NO MAPS LOADED)".to_string()] } else { names };
        d.list(Rect::new(10, 28, 180, 86), rows, 0, L_MAIN, false);
        d.button(10, 120, 180, "&Upload a .emap file", B_UPLOAD, ButtonLook::Normal);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_CANCEL);
        self.dialogs.push(d);
    }

    fn open_name(&mut self, idx: usize) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let mut d = Dialog::centered(Dk::Name { idx }, DlgStyle::Box, 180, 56, w, h);
        d.centered_text(&self.font, 6, format!("PLAYER #{}", idx + 1), BLACK);
        d.centered_text(&self.font, 16, "PLEASE ENTER YOUR LAST NAME ...", BLACK);
        let existing = self.setup.as_ref().map(|s| s.players[idx].name.clone()).unwrap_or_default();
        d.elems.push(Elem::Input { r: d.rel(40, 30, 100, 13), text: existing, max: 16 });
        d.cancel_button = None;
        self.dialogs.push(d);
    }

    fn open_report(&mut self, idx: usize) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let name = self.setup.as_ref().map(|s| s.players[idx].name.clone()).unwrap_or_default();
        self.seed_counter += 1;
        let planet = rank_planet(self.seed_counter + name.len() as u64);
        let mut d = Dialog::centered(Dk::Report { idx }, DlgStyle::Box, 210, 50, w, h);
        d.centered_text(&self.font, 6, format!("{planet} GENERAL {name}. . ."), BLACK);
        d.centered_text(&self.font, 15, "REPORT TO YOUR COMMAND POST!!", BLACK);
        d.button(85, 30, 40, "&OK", B_OK, ButtonLook::Normal);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_OK);
        self.dialogs.push(d);
    }

    fn open_production(&mut self, city: usize) {
        let Screen::Play(ps) = &self.screen else { return };
        let g = &ps.game;
        let c = &g.cities[city];
        let owner = c.owner;
        let eff = g.player(owner).production_eff;
        let sel = c.production.map(|k| k.index()).unwrap_or(0);
        let mut d = Dialog::new(Dk::Production { city }, DlgStyle::Panel, Rect::new(16, 30, 146, 128));
        d.title = Some("CITY PRODUCTION".into());
        d.elems.push(Elem::Icon { x: d.rect.x + 6, y: d.rect.y + 5, art: CellArt::City { owner, occupied: false } });
        d.text(6, 24, format!("CITY: {}", c.name.to_uppercase()), WHITE);
        let geom = g.geom();
        d.text(6, 36, format!("LOC: {},{}", geom.row(c.loc), geom.col(c.loc)), WHITE);
        d.button(96, 21, 40, "&OK", B_OK, ButtonLook::Normal);
        d.text(92, 38, "TURNS UNDR.", WHITE);
        d.text(92, 46, "CONST. CNST.", WHITE);
        let rows: Vec<String> = UNIT_TYPES
            .iter()
            .map(|&k| {
                let plural = match k {
                    UnitType::Army => "ARMIES".to_string(),
                    _ => format!("{}S", k.name().to_uppercase()),
                };
                let turns = g.rules.build_time(k, true, eff);
                let building = g.player_cities(owner).filter(|&ci| g.cities[ci].production == Some(k)).count();
                let mark = if g.can_build(city, k) { "" } else { "-" };
                format!("{mark}{plural:<11}{turns:>5}{building:>6}")
            })
            .collect();
        d.list(Rect::new(30, 56, 110, 68), rows, sel, L_MAIN, true);
        if let Some(Elem::List { icons, .. }) = d.elems.last_mut() {
            for (i, k) in UNIT_TYPES.iter().enumerate() {
                icons[i] = Some(CellArt::Unit { kind: *k, owner, over_sea: false, sentry: false, loaded: false });
            }
        }
        d.default_button = Some(B_OK);
        d.cancel_button = if c.production.is_some() { Some(B_CANCEL) } else { None };
        self.dialogs.push(d);
    }

    fn open_unit_produced(&mut self, city: usize, unit: UnitId) {
        let Screen::Play(ps) = &self.screen else { return };
        let g = &ps.game;
        let u = g.unit(unit);
        let c = &g.cities[city];
        let w = self.frame.width as i32;
        let dw = 162;
        let mut d = Dialog::new(Dk::UnitProduced { city, unit }, DlgStyle::Panel, Rect::new((w - dw) / 2, 30, dw, 66));
        d.elems.push(Elem::Icon {
            x: d.rect.x + dw / 2 - 4,
            y: d.rect.y + 4,
            art: CellArt::Unit { kind: u.kind, owner: u.owner, over_sea: false, sentry: false, loaded: false },
        });
        d.elems.push(Elem::Fill { r: d.rel(4, 14, dw - 8, 30), color: WHITE });
        d.elems.push(Elem::Frame { r: d.rel(4, 14, dw - 8, 30), color: BLACK });
        if u.kind.is_ship() {
            d.centered_text(&self.font, 18, format!("{} JUST PRODUCED A {}", c.name.to_uppercase(), u.kind.name().to_uppercase()), BLACK);
            d.hot_text(8, 31, "&SHIP NAME:", BLACK);
            d.elems.push(Elem::Input { r: d.rel(54, 28, dw - 62, 13), text: u.name.to_uppercase(), max: 16 });
        } else {
            d.centered_text(&self.font, 18, format!("{} JUST PRODUCED THE", c.name.to_uppercase()), BLACK);
            d.centered_text(&self.font, 30, u.name.to_uppercase(), BLACK);
        }
        d.button(8, 49, 40, "&OK", B_OK, ButtonLook::Normal);
        d.button(dw - 62, 49, 54, "&Production", B_PROD, ButtonLook::Normal);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_OK);
        self.dialogs.push(d);
    }

    fn open_info(&mut self, loc: Loc) {
        let Screen::Play(ps) = &self.screen else { return };
        let g = &ps.game;
        let geom = g.geom();
        let me = ps.viewer;
        let view = g.player(me).view[loc];
        let mut lines: Vec<(String, String)> = Vec::new();
        let mut art = crate::mapdraw::cell_art(g, me, loc, ps.reveal_all);
        lines.push(("LOCATION:".into(), format!("{},{}", geom.row(loc), geom.col(loc))));
        if let Some(ci) = g.city_at(loc).filter(|_| !view.is_unknown() || ps.reveal_all) {
            let c = &g.cities[ci];
            lines.insert(0, ("CITY:".into(), c.name.to_uppercase()));
            if c.owner == me || ps.reveal_all {
                let prod = c.production.map(|k| k.name().to_uppercase()).unwrap_or_else(|| "NOTHING".into());
                lines.push(("PRODUCING:".into(), prod));
                if c.production.is_some() {
                    lines.push(("COMPLETE IN:".into(), format!("{} TURNS", c.ready_turn.saturating_sub(g.turn))));
                }
                let here = g.units_at(loc).len();
                lines.push(("UNITS INSIDE:".into(), here.to_string()));
                if let Some(fp) = c.flight_path {
                    let to = g.city_at(fp).map(|t| g.cities[t].name.to_uppercase()).unwrap_or_default();
                    lines.push(("FLIGHT PATH:".into(), to));
                }
                art = CellArt::City { owner: c.owner, occupied: false };
            } else if c.owner == 0 {
                lines.push(("STATUS:".into(), "NEUTRAL".into()));
            } else {
                lines.push(("OWNER:".into(), format!("PLAYER {}", c.owner)));
                match g.player(me).recon.iter().find(|(x, _, _)| *x == ci) {
                    Some((_, p, t)) => lines.push((
                        format!("RECON T{t}:"),
                        p.map(|k| k.name().to_uppercase()).unwrap_or_else(|| "NOTHING".into()),
                    )),
                    None => lines.push(("PRODUCING:".into(), "UNKNOWN".into())),
                }
            }
        } else if view.owner() == me || ps.reveal_all {
            if let Some(id) = g.top_unit(loc) {
                let u = g.unit(id);
                let st = g.rules.stats(u.kind);
                lines.insert(0, ("UNIT:".into(), if u.kind.is_ship() { format!("{} {}", u.kind.name(), u.name).to_uppercase() } else { u.name.to_uppercase() }));
                lines.push(("ORDERS:".into(), u.orders.describe().to_uppercase()));
                if u.kind.is_ship() {
                    lines.push(("DAMAGE:".into(), format!("{}/{}", st.hits - u.hits, st.hits)));
                }
                if u.kind == UnitType::Fighter {
                    lines.push(("FUEL:".into(), u.fuel.to_string()));
                }
                if u.kind.cargo().is_some() {
                    lines.push(("ABOARD:".into(), format!("{} OF {}", g.cargo(id).len(), g.capacity(id))));
                }
            }
        } else if let Some(t) = view.unit_type() {
            lines.insert(0, ("UNIT:".into(), format!("ENEMY {} (LAST SEEN)", t.name().to_uppercase())));
        } else {
            let what = if view.is_unknown() { "UNEXPLORED" } else if view.is_sea() { "SEA" } else { "LAND" };
            lines.insert(0, ("TERRAIN:".into(), what.into()));
        }
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let dw = 170;
        let dh = 30 + lines.len() as i32 * 9 + 6;
        let mut d = Dialog::centered(Dk::Info, DlgStyle::Panel, dw, dh, w, h);
        d.elems.push(Elem::Icon { x: d.rect.x + 14, y: d.rect.y + 6, art });
        d.button(dw - 50, 4, 42, "&OK", B_OK, ButtonLook::Normal);
        d.elems.push(Elem::Fill { r: d.rel(4, 22, dw - 8, dh - 26), color: WHITE });
        d.elems.push(Elem::Frame { r: d.rel(4, 22, dw - 8, dh - 26), color: BLACK });
        for (i, (k, v)) in lines.iter().enumerate() {
            let y = 26 + i as i32 * 9;
            d.text(8, y, k.clone(), BLACK);
            d.text(66, y, v.chars().take(24).collect::<String>(), BLACK);
        }
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_OK);
        self.dialogs.push(d);
    }

    fn open_war_report(&mut self) {
        let Screen::Play(ps) = &self.screen else { return };
        let g = &ps.game;
        let me = ps.viewer;
        let p = g.player(me);
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let mut d = Dialog::centered(Dk::WarReport, DlgStyle::Report, 240, 150, w, h);
        d.title = Some("WAR REPORT".into());
        d.button(10, 6, 40, "&OK", B_OK, ButtonLook::Normal);
        let owned = g.player_cities(me).count();
        let explored = p.view.iter().filter(|v| !v.is_unknown()).count() * 100 / g.geom().size().max(1);
        d.text(80, 24, "CAPTURED  EXIST", BLACK);
        d.text(170, 18, "WORLD", BLACK);
        d.text(160, 26, "EXPLORED", BLACK);
        d.elems.push(Elem::Icon { x: d.rect.x + 8, y: d.rect.y + 32, art: CellArt::City { owner: me, occupied: false } });
        d.text(20, 34, "CITIES", BLACK);
        d.text(92, 34, format!("{owned:>3}    {:>3}", g.cities.len()), BLACK);
        d.text(176, 34, format!("{explored}%"), BLACK);
        d.text(68, 50, "UNDER  SOONEST ARE IN", BLACK);
        d.text(68, 58, "CONSTR COMPLETE COMBAT", BLACK);
        d.text(164, 50, " WE'VE  WE'VE", MAGENTA);
        d.text(164, 58, "DESTRYD  LOST", MAGENTA);
        for (i, k) in UNIT_TYPES.iter().enumerate() {
            let y = 68 + i as i32 * 9;
            let building: Vec<usize> = g.player_cities(me).filter(|&ci| g.cities[ci].production == Some(*k)).collect();
            let soonest = building.iter().map(|&ci| g.cities[ci].ready_turn.saturating_sub(g.turn)).min();
            let alive = g.player_units(me).filter(|&u| g.unit(u).kind == *k).count();
            d.elems.push(Elem::Icon { x: d.rect.x + 6, y: d.rect.y + y, art: CellArt::Unit { kind: *k, owner: me, over_sea: false, sentry: false, loaded: false } });
            d.text(16, y, k.name().to_uppercase(), BLACK);
            let soon = soonest.map(|s| format!("{s:>4}")).unwrap_or_else(|| " ---".into());
            d.text(
                70,
                y,
                format!("{:>4}   {}   {:>4}   {:>4}   {:>4}", building.len(), soon, alive, p.stats.destroyed[k.index()], p.stats.lost[k.index()]),
                BLACK,
            );
        }
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_OK);
        self.dialogs.push(d);
    }

    fn open_ship_report(&mut self) {
        let Screen::Play(ps) = &self.screen else { return };
        let g = &ps.game;
        let me = ps.viewer;
        let geom = g.geom();
        let rows: Vec<String> = g
            .player_units(me)
            .filter(|&u| g.unit(u).kind.is_ship())
            .map(|u| {
                let s = g.unit(u);
                let st = g.rules.stats(s.kind);
                format!(
                    "{:<16} {:<10} {}/{:<3} {:>3},{:<3}",
                    s.name.to_uppercase(),
                    s.kind.name().to_uppercase(),
                    st.hits - s.hits,
                    st.hits,
                    geom.row(s.loc),
                    geom.col(s.loc)
                )
            })
            .collect();
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let mut d = Dialog::centered(Dk::ShipReport, DlgStyle::Report, 220, 128, w, h);
        d.title = Some("SHIP REPORT".into());
        d.button(10, 6, 40, "&OK", B_OK, ButtonLook::Normal);
        d.text(10, 24, "SHIP             TYPE       DAMAGE LOC", BLACK);
        let rows = if rows.is_empty() { vec!["THERE ARE NO SHIPS REGISTERED.".to_string()] } else { rows };
        d.list(Rect::new(4, 33, 212, 90), rows, 0, L_MAIN, false);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_OK);
        self.dialogs.push(d);
    }

    fn open_combat_report(&mut self) {
        let Screen::Play(ps) = &self.screen else { return };
        let g = &ps.game;
        let me = ps.viewer;
        let geom = g.geom();
        let log = &g.player(me).last_combat_log;
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        if log.is_empty() {
            let mut d = Dialog::centered(Dk::CombatReport, DlgStyle::Box, 110, 46, w, h);
            d.centered_text(&self.font, 6, "WE HAVE NO BATTLES", BLACK);
            d.centered_text(&self.font, 14, "TO REPORT!", BLACK);
            d.button(30, 28, 50, "&Great!", B_OK, ButtonLook::Normal);
            d.default_button = Some(B_OK);
            d.cancel_button = Some(B_OK);
            self.dialogs.push(d);
            return;
        }
        let rows: Vec<String> = log
            .iter()
            .map(|r| {
                format!(
                    "{},{} {} BY {} {}",
                    geom.row(r.loc),
                    geom.col(r.loc),
                    r.defender_name.to_uppercase(),
                    r.attacker_kind.map(|k| k.name().to_uppercase()).unwrap_or_default(),
                    if r.defender_won { "WON" } else { "LOST" }
                )
            })
            .collect();
        let mut d = Dialog::centered(Dk::CombatReport, DlgStyle::Report, 230, 120, w, h);
        d.title = Some("COMBAT REPORT".into());
        d.button(10, 6, 40, "&OK", B_OK, ButtonLook::Normal);
        d.list(Rect::new(4, 24, 222, 90), rows, 0, L_MAIN, false);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_OK);
        self.dialogs.push(d);
    }

    fn open_world_map(&mut self) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let dw = (w - 40).min(420);
        let dh = (h - 40).min(260);
        let mut d = Dialog::centered(Dk::WorldMap, DlgStyle::MapWindow, dw, dh, w, h);
        d.title = Some("WORLD MAP".into());
        d.button(14, 6, 40, "&OK", B_OK, ButtonLook::Normal);
        d.elems.push(Elem::Frame { r: d.rel(4, 22, dw - 8, dh - 26), color: CYAN });
        d.elems.push(Elem::Custom { r: d.rel(5, 23, dw - 10, dh - 28), id: 0 });
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_OK);
        self.dialogs.push(d);
    }

    fn open_display(&mut self) {
        let s = self.settings.clone();
        self.build_display(s);
    }

    fn build_display(&mut self, s: Settings) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let has_icons = self.has_original_icons();
        let has_font = self.font_original.is_some();
        let mut d = Dialog::centered(Dk::Display { s: s.clone() }, DlgStyle::Report, 304, 184, w, h);
        d.title = Some("GAME DISPLAY CONTROLS".into());
        d.button(10, 6, 40, "&OK", B_OK, ButtonLook::Normal);
        d.button(252, 6, 42, "&Cancel", B_CANCEL, ButtonLook::Normal);
        d.hot_text(40, 26, "&BATTLE DELAY", BLACK);
        d.elems.push(Elem::Slider { r: d.rel(28, 34, 96, 9), value: s.battle_delay, id: S_BATTLE });
        d.text(66, 46, format!("{}%", s.battle_delay), BLACK);
        d.text(194, 26, "MESSAGE DELAY", BLACK);
        d.elems.push(Elem::Slider { r: d.rel(180, 34, 96, 9), value: s.message_delay, id: S_MSG });
        d.text(218, 46, format!("{}%", s.message_delay), BLACK);

        let look = |enabled: bool| if enabled { ButtonLook::Normal } else { ButtonLook::Disabled };
        let rows = [62, 80, 98, 116, 134];
        let (lx, rx, bw) = (8, 156, 66);
        // Left column: how the screen is drawn.
        d.text(lx, rows[0] + 3, "LOOK", BLACK);
        d.button(lx + 74, rows[0], bw, if s.hd { "&L HIGH-RES" } else { "&L CGA" }, B_LOOK, ButtonLook::Normal);
        d.text(lx, rows[1] + 3, "TEXT SIZE", BLACK);
        let size = TEXT_SIZES[(s.text_size as usize).min(TEXT_SIZES.len() - 1)].1;
        d.button(lx + 74, rows[1], bw, &format!("&T {size}"), B_TEXT, look(s.hd));
        d.text(lx, rows[2] + 3, "SCREEN SIZE", BLACK);
        d.button(lx + 74, rows[2], bw, &format!("&Z {}", s.resolution.label()), B_RES, look(!s.hd));
        d.text(lx, rows[3] + 3, "PIXEL SHAPE", BLACK);
        d.button(lx + 74, rows[3], bw, if s.cga_aspect { "&P CGA 4:3" } else { "&P SQUARE" }, B_ASPECT, look(!s.hd));
        d.text(lx, rows[4] + 3, "MAP ZOOM", BLACK);
        let zoom_label = if s.map_square == 0 { "&A AUTO".to_string() } else { format!("&A {}PX", s.map_square) };
        d.button(lx + 74, rows[4], bw, &zoom_label, B_ZOOM, look(s.hd));
        // Right column: what is drawn.
        d.text(rx, rows[0] + 3, "ICONS", BLACK);
        let icon_label = if s.icon_set == 0 || !has_icons { "&I BUILT-IN".to_string() } else { format!("&I ORIGINAL {}", s.icon_set) };
        d.button(rx + 74, rows[0], bw, &icon_label, B_ICONS, look(has_icons));
        d.text(rx, rows[1] + 3, "SMOOTH ICONS", BLACK);
        d.button(rx + 74, rows[1], bw, if s.smooth_icons { "&M ON" } else { "&M OFF" }, B_SMOOTH, look(s.hd));
        d.text(rx, rows[2] + 3, "FONT", BLACK);
        let font_label = if s.hd && s.smooth_text {
            "&F SMOOTH"
        } else if s.original_font && has_font {
            "&F ORIGINAL"
        } else {
            "&F PIXEL"
        };
        d.button(rx + 74, rows[2], bw, font_label, B_FONT, ButtonLook::Normal);
        d.text(rx, rows[3] + 3, "UNIT REPORTS", BLACK);
        d.button(rx + 74, rows[3], bw, if s.report_armies_fighters { "&R ON" } else { "&R OFF" }, B_REPORT_ON, ButtonLook::Normal);
        let note = if s.hd {
            "HIGH-RES DRAWS SHARP TEXT AT YOUR SCREEN'S RESOLUTION."
        } else {
            "CGA SHOWS THE 1987 PICTURE PIXEL FOR PIXEL."
        };
        d.centered_text(&self.font, 156, note, BLACK);
        d.centered_text(&self.font, 166, "MAP ZOOM: [ AND ] KEYS. UNIT REPORTS: EACH NEW ARMY AND FIGHTER.", BLACK);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_CANCEL);
        self.dialogs.push(d);
    }

    fn open_confirm_risky(&mut self, unit: UnitId, dir: Dir) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let mut d = Dialog::centered(Dk::ConfirmRisky { unit, dir }, DlgStyle::Box, 170, 46, w, h);
        d.centered_text(&self.font, 6, "THAT MOVE WILL PROBABLY", BLACK);
        d.centered_text(&self.font, 14, "DESTROY YOUR UNIT. ARE YOU SURE?", BLACK);
        d.button(25, 28, 50, "&Yes", B_OK, ButtonLook::Normal);
        d.button(95, 28, 50, "&No", B_CANCEL, ButtonLook::Normal);
        d.default_button = Some(B_CANCEL);
        d.cancel_button = Some(B_CANCEL);
        self.dialogs.push(d);
    }

    fn save_keys(&self) -> Vec<String> {
        self.storage.keys().filter(|k| k.starts_with("empire.save.") || k.as_str() == "empire.autosave").cloned().collect()
    }

    fn save_label(k: &str) -> String {
        if k == "empire.autosave" {
            "(AUTOSAVE)".into()
        } else {
            k.trim_start_matches("empire.save.").to_string()
        }
    }

    fn open_save_as(&mut self) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let current = match &self.screen {
            Screen::Play(ps) => ps.save_name.clone().unwrap_or_default(),
            _ => return,
        };
        let mut d = Dialog::centered(Dk::SaveAs, DlgStyle::Panel, 200, 76, w, h);
        d.title = Some("SAVE GAME AS".into());
        d.text(10, 22, "NAME:", WHITE);
        d.elems.push(Elem::Input { r: d.rel(40, 19, 150, 13), text: current, max: 20 });
        d.button(10, 42, 40, "&OK", B_OK, ButtonLook::Normal);
        d.button(60, 42, 70, "Down&load", B_DOWNLOAD, ButtonLook::Normal);
        d.button(140, 42, 50, "&Cancel", B_CANCEL, ButtonLook::Normal);
        d.text(10, 62, "SAVES STAY IN THIS BROWSER", WHITE);
        d.cancel_button = Some(B_CANCEL);
        self.dialogs.push(d);
    }

    fn open_load(&mut self) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let keys = self.save_keys();
        let rows: Vec<String> = if keys.is_empty() { vec!["(NO SAVED GAMES)".into()] } else { keys.iter().map(|k| Self::save_label(k)).collect() };
        let mut d = Dialog::centered(Dk::LoadList { keys }, DlgStyle::Panel, 200, 130, w, h);
        d.title = Some("LOAD GAME".into());
        d.button(10, 9, 40, "&OK", B_OK, ButtonLook::Normal);
        d.button(148, 9, 42, "&Cancel", B_CANCEL, ButtonLook::Normal);
        d.list(Rect::new(10, 28, 180, 76), rows, 0, L_MAIN, false);
        d.button(10, 110, 180, "From a &file...", B_UPLOAD, ButtonLook::Normal);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_CANCEL);
        self.dialogs.push(d);
    }

    fn open_delete(&mut self) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let mut keys = self.save_keys();
        keys.extend(self.storage.keys().filter(|k| k.starts_with("empire.map.")).cloned());
        let rows: Vec<String> = if keys.is_empty() {
            vec!["(NOTHING TO DELETE)".into()]
        } else {
            keys.iter()
                .map(|k| if let Some(m) = k.strip_prefix("empire.map.") { format!("MAP {m}") } else { Self::save_label(k) })
                .collect()
        };
        let mut d = Dialog::centered(Dk::DeleteList { keys }, DlgStyle::Panel, 200, 112, w, h);
        d.title = Some("DELETE".into());
        d.button(10, 9, 50, "&Delete", B_OK, ButtonLook::Normal);
        d.button(148, 9, 42, "&Cancel", B_CANCEL, ButtonLook::Normal);
        d.list(Rect::new(10, 28, 180, 76), rows, 0, L_MAIN, false);
        d.cancel_button = Some(B_CANCEL);
        self.dialogs.push(d);
    }

    fn open_handoff(&mut self, p: PlayerId) {
        let Screen::Play(ps) = &self.screen else { return };
        let name = ps.game.player(p).name.to_uppercase();
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let mut d = Dialog::centered(Dk::HandOff, DlgStyle::Box, 200, 50, w, h);
        d.centered_text(&self.font, 6, format!("PLAYER #{p}: GENERAL {name}"), BLACK);
        d.centered_text(&self.font, 15, "REPORT TO YOUR COMMAND POST!!", BLACK);
        d.button(80, 30, 40, "&OK", B_OK, ButtonLook::Normal);
        d.default_button = Some(B_OK);
        self.dialogs.push(d);
    }

    fn open_surrender(&mut self, p: PlayerId) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let mut d = Dialog::centered(Dk::Surrender, DlgStyle::Box, 210, 50, w, h);
        d.centered_text(&self.font, 6, format!("PLAYER #{p} OFFERS TO SURRENDER."), BLACK);
        d.centered_text(&self.font, 15, "DO YOU ACCEPT?", BLACK);
        d.button(30, 30, 60, "&Accept", B_ACCEPT, ButtonLook::Normal);
        d.button(120, 30, 60, "&Refuse", B_REFUSE, ButtonLook::Normal);
        self.dialogs.push(d);
    }

    fn open_game_over(&mut self, winner: Option<PlayerId>) {
        let Screen::Play(ps) = &self.screen else { return };
        let me = ps.viewer;
        let text = match winner {
            Some(w) if w == me && ps.game.player(me).is_human() => "CONGRATULATIONS! YOU HAVE CONQUERED THE WORLD!".to_string(),
            Some(w) => format!("PLAYER #{w} ({}) HAS CONQUERED THE WORLD.", ps.game.player(w).name.to_uppercase()),
            None => "THE WAR IS OVER. NOBODY WON.".into(),
        };
        self.record_commanders(winner);
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let mut d = Dialog::centered(Dk::GameOver, DlgStyle::Box, 230, 50, w, h);
        d.centered_text(&self.font, 8, text, BLACK);
        d.centered_text(&self.font, 17, "YOU MAY NOW VIEW THE WORLD.", BLACK);
        d.button(95, 31, 40, "&OK", B_OK, ButtonLook::Normal);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_OK);
        self.dialogs.push(d);
        self.sound("gameover");
    }

    fn open_commanders(&mut self) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let log: BTreeMap<String, [u32; 3]> =
            self.storage.get("empire.commanders").and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default();
        let mut rows = vec![format!("{:<16} {:>5} {:>5} {:>6}", "COMMANDER", "WARS", "WINS", "LOSSES")];
        for (n, [wars, wins, losses]) in &log {
            rows.push(format!("{:<16} {wars:>5} {wins:>5} {losses:>6}", n.to_uppercase()));
        }
        let mut d = Dialog::centered(Dk::Commanders, DlgStyle::Report, 200, 110, w, h);
        d.title = Some("COMMANDERS REPORT".into());
        d.button(10, 6, 40, "&OK", B_OK, ButtonLook::Normal);
        d.list(Rect::new(4, 24, 192, 80), rows, 0, L_MAIN, false);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_OK);
        self.dialogs.push(d);
    }

    fn record_commanders(&mut self, winner: Option<PlayerId>) {
        let Screen::Play(ps) = &self.screen else { return };
        let mut log: BTreeMap<String, [u32; 3]> =
            self.storage.get("empire.commanders").and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default();
        for p in ps.game.players.iter().filter(|p| p.is_human()) {
            let e = log.entry(p.name.clone()).or_insert([0, 0, 0]);
            e[0] += 1;
            if winner == Some(p.id) {
                e[1] += 1;
            } else {
                e[2] += 1;
            }
        }
        let s = serde_json::to_string(&log).unwrap_or_default();
        self.store("empire.commanders", Some(s));
    }

    fn open_about(&mut self) {
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let mut d = Dialog::centered(Dk::About, DlgStyle::Box, 230, 100, w, h);
        let lines = [
            format!("EMPIRE, VERSION {VERSION}"),
            "AFTER THE 1987 GAME BY WALTER BRIGHT".into(),
            "AND MARK BALDWIN (INTERSTEL).".into(),
            "NOT AFFILIATED WITH THEM.".into(),
            "".into(),
            "RUST + WEBASSEMBLY".into(),
        ];
        for (i, l) in lines.iter().enumerate() {
            d.centered_text(&self.font, 6 + i as i32 * 9, l.clone(), BLACK);
        }
        d.button(95, 80, 40, "&OK", B_OK, ButtonLook::Normal);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_OK);
        self.dialogs.push(d);
    }

    fn open_view(&mut self) {
        let Screen::Play(ps) = &self.screen else { return };
        let n = ps.game.players.len();
        let (w, h) = (self.frame.width as i32, self.frame.height as i32);
        let dw = 60 + n as i32 * 50;
        let mut d = Dialog::centered(Dk::View, DlgStyle::Panel, dw.max(180), 64, w, h);
        d.title = Some("VIEW".into());
        d.button(8, 20, 50, "&World", B_WORLD, if ps.reveal_all { ButtonLook::Selected } else { ButtonLook::Normal });
        for i in 0..n {
            let look = if !ps.reveal_all && ps.viewer as usize == i + 1 { ButtonLook::Selected } else { ButtonLook::Normal };
            d.button(62 + i as i32 * 48, 20, 44, &format!("P&{}", i + 1), B_VIEWP + i as u32 + 1, look);
        }
        let can_continue = !ps.game.game_over;
        d.button(8, 42, 80, "&Continue", B_CONTINUE, if can_continue { ButtonLook::Normal } else { ButtonLook::Disabled });
        d.button(dw.max(180) - 52, 42, 44, "&OK", B_OK, ButtonLook::Normal);
        d.default_button = Some(B_OK);
        d.cancel_button = Some(B_OK);
        self.dialogs.push(d);
    }

    // ------------------------------------------------------------------
    // Dialog events. Return true to keep the dialog open.

    fn dialog_event(&mut self, d: &mut Dialog<Dk>, ev: DlgEvent) -> bool {
        let kind = d.kind.clone();
        match kind {
            Dk::Alert | Dk::Info | Dk::WarReport | Dk::ShipReport | Dk::CombatReport | Dk::WorldMap | Dk::Commanders | Dk::About => {
                !matches!(ev, DlgEvent::Button(_))
            }
            Dk::Opponents => self.on_opponents(ev),
            Dk::Options { sel } => self.on_options(sel, ev),
            Dk::WorldMapChoice => self.on_world_map_choice(ev),
            Dk::MapSelect { names } => self.on_map_select(d, &names, ev),
            Dk::Name { idx } => {
                if let DlgEvent::Submit = ev {
                    let name = d.input_text().unwrap_or("").trim().to_string();
                    if name.is_empty() {
                        return true;
                    }
                    if let Some(s) = &mut self.setup {
                        s.players[idx].name = name;
                    }
                    self.open_report(idx);
                    return false;
                }
                true
            }
            Dk::Report { idx } => {
                if let DlgEvent::Button(_) = ev {
                    self.next_name_or_start(idx + 1);
                    return false;
                }
                true
            }
            Dk::Production { city } => self.on_production(d, city, ev),
            Dk::UnitProduced { city, unit } => {
                let name = d.input_text().map(|s| s.trim().to_string());
                let rename = |app: &mut Self| {
                    if let (Some(n), Screen::Play(ps)) = (name.clone(), &mut app.screen) {
                        if !n.is_empty() && ps.game.units[unit].alive {
                            ps.game.units[unit].name = n;
                        }
                    }
                };
                match ev {
                    DlgEvent::Button(B_PROD) => {
                        rename(self);
                        self.open_production(city);
                        false
                    }
                    DlgEvent::Button(_) | DlgEvent::Submit => {
                        rename(self);
                        false
                    }
                    _ => true,
                }
            }
            Dk::Display { s } => self.on_display(s, ev),
            Dk::ConfirmRisky { unit, dir } => match ev {
                DlgEvent::Button(B_OK) => {
                    if let Screen::Play(ps) = &mut self.screen {
                        let mut fx = Vec::new();
                        ps.try_move(unit, dir, true, &mut fx);
                        self.apply_effects(fx);
                    }
                    false
                }
                DlgEvent::Button(_) => false,
                _ => true,
            },
            Dk::ConfirmEndTurn => match ev {
                DlgEvent::Button(B_OK) => {
                    if let Screen::Play(ps) = &mut self.screen {
                        ps.end_turn_now();
                    }
                    false
                }
                DlgEvent::Button(_) => false,
                _ => true,
            },
            Dk::ConfirmExit => match ev {
                DlgEvent::Button(B_OK) => {
                    self.screen = Screen::Desktop;
                    self.desktop_message = "WELCOME TO EMPIRE".into();
                    false
                }
                DlgEvent::Button(_) => false,
                _ => true,
            },
            Dk::SaveAs => match ev {
                DlgEvent::Button(B_OK) | DlgEvent::Submit => {
                    let name = d.input_text().unwrap_or("").trim().to_uppercase();
                    if name.is_empty() {
                        return true;
                    }
                    self.save_game(&name);
                    false
                }
                DlgEvent::Button(B_DOWNLOAD) => {
                    let name = d.input_text().unwrap_or("").trim().to_lowercase();
                    let name = if name.is_empty() { "empire".to_string() } else { name.replace(' ', "_") };
                    if let Screen::Play(ps) = &self.screen {
                        let text = ps.game.to_save();
                        self.requests.push(HostRequest::Download { filename: format!("{name}.emp"), text });
                    }
                    false
                }
                DlgEvent::Button(_) => false,
                _ => true,
            },
            Dk::LoadList { keys } => match ev {
                DlgEvent::Button(B_UPLOAD) => {
                    self.requests.push(HostRequest::Upload { accept: ".json,.emp,.emap,.txt".into() });
                    false
                }
                DlgEvent::Button(B_OK) | DlgEvent::ListActivate(..) => {
                    let sel = list_sel(d);
                    if let Some(k) = keys.get(sel) {
                        if let Some(text) = self.storage.get(k).cloned() {
                            match Game::from_save(&text) {
                                Ok(g) => {
                                    let name = (k != "empire.autosave").then(|| Self::save_label(k));
                                    self.start_play(g, name);
                                }
                                Err(e) => self.alert(&format!("THAT SAVE IS DAMAGED: {e}")),
                            }
                        }
                    }
                    false
                }
                DlgEvent::Button(_) => false,
                _ => true,
            },
            Dk::DeleteList { keys } => match ev {
                DlgEvent::Button(B_OK) => {
                    let sel = list_sel(d);
                    if let Some(k) = keys.get(sel).cloned() {
                        self.store(&k, None);
                        self.dialogs.push(d.clone());
                        self.dialogs.pop();
                        self.open_delete();
                    }
                    false
                }
                DlgEvent::Button(_) => false,
                _ => true,
            },
            Dk::HandOff => {
                if let DlgEvent::Button(_) = ev {
                    if let Screen::Play(ps) = &mut self.screen {
                        ps.game.ack_handoff();
                        ps.pending = None;
                    }
                    return false;
                }
                true
            }
            Dk::Surrender => match ev {
                DlgEvent::Button(b @ (B_ACCEPT | B_REFUSE)) => {
                    if let Screen::Play(ps) = &mut self.screen {
                        ps.game.answer_surrender(b == B_ACCEPT);
                        ps.pending = None;
                    }
                    false
                }
                _ => true,
            },
            Dk::GameOver => !matches!(ev, DlgEvent::Button(_)),
            Dk::EdValidate | Dk::EdWorldView => !matches!(ev, DlgEvent::Button(_)),
            Dk::EdSprinkle => match ev {
                DlgEvent::Button(B_OK) => {
                    if let Screen::Editor(es) = &mut self.screen {
                        es.sprinkle();
                    }
                    false
                }
                DlgEvent::Button(_) => false,
                _ => true,
            },
            Dk::EdExit => match ev {
                DlgEvent::Button(B_OK) => {
                    self.screen = Screen::Desktop;
                    false
                }
                DlgEvent::Button(_) => false,
                _ => true,
            },
            Dk::EdSaveAs => match ev {
                DlgEvent::Button(B_OK) | DlgEvent::Submit => {
                    let name = d.input_text().unwrap_or("").trim().to_uppercase();
                    if name.is_empty() {
                        return true;
                    }
                    self.save_map(&name);
                    false
                }
                DlgEvent::Button(B_DOWNLOAD) => {
                    if let Screen::Editor(es) = &self.screen {
                        let name = d.input_text().unwrap_or("").trim().to_lowercase().replace(' ', "_");
                        let name = if name.is_empty() { "world".to_string() } else { name };
                        let text = es.map.to_text();
                        self.requests.push(HostRequest::Download { filename: format!("{name}.emap"), text });
                    }
                    false
                }
                DlgEvent::Button(_) => false,
                _ => true,
            },
            Dk::EdLoadList { names } => match ev {
                DlgEvent::Button(B_UPLOAD) => {
                    self.requests.push(HostRequest::Upload { accept: ".emap,.txt".into() });
                    false
                }
                DlgEvent::Button(B_OK) | DlgEvent::ListActivate(..) => {
                    let sel = list_sel(d);
                    if let Some(name) = names.get(sel) {
                        if let Some(text) = self.map_text(name) {
                            match WorldMap::parse(&text) {
                                Ok(map) => {
                                    let (w, h) = (self.frame.width as i32, self.frame.height as i32);
                                    let s = &self.settings;
                                    let params = MapGenParams {
                                        width: map.geom.width,
                                        height: map.geom.height,
                                        land_fraction: s.land_percent as f64 / 100.0,
                                        ..Default::default()
                                    };
                                    let clean = name.trim_end_matches(" (YOURS)").to_string();
                                    let mut es = EditorState::from_map(map, Some(clean), params, self.now as u64 + 1);
                                    es.zoom = self.zoom;
                                    let lay = es.layout(w, h);
                                    let c = es.cursor;
                                    es.center_on(c, &lay);
                                    self.screen = Screen::Editor(Box::new(es));
                                }
                                Err(e) => self.alert(&format!("BAD MAP: {e}")),
                            }
                        }
                    }
                    false
                }
                DlgEvent::Button(_) => false,
                _ => true,
            },
            Dk::View => match ev {
                DlgEvent::Button(B_WORLD) => {
                    if let Screen::Play(ps) = &mut self.screen {
                        ps.reveal_all = true;
                    }
                    self.dialogs.push(d.clone());
                    self.dialogs.pop();
                    self.open_view();
                    false
                }
                DlgEvent::Button(b) if b > B_VIEWP => {
                    if let Screen::Play(ps) = &mut self.screen {
                        ps.reveal_all = false;
                        ps.viewer = (b - B_VIEWP) as PlayerId;
                    }
                    self.open_view();
                    false
                }
                DlgEvent::Button(B_CONTINUE) => {
                    if let Screen::Play(ps) = &mut self.screen {
                        ps.mode = Mode::Computer;
                        ps.pending = None;
                    }
                    false
                }
                DlgEvent::Button(_) => false,
                _ => true,
            },
        }
    }

    fn on_opponents(&mut self, ev: DlgEvent) -> bool {
        let Some(setup) = &mut self.setup else { return false };
        match ev {
            DlgEvent::Button(B_CANCEL) => {
                self.setup = None;
                false
            }
            DlgEvent::Button(B_OK) => {
                let picked: Vec<PType> = setup.kinds.iter().copied().filter(|k| *k != PType::Nobody).collect();
                setup.players = picked
                    .iter()
                    .enumerate()
                    .map(|(i, k)| SetupPlayer {
                        kind: *k,
                        name: match k {
                            PType::Human => String::new(),
                            PType::Standard => format!("COMPUTER {}", i + 1),
                            PType::Expert => format!("EXPERT {}", i + 1),
                            PType::Nobody => String::new(),
                        },
                        production: 50,
                        combat: 50,
                    })
                    .collect();
                self.open_options(0);
                false
            }
            DlgEvent::Button(b) if b >= B_OPP => {
                let p = ((b - B_OPP) / 10) as usize;
                let i = (b - B_OPP) % 10;
                if p < setup.kinds.len() {
                    setup.kinds[p] = Self::opponent_kind_for(p, i);
                }
                self.open_opponents();
                false
            }
            DlgEvent::Key(k) => {
                if let Key::Char(c @ '1'..='3') = k.key {
                    let p = (c as u8 - b'1') as usize;
                    if p < setup.kinds.len() {
                        // Only player 3 may be left out.
                        let order = if p < 2 {
                            vec![PType::Human, PType::Standard, PType::Expert]
                        } else {
                            vec![PType::Human, PType::Standard, PType::Expert, PType::Nobody]
                        };
                        let cur = order.iter().position(|x| *x == setup.kinds[p]).unwrap_or(0);
                        setup.kinds[p] = order[(cur + 1) % order.len()];
                        self.open_opponents();
                        return false;
                    }
                }
                true
            }
            _ => true,
        }
    }

    fn on_options(&mut self, sel: usize, ev: DlgEvent) -> bool {
        let Some(setup) = &mut self.setup else { return false };
        let n = setup.players.len();
        let mut sel = sel;
        let adjust = |which: u8, delta: i32, sel: usize, setup: &mut Setup| {
            let p = &mut setup.players[sel];
            let v = if which == 0 { &mut p.production } else { &mut p.combat };
            *v = (*v as i32 + delta).clamp(1, 100) as u8;
        };
        match ev {
            DlgEvent::Button(B_CANCEL) => {
                self.setup = None;
                return false;
            }
            DlgEvent::Button(B_OK) => {
                self.open_world_map_choice();
                return false;
            }
            DlgEvent::Slider(id, delta) => {
                let p = (id / 10) as usize;
                let which = if id % 10 == S_PROD { 0 } else { 1 };
                if p < n {
                    sel = p;
                    adjust(which, delta, p, setup);
                }
            }
            DlgEvent::Key(k) => match k.key {
                Key::Char(c @ '1'..='6') if ((c as u8 - b'1') as usize) < n => sel = (c as u8 - b'1') as usize,
                Key::F(f @ 1..=4) => {
                    let step = if k.shift { 10 } else { 1 };
                    let (which, delta) = match f {
                        1 => (0, -step),
                        2 => (0, step),
                        3 => (1, -step),
                        _ => (1, step),
                    };
                    adjust(which, delta, sel, setup);
                }
                _ => return true,
            },
            _ => return true,
        }
        self.open_options(sel);
        false
    }

    fn on_world_map_choice(&mut self, ev: DlgEvent) -> bool {
        match ev {
            DlgEvent::Button(B_RANDOM) => {
                let s = &self.settings;
                let params = MapGenParams {
                    width: s.map_width,
                    height: s.map_height,
                    land_fraction: s.land_percent as f64 / 100.0,
                    ..Default::default()
                };
                self.create_game(MapSource::Random(params));
                false
            }
            DlgEvent::Button(B_SELECT) => {
                self.open_map_select();
                false
            }
            DlgEvent::Button(B_SIZE) => {
                let cur = (self.settings.map_width, self.settings.map_height);
                let i = MAP_SIZES.iter().position(|&s| s == cur).map(|i| (i + 1) % MAP_SIZES.len()).unwrap_or(2);
                (self.settings.map_width, self.settings.map_height) = MAP_SIZES[i];
                self.save_settings();
                self.open_world_map_choice();
                false
            }
            DlgEvent::Button(B_LAND) => {
                let steps = [18u8, 22, 27, 32, 38, 45, 55];
                let i = steps.iter().position(|&s| s == self.settings.land_percent).map(|i| (i + 1) % steps.len()).unwrap_or(2);
                self.settings.land_percent = steps[i];
                self.save_settings();
                self.open_world_map_choice();
                false
            }
            DlgEvent::Button(B_CANCEL) => {
                self.setup = None;
                false
            }
            _ => true,
        }
    }

    fn on_map_select(&mut self, d: &Dialog<Dk>, names: &[String], ev: DlgEvent) -> bool {
        match ev {
            DlgEvent::Button(B_OK) | DlgEvent::ListActivate(..) => {
                let sel = list_sel(d);
                match names.get(sel).and_then(|n| self.map_text(n)) {
                    Some(text) => {
                        self.create_game(MapSource::Text(text));
                        false
                    }
                    None => true,
                }
            }
            DlgEvent::Button(B_UPLOAD) => {
                self.requests.push(HostRequest::Upload { accept: ".emap,.txt".into() });
                true
            }
            DlgEvent::Button(B_CANCEL) => {
                self.open_world_map_choice();
                false
            }
            _ => true,
        }
    }

    fn create_game(&mut self, map: MapSource) {
        let Some(setup) = &mut self.setup else { return };
        self.seed_counter += 1;
        let seed = (self.now as u64).wrapping_mul(2654435761).wrapping_add(self.seed_counter * 97 + 13);
        let players: Vec<PlayerConfig> = setup
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut pc = match p.kind {
                    PType::Human => PlayerConfig::human(&format!("PLAYER {}", i + 1)),
                    PType::Standard => PlayerConfig::computer(&p.name, Skill::Standard),
                    _ => PlayerConfig::computer(&p.name, Skill::Expert),
                };
                pc.production_eff = p.production;
                pc.combat_eff = p.combat;
                pc
            })
            .collect();
        let config = GameConfig { seed, map, players, ..Default::default() };
        match Game::new(&config) {
            Ok(g) => {
                let n = g.cities.len();
                setup.game = Some(g);
                self.desktop_message = format!("{n} CITIES ARE TO BE CONQUERED.");
                self.next_name_or_start(0);
            }
            Err(e) => {
                self.alert(&format!("CAN'T START THAT GAME: {e}"));
            }
        }
    }

    fn next_name_or_start(&mut self, from: usize) {
        let Some(setup) = &mut self.setup else { return };
        let next = (from..setup.players.len()).find(|&i| setup.players[i].kind == PType::Human);
        match next {
            Some(i) => {
                setup.name_idx = i;
                self.open_name(i);
            }
            None => {
                let mut setup = self.setup.take().unwrap();
                if let Some(mut g) = setup.game.take() {
                    for (i, p) in setup.players.iter().enumerate() {
                        if p.kind == PType::Human && !p.name.is_empty() {
                            g.players[i].name = p.name.clone();
                        }
                    }
                    self.start_play(g, None);
                }
            }
        }
    }

    /// Start playing an existing game (hosts, tools and tests).
    pub fn start_game(&mut self, game: Game) {
        self.start_play(game, None);
    }

    fn start_play(&mut self, game: Game, save_name: Option<String>) {
        let mut ps = PlayState::new(game);
        ps.save_name = save_name;
        ps.message_delay = self.settings.message_delay as f64 * 30.0;
        ps.battle_delay = self.settings.battle_delay as f64 * 30.0;
        ps.report_produced = self.settings.report_armies_fighters;
        ps.zoom = self.zoom;
        let lay = ps.layout(self.frame.width as i32, self.frame.height as i32);
        let c = ps.cursor;
        ps.center_on(c, &lay);
        self.dialogs.clear();
        self.menu = None;
        self.screen = Screen::Play(Box::new(ps));
    }

    fn save_game(&mut self, name: &str) {
        let Screen::Play(ps) = &mut self.screen else { return };
        let text = ps.game.to_save();
        ps.save_name = Some(name.to_string());
        ps.message = format!("GAME SAVED AS {name}");
        let key = format!("empire.save.{name}");
        self.store(&key, Some(text));
    }

    fn autosave(&mut self) {
        let Screen::Play(ps) = &self.screen else { return };
        if ps.game.game_over {
            return;
        }
        let text = ps.game.to_save();
        self.store("empire.autosave", Some(text));
    }

    fn on_production(&mut self, d: &mut Dialog<Dk>, city: usize, ev: DlgEvent) -> bool {
        match ev {
            DlgEvent::Button(B_OK) | DlgEvent::ListActivate(..) => {
                let sel = list_sel(d);
                let kind = UnitType::from_index(sel).unwrap_or(UnitType::Army);
                if let Screen::Play(ps) = &mut self.screen {
                    match ps.game.cmd_set_production(city, kind) {
                        Ok(()) => {
                            if matches!(ps.pending, Some(Pending::CityProduction { .. })) {
                                ps.pending = None;
                            }
                            ps.message = format!("{} WILL PRODUCE {}", ps.game.cities[city].name.to_uppercase(), kind.name().to_uppercase());
                            return false;
                        }
                        Err(e) => {
                            ps.message = e.to_uppercase();
                            self.sound("error");
                            return true;
                        }
                    }
                }
                false
            }
            DlgEvent::Button(B_CANCEL) => false,
            DlgEvent::Key(k) => {
                // Unit letters pick a row, as in the original.
                if let Some(c) = k.letter() {
                    if let Some(t) = UnitType::from_letter(c) {
                        if let Some((sel, _, _, _)) = d.list_mut(L_MAIN) {
                            *sel = t.index();
                        }
                    }
                }
                true
            }
            _ => true,
        }
    }

    fn on_display(&mut self, mut s: Settings, ev: DlgEvent) -> bool {
        match ev {
            DlgEvent::Button(B_OK) => {
                self.settings = s;
                self.save_settings();
                self.apply_settings();
                return false;
            }
            DlgEvent::Button(B_CANCEL) => {
                // Undo the live preview.
                self.apply_settings();
                return false;
            }
            DlgEvent::Slider(S_BATTLE, dl) => s.battle_delay = (s.battle_delay as i32 + dl).clamp(0, 100) as u8,
            DlgEvent::Slider(S_MSG, dl) => s.message_delay = (s.message_delay as i32 + dl).clamp(0, 100) as u8,
            DlgEvent::Button(B_REPORT_ON) | DlgEvent::Button(B_REPORT_OFF) => s.report_armies_fighters = !s.report_armies_fighters,
            DlgEvent::Button(B_LOOK) => s.hd = !s.hd,
            DlgEvent::Button(B_TEXT) => s.text_size = (s.text_size + 1) % TEXT_SIZES.len() as u8,
            DlgEvent::Button(B_ZOOM) => {
                let i = MAP_SQUARES.iter().position(|&m| m >= s.map_square).map(|i| (i + 1) % MAP_SQUARES.len()).unwrap_or(0);
                s.map_square = MAP_SQUARES[i];
            }
            DlgEvent::Button(B_SMOOTH) => s.smooth_icons = !s.smooth_icons,
            DlgEvent::Button(B_RES) => s.resolution = s.resolution.next(),
            DlgEvent::Button(B_ICONS) => {
                let avail: Vec<u8> = (1..=4).filter(|&i| self.tiles_original[i as usize - 1].is_some()).collect();
                let mut cycle = vec![0u8];
                cycle.extend(avail);
                let i = cycle.iter().position(|&x| x == s.icon_set).map(|i| (i + 1) % cycle.len()).unwrap_or(0);
                s.icon_set = cycle[i];
            }
            DlgEvent::Button(B_FONT) => {
                // High-res: smooth -> original -> built-in pixel. CGA: original <-> built-in.
                let has_font = self.font_original.is_some();
                if s.hd {
                    if s.smooth_text {
                        s.smooth_text = false;
                        s.original_font = has_font;
                    } else if s.original_font && has_font {
                        s.original_font = false;
                    } else {
                        s.smooth_text = true;
                    }
                } else {
                    s.original_font = !s.original_font && has_font;
                }
            }
            DlgEvent::Button(B_ASPECT) => s.cga_aspect = !s.cga_aspect,
            DlgEvent::Key(k) if matches!(k.key, Key::Left | Key::Right) => {
                let dl = if k.key == Key::Left { -10 } else { 10 };
                s.battle_delay = (s.battle_delay as i32 + dl).clamp(0, 100) as u8;
            }
            _ => return true,
        }
        // Preview the change immediately and rebuild the dialog for the new screen.
        let old = std::mem::replace(&mut self.settings, s.clone());
        self.apply_settings();
        self.settings = old;
        self.build_display(s);
        false
    }
}

fn first_enabled(m: &Menu<Cmd>) -> Option<usize> {
    m.items.iter().position(|it| !it.is_divider() && it.enabled)
}

fn list_sel<K>(d: &Dialog<K>) -> usize {
    d.elems
        .iter()
        .find_map(|e| match e {
            Elem::List { sel, .. } => Some(*sel),
            _ => None,
        })
        .unwrap_or(0)
}

/// Overall ratings as on the New Game Options screen: a player's rating rises
/// as their own efficiencies fall relative to their strongest opponent.
/// "Standard" computer players carry a built-in handicap worth 5 points.
fn ratings(players: &[SetupPlayer]) -> Vec<i32> {
    let strength = |p: &SetupPlayer| -> f64 {
        let base = (p.production as f64 + p.combat as f64) / 2.0;
        if p.kind == PType::Standard {
            base * 0.9
        } else {
            base
        }
    };
    players
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mine = strength(p);
            let best_opp = players.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, q)| strength(q)).fold(0.0, f64::max);
            (50.0 + (best_opp - mine) / 2.0).round().clamp(0.0, 100.0) as i32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_rating_matches_original() {
        let ps = vec![
            SetupPlayer { kind: PType::Human, name: String::new(), production: 50, combat: 50 },
            SetupPlayer { kind: PType::Standard, name: String::new(), production: 50, combat: 50 },
        ];
        // The original showed 47% / 52% for this setup.
        let r = ratings(&ps);
        assert!((47..=48).contains(&r[0]) && (52..=53).contains(&r[1]), "{r:?}");
    }
}
