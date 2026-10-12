//! The map editor ("Creating Your Own Worlds" in the manual).

use crate::commands::Cmd;
use crate::gfx::*;
use crate::input::KeyEvent;
use crate::layout::{PlayLayout, Zoom, TILE};
use crate::mapdraw::{CellArt, MapPainter};
use crate::text::Font;
use crate::theme::Theme;
use crate::ui;
use empire_core::mapgen::{self, MapGenParams};
use empire_core::{Geometry, Loc, Rng, Terrain, WorldMap};

/// Requests from the editor to the app.
#[derive(Clone, Debug, PartialEq)]
pub enum EditEffect {
    Validate(Vec<(Loc, String)>),
    WorldView,
    AskSprinkle,
    LoadMap,
    SaveMap,
    SaveMapAs,
    Exit,
    Sound(&'static str),
}

#[derive(Clone, Debug)]
struct Growth {
    terrain: Terrain,
    /// None grows until a key is pressed.
    budget: Option<usize>,
    cells: Vec<Loc>,
}

pub struct EditorState {
    pub map: WorldMap,
    pub name: Option<String>,
    pub cursor: Loc,
    pub origin: (usize, usize),
    growth: Option<Growth>,
    rng: Rng,
    pub dirty: bool,
    pub message: String,
    pub watch: bool,
    pub params: MapGenParams,
    /// Text and map scale; the app keeps this current.
    pub zoom: Zoom,
}

impl EditorState {
    pub fn new(width: usize, height: usize, land_fraction: f64, seed: u64) -> Self {
        let geom = Geometry::new(width.max(12), height.max(10));
        let map = WorldMap::new_filled(geom, Terrain::Sea);
        let params = MapGenParams { width: geom.width, height: geom.height, land_fraction, ..Default::default() };
        Self::from_map(map, None, params, seed)
    }

    pub fn from_map(map: WorldMap, name: Option<String>, params: MapGenParams, seed: u64) -> Self {
        let g = map.geom;
        let cursor = g.loc(g.height / 2, g.width / 2);
        let mut e = EditorState {
            map,
            name,
            cursor,
            origin: (0, 0),
            growth: None,
            rng: Rng::new(seed),
            dirty: false,
            message: String::new(),
            watch: true,
            zoom: Zoom::default(),
            params,
        };
        e.status();
        e
    }

    pub fn layout(&self, w: i32, h: i32) -> PlayLayout {
        let digits = (self.map.geom.height.max(self.map.geom.width) - 1).to_string().len() as i32;
        PlayLayout::new(w, h, digits.min(3), self.zoom)
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        let mut c = (0, 0, 0);
        for &t in &self.map.terrain {
            match t {
                Terrain::Sea => c.0 += 1,
                Terrain::Land => c.1 += 1,
                Terrain::City => c.2 += 1,
            }
        }
        c
    }

    pub fn status(&mut self) {
        let (s, l, c) = self.counts();
        let name = self.name.clone().unwrap_or_else(|| "UNTITLED".into());
        self.message = format!("MAP: {name}  COUNTS- SEA: {s} LAND: {l} CITIES: {c}");
    }

    // Viewport helpers.
    pub fn center_on(&mut self, loc: Loc, lay: &PlayLayout) {
        let g = self.map.geom;
        let oc = g.col(loc) as i64 - lay.cols as i64 / 2;
        let or = g.row(loc) as i64 - lay.rows as i64 / 2;
        self.origin = (
            oc.clamp(0, (g.width as i64 - lay.cols as i64).max(0)) as usize,
            or.clamp(0, (g.height as i64 - lay.rows as i64).max(0)) as usize,
        );
    }

    fn visible(&self, loc: Loc, lay: &PlayLayout) -> bool {
        let g = self.map.geom;
        let (r, c) = (g.row(loc), g.col(loc));
        c >= self.origin.0 && c < self.origin.0 + lay.cols && r >= self.origin.1 && r < self.origin.1 + lay.rows
    }

    pub fn scroll(&mut self, dc: i64, dr: i64, lay: &PlayLayout) {
        let g = self.map.geom;
        let c = (self.origin.0 as i64 + dc).clamp(0, (g.width as i64 - lay.cols as i64).max(0));
        let r = (self.origin.1 as i64 + dr).clamp(0, (g.height as i64 - lay.rows as i64).max(0));
        self.origin = (c as usize, r as usize);
    }

    pub fn loc_at_pixel(&self, x: i32, y: i32, lay: &PlayLayout) -> Option<Loc> {
        if !lay.map.contains(x, y) {
            return None;
        }
        let (dc, dr) = lay.cell_at(x, y)?;
        let (c, r) = (self.origin.0 + dc, self.origin.1 + dr);
        let g = self.map.geom;
        (c < g.width && r < g.height).then(|| g.loc(r, c))
    }

