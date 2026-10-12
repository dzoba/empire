//! Indexed-colour framebuffer and drawing primitives.
//!
//! Drawing uses *logical* coordinates: the original's 320x200-style grid that
//! all layouts are written in. Each logical pixel covers `scale` x `scale`
//! device pixels, so at scale 1 this is the authentic CGA picture and at
//! higher scales the same layout renders at the monitor's real resolution:
//! text is drawn with a real font, rules are thin hairlines, and icons are
//! smoothed with a palette-preserving pixel-art upscaler.
//!
//! Pixels are indices into a 256-entry palette. Entries 0-15 are the standard
//! EGA colours (the CGA look uses black 0, cyan 11, magenta 13, white 15);
//! higher entries are allocated on demand for antialiased text edges, so
//! untouched pixels always read back as their exact base colour.

use crate::font::FONT8X8;
use std::collections::HashMap;

pub const BLACK: u8 = 0;
pub const BLUE: u8 = 1;
pub const GREEN: u8 = 2;
pub const CYAN_DARK: u8 = 3;
pub const RED: u8 = 4;
pub const MAGENTA_DARK: u8 = 5;
pub const BROWN: u8 = 6;
pub const GREY: u8 = 7;
pub const DARK_GREY: u8 = 8;
pub const LIGHT_BLUE: u8 = 9;
pub const LIGHT_GREEN: u8 = 10;
pub const CYAN: u8 = 11;
pub const LIGHT_RED: u8 = 12;
pub const MAGENTA: u8 = 13;
pub const YELLOW: u8 = 14;
pub const WHITE: u8 = 15;

/// Standard EGA/VGA text-mode palette.
pub const EGA_PALETTE: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0xAA],
    [0x00, 0xAA, 0x00],
    [0x00, 0xAA, 0xAA],
    [0xAA, 0x00, 0x00],
    [0xAA, 0x00, 0xAA],
    [0xAA, 0x55, 0x00],
    [0xAA, 0xAA, 0xAA],
    [0x55, 0x55, 0x55],
    [0x55, 0x55, 0xFF],
    [0x55, 0xFF, 0x55],
    [0x55, 0xFF, 0xFF],
    [0xFF, 0x55, 0x55],
    [0xFF, 0x55, 0xFF],
    [0xFF, 0xFF, 0x55],
    [0xFF, 0xFF, 0xFF],
];

/// CGA palette 1, high intensity, as EGA indices: black, cyan, magenta, white.
pub const CGA1: [u8; 4] = [BLACK, CYAN, MAGENTA, WHITE];

/// Antialiasing levels for text edges (coverage is quantised to this many steps).
pub const BLEND_LEVELS: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn inset(&self, d: i32) -> Rect {
        Rect::new(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d)
    }
}

pub struct Frame {
    /// Logical size.
    pub width: usize,
    pub height: usize,
    /// Device pixels per logical pixel.
    pub scale: usize,
    /// Device-resolution palette indices.
    pub pixels: Vec<u8>,
    pub rgba: Vec<u8>,
    pub palette: Vec<[u8; 3]>,
    blends: HashMap<(u8, u8, u8), u8>,
    tile_cache: HashMap<([u8; 64], u8), Vec<u8>>,
}

fn base_palette() -> Vec<[u8; 3]> {
    let mut p = EGA_PALETTE.to_vec();
    p.resize(256, [0, 0, 0]);
    p
}

impl Frame {
    pub fn new(width: usize, height: usize) -> Self {
        Self::with_scale(width, height, 1)
    }

    pub fn with_scale(width: usize, height: usize, scale: usize) -> Self {
        let scale = scale.max(1);
        let n = width * scale * height * scale;
        Frame {
            width,
            height,
            scale,
            pixels: vec![0; n],
            rgba: vec![255; n * 4],
            palette: base_palette(),
            blends: HashMap::new(),
            tile_cache: HashMap::new(),
        }
    }

    pub fn resize(&mut self, width: usize, height: usize, scale: usize) {
        let scale = scale.max(1);
        if width != self.width || height != self.height || scale != self.scale {
            let cache = std::mem::take(&mut self.tile_cache);
            *self = Frame::with_scale(width, height, scale);
            self.tile_cache = cache;
        }
    }

    /// Device (canvas) size in pixels.
    pub fn device_width(&self) -> usize {
        self.width * self.scale
    }
    pub fn device_height(&self) -> usize {
        self.height * self.scale
    }

    /// Thickness of a one-pixel rule in device pixels.
    pub fn hairline(&self) -> usize {
        ((self.scale as f64 / 2.5).round() as usize).max(1)
    }

    // ------------------------------------------------------------------
    // Device-pixel access (for the text renderer)

