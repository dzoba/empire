//! Fonts.
//!
//! Layouts assume the original game's text grid: a 4x8 logical cell, 80
//! columns at 320 pixels. Two kinds of font fill that grid:
//!
//! * bitmap fonts: the built-in [`small_font`] (original artwork with the
//!   original's metrics) or an imported Interstel font (`.font` text format,
//!   see `tools/interstel_import`), drawn pixel for pixel;
//! * the smooth font ([`smooth_font`], IBM Plex Mono), rasterised at the
//!   screen's real resolution and sized so each character advances exactly
//!   one cell, so every layout written for the original still fits.

use crate::gfx::{Frame, Rect, BLEND_LEVELS};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Bundled smooth font (SIL Open Font License, see assets/fonts/OFL.txt).
const PLEX_MONO: &[u8] = include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf");

type GlyphImage = (fontdue::Metrics, Vec<u8>);

pub struct VectorFont {
    font: fontdue::Font,
    /// Em size in logical units, chosen so one character advances one cell.
    em_units: f32,
    /// Baseline offset from the top of the cell, in logical units.
    baseline_units: f32,
    cache: RefCell<HashMap<(char, u32), GlyphImage>>,
}

impl std::fmt::Debug for VectorFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VectorFont(em {} units)", self.em_units)
    }
}

impl VectorFont {
    fn glyph(&self, ch: char, px: f32) -> GlyphImage {
        let key = (ch, (px * 8.0) as u32);
        if let Some(g) = self.cache.borrow().get(&key) {
            return g.clone();
        }
        let g = self.font.rasterize(ch, px);
        let mut cache = self.cache.borrow_mut();
        if cache.len() > 2048 {
            cache.clear();
        }
        cache.insert(key, g.clone());
        g
    }
}

#[derive(Clone, Debug)]
pub struct Font {
    pub cell_w: i32,
    pub cell_h: i32,
    /// Monospace advance in logical pixels.
    pub advance: i32,
    /// Bitmap glyphs for codepoints 0..128: one Vec of rows, bit i = column i.
    pub glyphs: Vec<Option<Vec<u16>>>,
    pub uppercase_only: bool,
    /// When set, text is rasterised from this font instead of the bitmaps.
    pub vector: Option<Rc<VectorFont>>,
}

impl Font {
    pub fn glyph(&self, ch: char) -> Option<&Vec<u16>> {
        let mut c = ch;
        if self.uppercase_only {
            c = c.to_ascii_uppercase();
        }
        let code = c as usize;
        if code < self.glyphs.len() {
            if let Some(g) = &self.glyphs[code] {
                return Some(g);
            }
        }
        if c.is_ascii_lowercase() {
            let up = c.to_ascii_uppercase() as usize;
            return self.glyphs.get(up).and_then(|g| g.as_ref());
        }
        None
    }

    pub fn width(&self, s: &str) -> i32 {
        s.chars().count() as i32 * self.advance
    }

    pub fn draw(&self, f: &mut Frame, x: i32, y: i32, s: &str, fg: u8, bg: Option<u8>) -> i32 {
        let mut cx = x;
        for ch in s.chars() {
            self.draw_char(f, cx, y, ch, fg, bg);
            cx += self.advance;
        }
        cx
    }

    pub fn draw_char(&self, f: &mut Frame, x: i32, y: i32, ch: char, fg: u8, bg: Option<u8>) {
        if let Some(bg) = bg {
            f.fill(Rect::new(x, y, self.advance, self.cell_h), bg);
        }
        if let Some(v) = &self.vector {
            self.draw_vector(v, f, x, y, ch, fg);
            return;
        }
        if let Some(rows) = self.glyph(ch) {
            for (ry, bits) in rows.iter().enumerate() {
                for cx in 0..self.cell_w {
                    if bits & (1 << cx) != 0 {
                        f.put(x + cx, y + ry as i32, fg);
                    }
                }
            }
        }
    }

    fn draw_vector(&self, v: &VectorFont, f: &mut Frame, x: i32, y: i32, ch: char, fg: u8) {
        let ch = if self.uppercase_only { ch.to_ascii_uppercase() } else { ch };
        if ch == ' ' {
            return;
        }
        let s = f.scale as f32;
        let px = v.em_units * s;
        let (m, bitmap) = v.glyph(ch, px);
        if m.width == 0 || m.height == 0 {
            return;
        }
        let cell = self.advance as f32 * s;
        let gx = (x as f32 * s + (cell - m.advance_width) / 2.0).round() as i32 + m.xmin;
        let baseline = (y as f32 * s + v.baseline_units * s).round() as i32;
        let top = baseline - (m.ymin + m.height as i32);
        for row in 0..m.height {
            for col in 0..m.width {
                let cov = bitmap[row * m.width + col] as u32;
                if cov == 0 {
                    continue;
                }
                let level = ((cov * BLEND_LEVELS as u32 + 127) / 255) as u8;
                if level == 0 {
                    continue;
                }
                let (dx, dy) = (gx + col as i32, top + row as i32);
                let under = f.get_device(dx, dy);
                let c = f.blend(under, fg, level);
                f.put_device(dx, dy, c);
            }
        }
    }

