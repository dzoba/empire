//! Drawing the world map from a player's point of view.

use crate::gfx::*;
use crate::theme::Theme;
use crate::tiles::{is_player_px, Tile, TileSet};
use empire_core::{Game, Loc, MapVal, Orders, PlayerId, Terrain, UnitType};

pub struct MapPainter<'a> {
    pub tiles: &'a TileSet,
    pub theme: &'a Theme,
    /// Smooth icon edges when drawing above 1x (high-resolution mode).
    pub smooth: bool,
}

/// What to draw in one map cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellArt {
    Border,
    Unknown,
    Sea,
    Land { coast: u8 },
    City { owner: PlayerId, occupied: bool },
    Unit { kind: UnitType, owner: PlayerId, over_sea: bool, sentry: bool, loaded: bool },
}

pub fn unit_tile_name(kind: UnitType, over_sea: bool, sentry: bool, loaded: bool) -> &'static str {
    match kind {
        UnitType::Army if sentry => "army_sentry",
        UnitType::Army => "army",
        UnitType::Fighter if over_sea => "fighter_sea",
        UnitType::Fighter => "fighter",
        UnitType::Destroyer => "destroyer",
        UnitType::Transport if loaded => "transport_loaded",
        UnitType::Transport => "transport",
        UnitType::Submarine => "submarine",
        UnitType::Cruiser => "cruiser",
        UnitType::Carrier if loaded => "carrier_loaded",
        UnitType::Carrier => "carrier",
        UnitType::Battleship => "battleship",
    }
}

/// Coast mask for a land cell from the viewer's knowledge: N=1, E=2, S=4, W=8.
pub fn coast_mask(game: &Game, view: &[MapVal], loc: Loc) -> u8 {
    let geom = game.geom();
    let mut m = 0;
    for (bit, d) in [(1u8, 2u8), (2, 0), (4, 6), (8, 4)] {
        if let Some(n) = geom.step(loc, d) {
            let v = view[n];
            let sea = v.is_sea() || (geom.is_border(n) && game.map.get(n) == Terrain::Sea);
            if sea {
                m |= bit;
            }
        }
    }
    m
}

/// Coast mask from the true terrain (used when the whole map is revealed).
pub fn coast_mask_terrain(game: &Game, loc: Loc) -> u8 {
    let geom = game.geom();
    let mut m = 0;
    for (bit, d) in [(1u8, 2u8), (2, 0), (4, 6), (8, 4)] {
        if let Some(n) = geom.step(loc, d) {
            if game.map.get(n) == Terrain::Sea {
                m |= bit;
            }
        }
    }
    m
}

/// Decide what the viewer sees at `loc`.
pub fn cell_art(game: &Game, viewer: PlayerId, loc: Loc, reveal_all: bool) -> CellArt {
    let geom = game.geom();
    if geom.is_border(loc) {
        return CellArt::Border;
    }
    let view = &game.player(viewer).view;
    let v = if reveal_all { game.truth(loc) } else { view[loc] };
    if v.is_unknown() {
        return CellArt::Unknown;
    }
    if v.is_city() {
        let occupied = v.owner() == viewer && !game.units_at(loc).is_empty();
        return CellArt::City { owner: v.owner(), occupied };
    }
    if let Some(kind) = v.unit_type() {
        let owner = v.owner();
        let (mut sentry, mut loaded) = (false, false);
        if owner == viewer || reveal_all {
            if let Some(id) = game.top_unit(loc) {
                let u = game.unit(id);
                if u.kind == kind && u.owner == owner {
                    sentry = kind == UnitType::Army && u.orders == Orders::Sentry;
                    loaded = kind.cargo().is_some() && !game.cargo(id).is_empty();
                }
            }
        }
        return CellArt::Unit { kind, owner, over_sea: v.is_sea(), sentry, loaded };
    }
    if v.is_sea() {
        CellArt::Sea
    } else if reveal_all {
        CellArt::Land { coast: coast_mask_terrain(game, loc) }
    } else {
        CellArt::Land { coast: coast_mask(game, view, loc) }
    }
}