    fn paint(&mut self, t: Terrain) {
        if self.map.geom.is_border(self.cursor) && t == Terrain::City {
            return;
        }
        self.map.set(self.cursor, t);
        self.dirty = true;
        self.status();
    }

    fn start_growth(&mut self, terrain: Terrain, budget: Option<usize>) {
        if !self.map.geom.is_border(self.cursor) && self.map.get(self.cursor) != Terrain::City {
            self.map.set(self.cursor, terrain);
        }
        let label = match (terrain, budget) {
            (Terrain::Land, None) => "GROWING LAND - PRESS ANY KEY TO STOP",
            (_, None) => "GROWING SEA - PRESS ANY KEY TO STOP",
            (Terrain::Land, _) => "BLOTCHING LAND",
            _ => "BLOTCHING SEA",
        };
        self.message = label.into();
        self.growth = Some(Growth { terrain, budget, cells: vec![self.cursor] });
        self.dirty = true;
    }

    pub fn growing(&self) -> bool {
        self.growth.is_some()
    }

    /// Advance a running growth.
    pub fn tick(&mut self, lay: &PlayLayout) {
        let Some(mut g) = self.growth.take() else { return };
        let geom = self.map.geom;
        let per_tick = 4;
        for _ in 0..per_tick {
            if g.cells.is_empty() || g.budget == Some(0) {
                break;
            }
            let from = g.cells[self.rng.below_usize(g.cells.len())];
            let d = if self.rng.below(4) == 0 { self.rng.below(8) as u8 } else { (self.rng.below(4) * 2) as u8 };
            let Some(to) = geom.step(from, d) else { continue };
            if geom.is_border(to) || self.map.get(to) == g.terrain || self.map.get(to) == Terrain::City {
                continue;
            }
            self.map.set(to, g.terrain);
            g.cells.push(to);
            if let Some(b) = &mut g.budget {
                *b -= 1;
            }
            if self.watch && !self.visible(to, lay) {
                self.center_on(to, lay);
            }
        }
        if g.budget == Some(0) {
            self.status();
        } else {
            self.growth = Some(g);
        }
    }

    pub fn key(&mut self, ev: KeyEvent, lay: &PlayLayout) -> Vec<EditEffect> {
        if self.growth.take().is_some() {
            self.status();
            return Vec::new();
        }
        if let Some(d) = ev.direction() {
            let steps = if ev.shift { 5 } else { 1 };
            for _ in 0..steps {
                if let Some(n) = self.map.geom.step(self.cursor, d) {
                    self.cursor = n;
                }
            }
            if !self.visible(self.cursor, lay) {
                self.center_on(self.cursor, lay);
            }
            let g = self.map.geom;
            let what = match self.map.get(self.cursor) {
                Terrain::Sea => "SEA",
                Terrain::Land => "LAND",
                Terrain::City => "CITY",
            };
            self.message = format!("LOC: {},{} - {what}", g.row(self.cursor), g.col(self.cursor));
            return Vec::new();
        }
        match crate::commands::editor_shortcut(&ev) {
            Some(c) => self.command(c, lay),
            None => Vec::new(),
        }
    }

    pub fn command(&mut self, cmd: Cmd, lay: &PlayLayout) -> Vec<EditEffect> {
        use Cmd::*;
        let mut fx = Vec::new();
        match cmd {
            EdSea => self.paint(Terrain::Sea),
            EdLand => self.paint(Terrain::Land),
            EdCity => self.paint(Terrain::City),
            EdAllSea | EdAllLand => {
                let t = if cmd == EdAllSea { Terrain::Sea } else { Terrain::Land };
                for x in self.map.terrain.iter_mut() {
                    *x = t;
                }
                self.dirty = true;
                self.status();
            }
            EdGrowLand => self.start_growth(Terrain::Land, None),
            EdGrowSea => self.start_growth(Terrain::Sea, None),
            EdBlotchLand | EdBlotchSea => {
                let n = 15 + self.rng.below(40) as usize;
                let t = if cmd == EdBlotchLand { Terrain::Land } else { Terrain::Sea };
                self.start_growth(t, Some(n));
            }
            EdBuildWorld => {
                let mut rng = Rng::new(self.rng.next_u32() as u64);
                mapgen::build_landmasses(&mut self.map, &self.params, &mut rng);
                self.dirty = true;
                self.status();
                fx.push(EditEffect::AskSprinkle);
            }
            EdSprinkle => self.sprinkle(),
            EdWatchBuild => {
                self.watch = !self.watch;
                self.message = if self.watch { "WATCH BUILD IS ON".into() } else { "WATCH BUILD IS OFF".into() };
            }
            EdValidate => fx.push(EditEffect::Validate(mapgen::validate(&self.map))),
            EdWorldView => fx.push(EditEffect::WorldView),
            EdLoadMap => fx.push(EditEffect::LoadMap),
            EdSaveMap => fx.push(EditEffect::SaveMap),
            EdSaveMapAs => fx.push(EditEffect::SaveMapAs),
            EdExit => fx.push(EditEffect::Exit),
            CenterScreen => self.center_on(self.cursor, lay),
            CenterCursor => {
                let g = self.map.geom;
                let r = (self.origin.1 + lay.rows / 2).min(g.height - 1);
                let c = (self.origin.0 + lay.cols / 2).min(g.width - 1);
                self.cursor = g.loc(r, c);
            }
            ScrollLeft => self.scroll(-1, 0, lay),
            ScrollRight => self.scroll(1, 0, lay),
            ScrollUp => self.scroll(0, -1, lay),
            ScrollDown => self.scroll(0, 1, lay),
            PageLeft => self.scroll(-(lay.cols as i64) / 2, 0, lay),
            PageRight => self.scroll(lay.cols as i64 / 2, 0, lay),
            PageUp => self.scroll(0, -(lay.rows as i64) / 2, lay),
            PageDown => self.scroll(0, lay.rows as i64 / 2, lay),
            _ => fx.push(EditEffect::Sound("error")),
        }
        fx
    }