    /// Parse the `.font` format: `font <w> <h>` then `glyph <code>` blocks of
    /// `h` rows using `.` and `#`.
    pub fn parse(text: &str) -> Result<Font, String> {
        let mut lines = text.lines();
        let mut w = 0;
        let mut h = 0;
        let mut glyphs: Vec<Option<Vec<u16>>> = vec![None; 128];
        while let Some(line) = lines.next() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            let mut words = line.split_whitespace();
            match words.next() {
                Some("font") => {
                    w = words.next().and_then(|s| s.parse().ok()).ok_or("bad font header")?;
                    h = words.next().and_then(|s| s.parse().ok()).ok_or("bad font header")?;
                }
                Some("glyph") => {
                    let code: usize = words.next().and_then(|s| s.parse().ok()).ok_or("bad glyph header")?;
                    let mut rows = Vec::new();
                    for _ in 0..h {
                        let r = lines.next().ok_or("glyph truncated")?;
                        let mut bits = 0u16;
                        for (i, ch) in r.chars().take(w as usize).enumerate() {
                            if ch == '#' {
                                bits |= 1 << i;
                            }
                        }
                        rows.push(bits);
                    }
                    if code < 128 {
                        glyphs[code] = Some(rows);
                    }
                }
                _ => {}
            }
        }
        if w == 0 || h == 0 {
            return Err("missing font header".into());
        }
        Ok(Font { cell_w: w, cell_h: h, advance: w, glyphs, uppercase_only: true, vector: None })
    }
}

/// The smooth font: IBM Plex Mono, sized so one character is one 4x8 cell.
pub fn smooth_font() -> Font {
    let font = fontdue::Font::from_bytes(PLEX_MONO, fontdue::FontSettings::default()).expect("bundled font parses");
    let advance_per_em = font.metrics('M', 1000.0).advance_width / 1000.0;
    let cell = small_font();
    let em_units = cell.advance as f32 / advance_per_em.max(0.3);
    Font {
        vector: Some(Rc::new(VectorFont { font, em_units, baseline_units: 7.0, cache: RefCell::new(HashMap::new()) })),
        ..cell
    }
}