impl MapPainter<'_> {
    fn cell(t: &Tile, recolor: Option<u8>, invert: bool) -> [u8; 64] {
        // EGA icons are 16 pixels wide for a 640-wide screen; squeeze to 8.
        let sx = if t.w >= 16 { t.w / 8 } else { 1 };
        let mut cell = [BLACK; 64];
        for ty in 0..8.min(t.h) {
            for tx in 0..8.min(t.w / sx) {
                let mut p = t.px[ty * t.w + tx * sx];
                if let Some(c) = recolor {
                    if is_player_px(p) {
                        p = c;
                    }
                }
                if invert {
                    p = if p == BLACK { WHITE } else if p == WHITE { BLACK } else { p };
                }
                cell[ty * 8 + tx] = p;
            }
        }
        cell
    }

    fn blit(&self, f: &mut Frame, x: i32, y: i32, t: &Tile, recolor: Option<u8>, invert: bool) {
        f.blit8(x, y, &Self::cell(t, recolor, invert), self.smooth);
    }

    /// Terrain: fine dither texture in high-resolution mode.
    fn terrain(&self, f: &mut Frame, x: i32, y: i32, name: &str) -> bool {
        let Some(t) = self.tiles.get(name) else { return false };
        let cell = Self::cell(t, None, false);
        if self.smooth {
            f.blit8_texture(x, y, &cell);
        } else {
            f.blit8(x, y, &cell, false);
        }
        true
    }

    fn named(&self, f: &mut Frame, x: i32, y: i32, name: &str) -> bool {
        if let Some(t) = self.tiles.get(name) {
            self.blit(f, x, y, t, None, false);
            true
        } else {
            false
        }
    }

    /// Player-specific tile (imported sets) or a recoloured generic one.
    fn player_tile(&self, f: &mut Frame, x: i32, y: i32, base: &str, owner: PlayerId) {
        let specific = format!("{base}_p{owner}");
        if let Some(t) = self.tiles.get(&specific) {
            self.blit(f, x, y, t, None, false);
            return;
        }
        // Fall back to player 1's art, recoloured.
        let generic = self.tiles.get(base).or_else(|| self.tiles.get(&format!("{base}_p1")));
        if let Some(t) = generic {
            self.blit(f, x, y, t, Some(self.theme.player(owner)), false);
        } else if base == "fighter_sea" {
            self.player_tile(f, x, y, "fighter", owner);
        } else if base.ends_with("_loaded") || base.ends_with("_sentry") || base == "city_occupied" {
            let plain = base.trim_end_matches("_loaded").trim_end_matches("_sentry");
            let plain = if plain == "city_occupied" { "city" } else { plain };
            self.player_tile(f, x, y, plain, owner);
        }
    }

    pub fn draw_art(&self, f: &mut Frame, x: i32, y: i32, art: CellArt) {
        match art {
            CellArt::Border => {
                if !self.terrain(f, x, y, "border") {
                    f.dither(crate::gfx::Rect::new(x, y, 8, 8), BLACK, WHITE);
                }
            }
            CellArt::Unknown => {
                if !self.terrain(f, x, y, "unknown") {
                    f.fill(crate::gfx::Rect::new(x, y, 8, 8), BLACK);
                }
            }
            CellArt::Sea => {
                self.terrain(f, x, y, "sea");
            }
            CellArt::Land { coast } => {
                if coast != 0 && self.terrain(f, x, y, &format!("land_coast_{coast}")) {
                    return;
                }
                self.terrain(f, x, y, "land");
                // Built-in sets draw white coastlines on top.
                if coast & 1 != 0 {
                    f.hline(x, x + 7, y, WHITE);
                }
                if coast & 2 != 0 {
                    f.vline(x + 7, y, y + 7, WHITE);
                }
                if coast & 4 != 0 {
                    f.hline(x, x + 7, y + 7, WHITE);
                }
                if coast & 8 != 0 {
                    f.vline(x, y, y + 7, WHITE);
                }
            }
            CellArt::City { owner: 0, .. } => {
                if !self.named(f, x, y, "city_neutral") {
                    if let Some(t) = self.tiles.get("city").or_else(|| self.tiles.get("city_p1")) {
                        self.blit(f, x, y, t, Some(WHITE), true);
                    }
                }
            }
            CellArt::City { owner, occupied } => {
                if occupied {
                    let specific = format!("city_occupied_p{owner}");
                    if self.named(f, x, y, &specific) {
                        return;
                    }
                    self.player_tile(f, x, y, "city", owner);
                    // Shade the background to show units inside.
                    for ty in 0..8 {
                        for tx in 0..8 {
                            if f.get(x + tx, y + ty) == BLACK && (tx + ty) % 2 == 0 {
                                f.put(x + tx, y + ty, self.theme.player(owner));
                            }
                        }
                    }
                } else {
                    self.player_tile(f, x, y, "city", owner);
                }
            }
            CellArt::Unit { kind, owner, over_sea, sentry, loaded } => {
                self.player_tile(f, x, y, unit_tile_name(kind, over_sea, sentry, loaded), owner);
            }
        }
    }

    /// The terrain under a cell, ignoring units (for blinking the active unit).
    pub fn terrain_art(game: &Game, viewer: PlayerId, loc: Loc) -> CellArt {
        let geom = game.geom();
        match game.map.get(loc) {
            Terrain::City => {
                let owner = game.city_at(loc).map(|c| game.cities[c].owner).unwrap_or(0);
                let occupied = owner == viewer && game.units_at(loc).len() > 1;
                CellArt::City { owner, occupied }
            }
            Terrain::Sea => CellArt::Sea,
            Terrain::Land => {
                let _ = geom;
                CellArt::Land { coast: coast_mask(game, &game.player(viewer).view, loc) }
            }
        }
    }

    pub fn draw_cursor(&self, f: &mut Frame, x: i32, y: i32, name: &str) {
        if self.named(f, x, y, name) {
            return;
        }
        match name {
            "cursor_survey" => {
                f.frame(crate::gfx::Rect::new(x, y, 8, 8), WHITE);
                f.frame(crate::gfx::Rect::new(x + 1, y + 1, 6, 6), BLACK);
                f.fill(crate::gfx::Rect::new(x + 3, y + 3, 2, 2), WHITE);
            }
            _ => {
                f.fill(crate::gfx::Rect::new(x, y, 8, 8), BLACK);
                f.hline(x + 1, x + 6, y + 3, WHITE);
                f.hline(x + 1, x + 6, y + 4, WHITE);
                f.vline(x + 3, y + 1, y + 6, WHITE);
                f.vline(x + 4, y + 1, y + 6, WHITE);
            }
        }
    }
}