    #[inline]
    pub fn put_device(&mut self, dx: i32, dy: i32, c: u8) {
        let (pw, ph) = (self.device_width() as i32, self.device_height() as i32);
        if dx >= 0 && dy >= 0 && dx < pw && dy < ph {
            self.pixels[dy as usize * pw as usize + dx as usize] = c;
        }
    }

    #[inline]
    pub fn get_device(&self, dx: i32, dy: i32) -> u8 {
        let (pw, ph) = (self.device_width() as i32, self.device_height() as i32);
        if dx >= 0 && dy >= 0 && dx < pw && dy < ph {
            self.pixels[dy as usize * pw as usize + dx as usize]
        } else {
            0
        }
    }

    fn fill_device(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u8) {
        let pw = self.device_width() as i32;
        let ph = self.device_height() as i32;
        let (x0, y0, x1, y1) = (x0.max(0), y0.max(0), x1.min(pw), y1.min(ph));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for y in y0..y1 {
            let row = y as usize * pw as usize;
            self.pixels[row + x0 as usize..row + x1 as usize].fill(c);
        }
    }

    /// Palette index for `fg` drawn over `bg` at `level`/`BLEND_LEVELS` coverage.
    pub fn blend(&mut self, bg: u8, fg: u8, level: u8) -> u8 {
        if level == 0 || bg == fg {
            return bg;
        }
        if level >= BLEND_LEVELS {
            return fg;
        }
        // Blend from base colours only, so blends of blends don't pile up.
        let bg = if bg < 16 { bg } else { return if level * 2 >= BLEND_LEVELS { fg } else { bg } };
        if let Some(&i) = self.blends.get(&(bg, fg, level)) {
            return i;
        }
        let next = 16 + self.blends.len();
        if next > 255 {
            return if level * 2 >= BLEND_LEVELS { fg } else { bg };
        }
        let a = level as f64 / BLEND_LEVELS as f64;
        let (b, f) = (self.palette[bg as usize], self.palette[fg as usize]);
        let mix = |i: usize| (b[i] as f64 * (1.0 - a) + f[i] as f64 * a).round() as u8;
        self.palette[next] = [mix(0), mix(1), mix(2)];
        self.blends.insert((bg, fg, level), next as u8);
        next as u8
    }

    // ------------------------------------------------------------------
    // Logical drawing

    pub fn clear(&mut self, c: u8) {
        self.pixels.fill(c);
    }