/// Built-in 3x5 capitals on a 4x8 cell (glyph rows 2..7).
pub fn small_font() -> Font {
    const G: &[(char, [&str; 5])] = &[
        ('A', [".#.", "#.#", "###", "#.#", "#.#"]),
        ('B', ["##.", "#.#", "##.", "#.#", "##."]),
        ('C', [".##", "#..", "#..", "#..", ".##"]),
        ('D', ["##.", "#.#", "#.#", "#.#", "##."]),
        ('E', ["###", "#..", "##.", "#..", "###"]),
        ('F', ["###", "#..", "##.", "#..", "#.."]),
        ('G', [".##", "#..", "#.#", "#.#", ".##"]),
        ('H', ["#.#", "#.#", "###", "#.#", "#.#"]),
        ('I', ["###", ".#.", ".#.", ".#.", "###"]),
        ('J', ["..#", "..#", "..#", "#.#", ".#."]),
        ('K', ["#.#", "#.#", "##.", "#.#", "#.#"]),
        ('L', ["#..", "#..", "#..", "#..", "###"]),
        ('M', ["#.#", "###", "###", "#.#", "#.#"]),
        ('N', ["##.", "#.#", "#.#", "#.#", "#.#"]),
        ('O', [".#.", "#.#", "#.#", "#.#", ".#."]),
        ('P', ["##.", "#.#", "##.", "#..", "#.."]),
        ('Q', [".#.", "#.#", "#.#", "##.", ".##"]),
        ('R', ["##.", "#.#", "##.", "#.#", "#.#"]),
        ('S', [".##", "#..", ".#.", "..#", "##."]),
        ('T', ["###", ".#.", ".#.", ".#.", ".#."]),
        ('U', ["#.#", "#.#", "#.#", "#.#", "###"]),
        ('V', ["#.#", "#.#", "#.#", "#.#", ".#."]),
        ('W', ["#.#", "#.#", "###", "###", "#.#"]),
        ('X', ["#.#", "#.#", ".#.", "#.#", "#.#"]),
        ('Y', ["#.#", "#.#", ".#.", ".#.", ".#."]),
        ('Z', ["###", "..#", ".#.", "#..", "###"]),
        ('0', ["###", "#.#", "#.#", "#.#", "###"]),
        ('1', [".#.", "##.", ".#.", ".#.", "###"]),
        ('2', ["##.", "..#", ".#.", "#..", "###"]),
        ('3', ["##.", "..#", ".#.", "..#", "##."]),
        ('4', ["#.#", "#.#", "###", "..#", "..#"]),
        ('5', ["###", "#..", "##.", "..#", "##."]),
        ('6', [".##", "#..", "###", "#.#", "###"]),
        ('7', ["###", "..#", ".#.", ".#.", ".#."]),
        ('8', ["###", "#.#", "###", "#.#", "###"]),
        ('9', ["###", "#.#", "###", "..#", "##."]),
        ('.', ["...", "...", "...", "...", ".#."]),
        (',', ["...", "...", "...", ".#.", "#.."]),
        (':', ["...", ".#.", "...", ".#.", "..."]),
        (';', ["...", ".#.", "...", ".#.", "#.."]),
        ('!', [".#.", ".#.", ".#.", "...", ".#."]),
        ('?', ["##.", "..#", ".#.", "...", ".#."]),
        ('\'', [".#.", ".#.", "...", "...", "..."]),
        ('"', ["#.#", "#.#", "...", "...", "..."]),
        ('-', ["...", "...", "###", "...", "..."]),
        ('+', ["...", ".#.", "###", ".#.", "..."]),
        ('/', ["..#", "..#", ".#.", "#..", "#.."]),
        ('\\', ["#..", "#..", ".#.", "..#", "..#"]),
        ('(', [".#.", "#..", "#..", "#..", ".#."]),
        (')', [".#.", "..#", "..#", "..#", ".#."]),
        ('[', ["##.", "#..", "#..", "#..", "##."]),
        (']', [".##", "..#", "..#", "..#", ".##"]),
        ('*', ["...", "#.#", ".#.", "#.#", "..."]),
        ('%', ["#.#", "..#", ".#.", "#..", "#.#"]),
        ('#', ["#.#", "###", "#.#", "###", "#.#"]),
        ('<', ["..#", ".#.", "#..", ".#.", "..#"]),
        ('>', ["#..", ".#.", "..#", ".#.", "#.."]),
        ('^', [".#.", "#.#", "...", "...", "..."]),
        ('=', ["...", "###", "...", "###", "..."]),
        ('_', ["...", "...", "...", "...", "###"]),
        ('&', [".#.", "#.#", ".#.", "#.#", ".##"]),
        ('@', ["###", "#.#", "###", "#..", "###"]),
        ('$', [".##", "##.", ".#.", ".##", "##."]),
        ('|', [".#.", ".#.", ".#.", ".#.", ".#."]),
        ('~', ["...", ".#.", "#.#", "...", "..."]),
    ];
    let mut glyphs: Vec<Option<Vec<u16>>> = vec![None; 128];
    for (ch, rows) in G {
        let mut v = vec![0u16; 8];
        for (r, s) in rows.iter().enumerate() {
            let mut bits = 0u16;
            for (i, c) in s.chars().enumerate() {
                if c == '#' {
                    bits |= 1 << i;
                }
            }
            v[r + 2] = bits;
        }
        glyphs[*ch as usize] = Some(v);
    }
    glyphs[' ' as usize] = Some(vec![0; 8]);
    Font { cell_w: 4, cell_h: 8, advance: 4, glyphs, uppercase_only: true, vector: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_font_covers_ascii_caps_and_digits() {
        let f = small_font();
        for c in ('A'..='Z').chain('0'..='9') {
            assert!(f.glyph(c).is_some(), "{c}");
            assert!(f.glyph(c.to_ascii_lowercase()).is_some());
        }
        assert_eq!(f.width("TURN: 6"), 28);
    }

    #[test]
    fn smooth_font_draws_inside_its_cell() {
        let font = smooth_font();
        assert_eq!(font.advance, 4, "layouts depend on the 4-unit advance");
        let mut f = Frame::with_scale(16, 8, 6);
        f.clear(crate::gfx::CYAN);
        font.draw(&mut f, 4, 0, "W", crate::gfx::BLACK, None);
        // Something was drawn, and only inside the W's cell (x 4..8).
        let mut inked = 0;
        for dy in 0..f.device_height() as i32 {
            for dx in 0..f.device_width() as i32 {
                if f.get_device(dx, dy) != crate::gfx::CYAN {
                    inked += 1;
                    assert!((4 * 6 - 1..8 * 6 + 1).contains(&dx), "ink at x={dx}");
                }
            }
        }
        assert!(inked > 20);
        assert_eq!(f.get(0, 0), crate::gfx::CYAN);
    }
}
