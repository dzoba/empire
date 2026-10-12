//! Tile sets: 8x8 icons in a plain-text format.
//!
//! ```text
//! palette cga1
//! tile army
//! ...33...
//! ```
//! With `palette cga1`, digits 0-3 are black, cyan, magenta, white. With
//! `palette ega`, hex digits 0-f are the 16 EGA colours. The built-in set
//! below is original artwork in the CGA style; `tools/interstel_import` can
//! convert the icon sets from an original Interstel disk into this format,
//! and the game uses them when they are loaded.

use crate::gfx::{CGA1, WHITE};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Tile {
    pub w: usize,
    pub h: usize,
    /// EGA palette indices, row-major.
    pub px: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct TileSet {
    pub tiles: HashMap<String, Tile>,
    /// True for sets whose unit icons are drawn in white and recoloured per player.
    pub recolor_units: bool,
}

impl TileSet {
    pub fn parse(text: &str) -> Result<TileSet, String> {
        let mut set = TileSet { tiles: HashMap::new(), recolor_units: true };
        let mut palette_cga = true;
        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut words = line.split_whitespace();
            match words.next() {
                Some("palette") => {
                    palette_cga = !matches!(words.next(), Some("ega"));
                }
                Some("recolor") => {
                    set.recolor_units = !matches!(words.next(), Some("off") | Some("no") | Some("false"));
                }
                Some("tile") => {
                    let name = words.next().ok_or("tile without a name")?.to_string();
                    let w: usize = words.next().and_then(|s| s.parse().ok()).unwrap_or(8);
                    let h: usize = words.next().and_then(|s| s.parse().ok()).unwrap_or(8);
                    let mut px = Vec::with_capacity(w * h);
                    let mut rows = 0;
                    while rows < h {
                        let Some(row) = lines.next() else { return Err(format!("tile {name}: too few rows")) };
                        let row = row.trim();
                        if row.is_empty() || row.starts_with('#') {
                            continue;
                        }
                        let chars: Vec<char> = row.chars().collect();
                        if chars.len() < w {
                            return Err(format!("tile {name}: row too short"));
                        }
                        for &ch in &chars[..w] {
                            let v = ch.to_digit(16).ok_or_else(|| format!("tile {name}: bad pixel {ch:?}"))? as u8;
                            px.push(if palette_cga { CGA1[(v & 3) as usize] } else { v & 15 });
                        }
                        rows += 1;
                    }
                    set.tiles.insert(name, Tile { w, h, px });
                }
                Some(other) => return Err(format!("unknown directive {other:?}")),
                None => {}
            }
        }
        Ok(set)
    }

    pub fn get(&self, name: &str) -> Option<&Tile> {
        self.tiles.get(name)
    }

    pub fn builtin() -> TileSet {
        let mut s = TileSet::parse(BUILTIN).expect("built-in tiles parse");
        s.recolor_units = true;
        s
    }

    /// Use tiles from `other` where present, keeping ours as fallback.
    pub fn overlay(&mut self, other: TileSet) {
        self.recolor_units = other.recolor_units;
        for (k, v) in other.tiles {
            self.tiles.insert(k, v);
        }
    }
}

/// Is this pixel the "player colour" placeholder in a recolourable icon?
pub fn is_player_px(p: u8) -> bool {
    p == WHITE
}

/// Built-in CGA-style icons (original artwork). 0 black, 1 cyan, 2 magenta, 3 white.
/// Unit and city icons are drawn in white (3) and recoloured per player.
pub const BUILTIN: &str = r#"
palette cga1

tile unknown
00000000
00000000
00000000
00000000
00000000
00000000
00000000
00000000

tile sea
11111111
11111111
11111131
11111111
11111111
11311111
11111111
11111111

# Textured land: a cyan and black dither. Coastlines are drawn on top.
tile land
10101010
01010101
10101010
01010101
10101010
01010101
10101010
01010101

tile border
30303030
03030303
30303030
03030303
30303030
03030303
30303030
03030303

tile city
00000000
33003033
33033303
33033303
33333333
30330303
33333333
33333333

tile army
00033000
00333300
00033000
03333330
30033003
00033000
00300300
03300330

tile army_sentry
00000000
00033000
00300300
00300300
03033030
03033030
30033003
33333333

tile fighter
00033000
00033000
03333330
33333333
00033000
00033000
00333300
00000000

tile destroyer
00000000
00000000
00003000
00033000
03333330
33333333
00333300
00000000

tile transport
00000000
03333330
03333330
03333330
33333333
33333333
03333330
00000000

tile transport_loaded
00000000
03333330
03000030
03333330
33333333
33333333
03333330
00000000

tile submarine
00000000
00000000
00000000
00033000
00033000
03333330
33333333
00000000

tile cruiser
00000000
00003000
03033303
03333333
33333333
33333333
03333330
00000000

tile carrier
00000000
00000030
00000030
33333333
33333333
03333330
00333300
00000000

tile carrier_loaded
00000000
00000030
00000030
30303033
33333333
03333330
00333300
00000000

tile battleship
00030000
00333000
30333030
33333330
33333333
33333333
33333333
03333330

tile explosion
20020020
02020200
00222000
22222220
00222000
02020200
20020020
00000000

tile cursor_survey
30000003
03000030
00300300
00000000
00000000
00300300
03000030
30000003

tile cursor_direction
00033000
00033000
00000000
33000033
33000033
00000000
00033000
00033000
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_parses() {
        let s = TileSet::builtin();
        for name in [
            "unknown", "sea", "land", "border", "city", "army", "army_sentry", "fighter", "destroyer", "transport",
            "transport_loaded", "submarine", "cruiser", "carrier", "carrier_loaded", "battleship", "explosion",
        ] {
            let t = s.get(name).unwrap_or_else(|| panic!("missing {name}"));
            assert_eq!((t.w, t.h, t.px.len()), (8, 8, 64));
        }
    }
}