    #[inline]
    pub fn put(&mut self, x: i32, y: i32, c: u8) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let s = self.scale as i32;
        if s == 1 {
            self.pixels[y as usize * self.width + x as usize] = c;
        } else {
            self.fill_device(x * s, y * s, (x + 1) * s, (y + 1) * s, c);
        }
    }

    /// The colour at the centre of a logical pixel.
    pub fn get(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return 0;
        }
        let s = self.scale as i32;
        self.get_device(x * s + s / 2, y * s + s / 2)
    }

    pub fn fill(&mut self, r: Rect, c: u8) {
        let s = self.scale as i32;
        self.fill_device(r.x * s, r.y * s, r.right() * s, r.bottom() * s, c);
    }

    /// Two-colour checkerboard, the classic CGA "grey".
    pub fn dither(&mut self, r: Rect, a: u8, b: u8) {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                self.put(x, y, if (x + y) & 1 == 0 { a } else { b });
            }
        }
    }

    /// A one-pixel horizontal rule: a hairline centred in logical row `y`.
    pub fn hline(&mut self, x0: i32, x1: i32, y: i32, c: u8) {
        let (a, b) = (x0.min(x1), x0.max(x1));
        let s = self.scale as i32;
        let t = self.hairline() as i32;
        let off = (s - t) / 2;
        self.fill_device(a * s, y * s + off, (b + 1) * s, y * s + off + t, c);
    }

    /// A one-pixel vertical rule: a hairline centred in logical column `x`.
    pub fn vline(&mut self, x: i32, y0: i32, y1: i32, c: u8) {
        let (a, b) = (y0.min(y1), y0.max(y1));
        let s = self.scale as i32;
        let t = self.hairline() as i32;
        let off = (s - t) / 2;
        self.fill_device(x * s + off, a * s, x * s + off + t, (b + 1) * s, c);
    }

    /// A rectangle outline drawn with hairlines along its outer edge pixels.
    pub fn frame(&mut self, r: Rect, c: u8) {
        self.hline(r.x, r.right() - 1, r.y, c);
        self.hline(r.x, r.right() - 1, r.bottom() - 1, c);
        self.vline(r.x, r.y, r.bottom() - 1, c);
        self.vline(r.right() - 1, r.y, r.bottom() - 1, c);
    }

    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u8) {
        let (mut x, mut y) = (x0, y0);
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.put(x, y, c);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Draw an 8x8 cell of palette indices at a logical position. With
    /// `smooth`, the cell is enlarged with Scale2x (which only ever reuses
    /// existing colours) before sampling down to the device size.
    pub fn blit8(&mut self, x: i32, y: i32, px: &[u8; 64], smooth: bool) {
        let s = self.scale as i32;
        if s == 1 || !smooth {
            for ty in 0..8 {
                for tx in 0..8 {
                    self.put(x + tx, y + ty, px[(ty * 8 + tx) as usize]);
                }
            }
            return;
        }
        let factor: u8 = if s <= 2 {
            2
        } else if s <= 4 {
            4
        } else {
            8
        };
        let key = (*px, factor);
        if !self.tile_cache.contains_key(&key) {
            let mut img = px.to_vec();
            let mut size = 8;
            while size < 8 * factor as usize {
                img = scale2x(&img, size, size);
                size *= 2;
            }
            if self.tile_cache.len() > 4096 {
                self.tile_cache.clear();
            }
            self.tile_cache.insert(key, img);
        }
        let img = &self.tile_cache[&key];
        let src = 8 * factor as i32;
        let dst = 8 * s;
        let (ox, oy) = (x * s, y * s);
        let pw = self.device_width() as i32;
        let ph = self.device_height() as i32;
        for dy in 0..dst {
            let py = oy + dy;
            if py < 0 || py >= ph {
                continue;
            }
            let sy = (dy * src / dst) as usize;
            let row = py as usize * pw as usize;
            for dx in 0..dst {
                let pxx = ox + dx;
                if pxx < 0 || pxx >= pw {
                    continue;
                }
                let sx = (dx * src / dst) as usize;
                self.pixels[row + pxx as usize] = img[sy * src as usize + sx];
            }
        }
    }

    /// Draw an 8x8 terrain cell. Two-colour dithers (a pixel whose four
    /// neighbours all share another colour) are rendered as a fine checker at
    /// device resolution, the way a CRT blended them, instead of as large
    /// squares. Every dither of the same colour pair uses one global phase,
    /// so neighbouring cells join seamlessly.
    pub fn blit8_texture(&mut self, x: i32, y: i32, px: &[u8; 64]) {
        let s = self.scale as i32;
        if s < 3 {
            self.blit8(x, y, px, false);
            return;
        }
        // For each source pixel: Some((a, b)) if it is part of a dither.
        let mut pair: [Option<(u8, u8)>; 64] = [None; 64];
        for ty in 0..8usize {
            for tx in 0..8usize {
                let p = px[ty * 8 + tx];
                // Terrain tiles repeat, so neighbours wrap around the cell.
                let n = [
                    px[((ty + 7) % 8) * 8 + tx],
                    px[((ty + 1) % 8) * 8 + tx],
                    px[ty * 8 + (tx + 7) % 8],
                    px[ty * 8 + (tx + 1) % 8],
                ];
                if n.iter().all(|&q| q == n[0]) && n[0] != p {
                    pair[ty * 8 + tx] = Some((p.min(n[0]), p.max(n[0])));
                }
            }
        }
        let grain = (s / 4).max(1);
        let (ox, oy) = (x * s, y * s);
        let pw = self.device_width() as i32;
        let ph = self.device_height() as i32;
        for dy in 0..8 * s {
            let py = oy + dy;
            if py < 0 || py >= ph {
                continue;
            }
            let sy = (dy / s) as usize;
            let row = py as usize * pw as usize;
            for dx in 0..8 * s {
                let pxx = ox + dx;
                if pxx < 0 || pxx >= pw {
                    continue;
                }
                let i = sy * 8 + (dx / s) as usize;
                let c = match pair[i] {
                    Some((a, b)) => {
                        if ((pxx / grain) + (py / grain)) & 1 == 0 {
                            a
                        } else {
                            b
                        }
                    }
                    None => px[i],
                };
                self.pixels[row + pxx as usize] = c;
            }
        }
    }

    /// Copy another frame's device pixels to device position (dx, dy),
    /// clipped to `w` x `h` device pixels. Both frames share one palette.
    pub fn paste(&mut self, src: &Frame, dx: i32, dy: i32, w: i32, h: i32) {
        let (sw, sh) = (src.device_width() as i32, src.device_height() as i32);
        let (pw, ph) = (self.device_width() as i32, self.device_height() as i32);
        let w = w.min(sw).min(pw - dx);
        let h = h.min(sh).min(ph - dy);
        if w <= 0 || h <= 0 || dx < 0 || dy < 0 {
            return;
        }
        for y in 0..h {
            let s0 = (y * sw) as usize;
            let d0 = ((dy + y) * pw + dx) as usize;
            self.pixels[d0..d0 + w as usize].copy_from_slice(&src.pixels[s0..s0 + w as usize]);
        }
    }

    /// Draw a character from the built-in 8x8 font (title card only).
    pub fn glyph(&mut self, x: i32, y: i32, ch: char, fg: u8, bg: Option<u8>) {
        let code = ch as u32;
        let bits = if code < 128 { FONT8X8[code as usize] } else { FONT8X8[b'?' as usize] };
        for (row, b) in bits.iter().enumerate() {
            for col in 0..8 {
                let on = b & (1 << col) != 0;
                if on {
                    self.put(x + col, y + row as i32, fg);
                } else if let Some(bg) = bg {
                    self.put(x + col, y + row as i32, bg);
                }
            }
        }
    }

    /// Convert to RGBA for the host.
    pub fn to_rgba(&mut self) -> &[u8] {
        let lut: Vec<[u8; 4]> = self.palette.iter().map(|c| [c[0], c[1], c[2], 255]).collect();
        for (dst, &p) in self.rgba.chunks_exact_mut(4).zip(self.pixels.iter()) {
            dst.copy_from_slice(&lut[p as usize]);
        }
        &self.rgba
    }

    /// Write a binary PPM at device resolution (tests and CLI screenshots).
    pub fn to_ppm(&self) -> Vec<u8> {
        let mut out = format!("P6\n{} {}\n255\n", self.device_width(), self.device_height()).into_bytes();
        out.reserve(self.pixels.len() * 3);
        for &p in &self.pixels {
            out.extend_from_slice(&self.palette[p as usize]);
        }
        out
    }
}