    pub fn sprinkle(&mut self) {
        let mut rng = Rng::new(self.rng.next_u32() as u64);
        let mut params = self.params.clone();
        params.width = self.map.geom.width;
        params.height = self.map.geom.height;
        mapgen::sprinkle(&mut self.map, &params, &mut rng);
        self.dirty = true;
        self.status();
    }

    pub fn click(&mut self, x: i32, y: i32, lay: &PlayLayout) {
        if lay.up_box.contains(x, y) {
            self.scroll(0, -1, lay);
        } else if lay.down_box.contains(x, y) {
            self.scroll(0, 1, lay);
        } else if lay.left_box.contains(x, y) {
            self.scroll(-1, 0, lay);
        } else if lay.right_box.contains(x, y) {
            self.scroll(1, 0, lay);
        } else if let Some(l) = self.loc_at_pixel(x, y, lay) {
            self.cursor = l;
            self.growth = None;
        }
    }

    fn art(&self, loc: Loc) -> CellArt {
        let g = self.map.geom;
        if g.is_border(loc) {
            return match self.map.get(loc) {
                Terrain::Sea => CellArt::Border,
                _ => CellArt::Land { coast: 0 },
            };
        }
        match self.map.get(loc) {
            Terrain::Sea => CellArt::Sea,
            Terrain::City => CellArt::City { owner: 0, occupied: false },
            Terrain::Land => CellArt::Land { coast: coast_mask_from(&self.map, loc) },
        }
    }

    pub fn draw(&self, f: &mut Frame, mf: &mut Frame, font: &Font, theme: &Theme, painter: &MapPainter, lay: &PlayLayout, blink: bool) {
        let geom = self.map.geom;
        let max_chars = ((lay.width - 8) / font.advance) as usize;
        let msg: String = self.message.chars().take(max_chars).collect();
        font.draw(f, 4, lay.msg_y, &msg, theme.text, None);
        f.hline(0, lay.width - 1, lay.rule2_y, theme.rule);
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
        f.fill(lay.row_strip, theme.strip_bg);
        f.fill(lay.col_strip, theme.strip_bg);
        crate::play::draw_strip_numbers(f, font, theme, lay, self.origin, (geom.width, geom.height));
        f.fill(lay.map, BLACK);
        let (mw, mh, mscale) = lay.map_picture();
        mf.resize(mw, mh, mscale);
        mf.clear(BLACK);
        for dr in 0..lay.rows {
            for dc in 0..lay.cols {
                let (r, c) = (self.origin.1 + dr, self.origin.0 + dc);
                if r >= geom.height || c >= geom.width {
                    continue;
                }
                let loc = geom.loc(r, c);
                painter.draw_art(mf, dc as i32 * TILE, dr as i32 * TILE, self.art(loc));
            }
        }
        if blink && self.visible(self.cursor, lay) {
            let x = (geom.col(self.cursor) - self.origin.0) as i32 * TILE;
            let y = (geom.row(self.cursor) - self.origin.1) as i32 * TILE;
            painter.draw_cursor(mf, x, y, "cursor_survey");
        }
        let u = lay.ui_scale;
        f.paste(mf, lay.map.x * u, lay.map.y * u, lay.map.w * u, lay.map.h * u);
    }
}

fn coast_mask_from(map: &WorldMap, loc: Loc) -> u8 {
    let geom = map.geom;
    let mut m = 0;
    for (bit, d) in [(1u8, 2u8), (2, 0), (4, 6), (8, 4)] {
        if let Some(n) = geom.step(loc, d) {
            if map.get(n) == Terrain::Sea {
                m |= bit;
            }
        }
    }
    m
}
