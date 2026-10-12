//! Screen layout for the playing screen, measured from the original at
//! 320x200 and generalised to any framebuffer size: a bigger screen shows more
//! of the map rather than bigger pixels.
//!
//! Text and frames are placed on the layout grid. Map squares have their own
//! size in device pixels, so the map can be zoomed without changing the text.

use crate::gfx::Rect;

/// Every map square is an 8x8 picture.
pub const TILE: i32 = 8;

/// Device pixels per layout unit, and per map-picture pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zoom {
    pub ui: i32,
    pub map: i32,
}

impl Default for Zoom {
    fn default() -> Self {
        Zoom { ui: 1, map: 1 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlayLayout {
    pub width: i32,
    pub height: i32,
    /// y of the menu bar text cell.
    pub menu_y: i32,
    /// y of the rule under the menu bar.
    pub rule1_y: i32,
    /// y of the message line text cell.
    pub msg_y: i32,
    /// y of the rule under the message line.
    pub rule2_y: i32,
    /// Outer frame around the map area.
    pub frame: Rect,
    /// "3" box, vertical track, "4" box.
    pub up_box: Rect,
    pub vtrack: Rect,
    pub down_box: Rect,
    /// Row-number strip.
    pub row_strip: Rect,
    /// The map viewport in pixels.
    pub map: Rect,
    /// Column-number strip.
    pub col_strip: Rect,
    /// "1" box, horizontal track, "2" box.
    pub left_box: Rect,
    pub htrack: Rect,
    pub right_box: Rect,
    pub cols: usize,
    pub rows: usize,
    /// Device pixels per layout unit.
    pub ui_scale: i32,
    /// Device pixels per map square.
    pub tile_dev: i32,
}

impl PlayLayout {
    /// `row_digits` is the number of digits in the largest row number.
    pub fn new(width: i32, height: i32, row_digits: i32, zoom: Zoom) -> PlayLayout {
        let ui = zoom.ui.max(1);
        let tile_dev = TILE * zoom.map.max(1);
        let strip_w = (row_digits.max(2) * 4).max(8);
        let frame = Rect::new(3, 22, width - 6, height - 22);
        let map_x = 12 + strip_w;
        let map_y = 23;
        let map_right = frame.right() - 1; // exclusive of the right frame line
        let cols = (((map_right - map_x) * ui) / tile_dev).max(1) as usize;
        let bottom_reserved = 17; // column strip (8) + rule + scroll bar (7) + rule
        let rows = (((height - map_y - bottom_reserved) * ui) / tile_dev).max(1) as usize;
        let map_h = (rows as i32 * tile_dev + ui - 1) / ui;
        let map = Rect::new(map_x, map_y, map_right - map_x, map_h);
        let strip_y = map.bottom() + 1;
        let scroll_y = strip_y + 8;
        PlayLayout {
            width,
            height,
            menu_y: 1,
            rule1_y: 10,
            msg_y: 13,
            rule2_y: 22,
            frame,
            up_box: Rect::new(4, 23, 7, 7),
            vtrack: Rect::new(4, 31, 7, map.bottom() - 31),
            down_box: Rect::new(4, strip_y, 7, 7),
            row_strip: Rect::new(12, map_y, strip_w, map.h),
            map,
            col_strip: Rect::new(12, strip_y, map_right - 12, 7),
            left_box: Rect::new(12, scroll_y, 7, 7),
            htrack: Rect::new(map_x, scroll_y, map_right - 7 - 1 - map_x, 7),
            right_box: Rect::new(map_right - 7, scroll_y, 7, 7),
            cols,
            rows,
            ui_scale: ui,
            tile_dev,
        }
    }

    /// Top-left corner of on-screen square (dc, dr), in layout units.
    pub fn cell_pos(&self, dc: usize, dr: usize) -> (i32, i32) {
        (self.map.x + dc as i32 * self.tile_dev / self.ui_scale, self.map.y + dr as i32 * self.tile_dev / self.ui_scale)
    }

    /// The on-screen square under a point given in layout units.
    pub fn cell_at(&self, x: i32, y: i32) -> Option<(usize, usize)> {
        if !self.map.contains(x, y) {
            return None;
        }
        let dc = ((x - self.map.x) * self.ui_scale / self.tile_dev) as usize;
        let dr = ((y - self.map.y) * self.ui_scale / self.tile_dev) as usize;
        (dc < self.cols && dr < self.rows).then_some((dc, dr))
    }

    /// Size of the offscreen map picture in map pixels, and its scale.
    pub fn map_picture(&self) -> (usize, usize, usize) {
        (self.cols * TILE as usize, self.rows * TILE as usize, (self.tile_dev / TILE) as usize)
    }

    /// Label every `n`th row and column so numbers of `digits` digits don't overlap.
    pub fn label_step(&self, digits: usize) -> usize {
        let need = (digits as i32 * 4 + 1) * self.ui_scale;
        let mut n = 2;
        while (n as i32) * self.tile_dev < need {
            n += 2;
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_layout() {
        let l = PlayLayout::new(320, 200, 2, Zoom::default());
        assert_eq!((l.cols, l.rows), (37, 20));
        assert_eq!(l.map.x, 20);
        assert_eq!(l.map.y, 23);
        assert_eq!(l.map.bottom(), 183);
        assert_eq!(l.col_strip.y, 184);
        assert_eq!(l.htrack.y, 192);
    }

    #[test]
    fn bigger_screens_show_more_map() {
        let l = PlayLayout::new(640, 400, 2, Zoom::default());
        assert!(l.cols > 70 && l.rows > 40);
    }

    #[test]
    fn map_zoom_is_independent_of_text() {
        // Text at 4 device pixels per unit; map squares at 16 or 64 device pixels.
        let near = PlayLayout::new(320, 200, 2, Zoom { ui: 4, map: 8 });
        let far = PlayLayout::new(320, 200, 2, Zoom { ui: 4, map: 2 });
        assert_eq!(near.msg_y, far.msg_y);
        assert!(far.cols >= near.cols * 4 - 1 && far.rows >= near.rows * 4 - 1);
        // Hit testing and positions agree.
        for dc in [0, 3, far.cols - 1] {
            let (x, y) = far.cell_pos(dc, 2);
            assert_eq!(far.cell_at(x + 1, y + 1), Some((dc, 2)));
        }
        assert!(far.label_step(2) > near.label_step(2) - 1);
    }
}