/// Scale2x (EPX): doubles a palette image, rounding diagonal edges without
/// introducing any new colours.
pub fn scale2x(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    let at = |x: isize, y: isize| -> u8 {
        let x = x.clamp(0, w as isize - 1) as usize;
        let y = y.clamp(0, h as isize - 1) as usize;
        src[y * w + x]
    };
    let ow = w * 2;
    for y in 0..h as isize {
        for x in 0..w as isize {
            let p = at(x, y);
            let a = at(x, y - 1);
            let b = at(x + 1, y);
            let c = at(x - 1, y);
            let d = at(x, y + 1);
            let e0 = if c == a && c != d && a != b { a } else { p };
            let e1 = if a == b && a != c && b != d { b } else { p };
            let e2 = if d == c && d != b && c != a { c } else { p };
            let e3 = if b == d && b != a && d != c { d } else { p };
            let (ox, oy) = (x as usize * 2, y as usize * 2);
            out[oy * ow + ox] = e0;
            out[oy * ow + ox + 1] = e1;
            out[(oy + 1) * ow + ox] = e2;
            out[(oy + 1) * ow + ox + 1] = e3;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_fill_and_readback() {
        let mut f = Frame::with_scale(10, 10, 4);
        f.clear(CYAN);
        f.fill(Rect::new(2, 2, 3, 3), MAGENTA);
        assert_eq!(f.get(3, 3), MAGENTA);
        assert_eq!(f.get(6, 6), CYAN);
        assert_eq!(f.device_width(), 40);
        assert_eq!(f.get_device(8, 8), MAGENTA);
        assert_eq!(f.get_device(7, 7), CYAN);
    }

    #[test]
    fn blends_keep_base_colours_exact() {
        let mut f = Frame::with_scale(10, 10, 4);
        f.clear(CYAN);
        let mid = f.blend(CYAN, BLACK, 4);
        assert!(mid >= 16);
        assert_eq!(f.blend(CYAN, BLACK, 4), mid, "blends are reused");
        f.put_device(1, 1, mid);
        assert_eq!(f.get(5, 5), CYAN, "untouched pixels stay exact");
        assert_eq!(f.blend(CYAN, BLACK, BLEND_LEVELS), BLACK);
        assert_eq!(f.blend(CYAN, BLACK, 0), CYAN);
    }

    #[test]
    fn scale2x_rounds_a_diagonal() {
        // A 2x2 checker corner: the shared diagonal colour fills in.
        let src = [1, 0, 0, 1];
        let out = scale2x(&src, 2, 2);
        assert_eq!(out.len(), 16);
        // No new colours appear.
        assert!(out.iter().all(|&c| c == 0 || c == 1));
        let src = [0, 0, 0, 0, 1, 1, 0, 1, 1];
        let out = scale2x(&src, 3, 3);
        // The top-left corner of the 1-block is rounded off.
        assert_eq!(out[2 * 6 + 2], 0);
        assert_eq!(out[3 * 6 + 3], 1);
    }

    #[test]
    fn hairlines_are_thin() {
        let mut f = Frame::with_scale(20, 20, 5);
        f.clear(BLACK);
        f.hline(0, 19, 10, WHITE);
        let t = f.hairline();
        assert_eq!(t, 2);
        let whites = (50..55).filter(|&dy| f.get_device(20, dy) == WHITE).count();
        assert_eq!(whites, t);
    }
}
