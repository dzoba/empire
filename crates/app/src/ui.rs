//! Widgets drawn in the style of the original CGA screens: menu bar, drop-down
//! menus, panels with title plates, buttons, list boxes, and scroll bars.

use crate::gfx::*;
use crate::text::Font;
use crate::theme::Theme;

/// A label with an optional hotkey letter, written as "&Random" -> R is hot.
pub fn split_hot(label: &str) -> (String, Option<usize>) {
    let mut out = String::new();
    let mut hot = None;
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            if let Some(&n) = chars.peek() {
                if n != '&' {
                    hot = Some(out.chars().count());
                }
                out.push(n);
                chars.next();
                continue;
            }
        }
        out.push(c);
    }
    (out, hot)
}

pub fn hot_char(label: &str) -> Option<char> {
    let (s, h) = split_hot(label);
    h.and_then(|i| s.chars().nth(i)).map(|c| c.to_ascii_uppercase())
}

pub fn draw_hot(f: &mut Frame, font: &Font, x: i32, y: i32, label: &str, fg: u8, hot: u8, bg: Option<u8>) -> i32 {
    let (s, h) = split_hot(label);
    let mut cx = x;
    for (i, ch) in s.chars().enumerate() {
        font.draw_char(f, cx, y, ch, if Some(i) == h { hot } else { fg }, bg);
        cx += font.advance;
    }
    cx
}

pub fn label_width(font: &Font, label: &str) -> i32 {
    font.width(&split_hot(label).0)
}

/// A dialog panel: fill, a frame (1px sides, 2-row top and bottom so it looks
/// even on a 4:3 screen), and a drop shadow to the right and below.
pub fn panel(f: &mut Frame, r: Rect, bg: u8, frame: u8, shadow: u8) {
    f.fill(Rect::new(r.right(), r.y + 4, 2, r.h - 2), shadow);
    f.fill(Rect::new(r.x + 3, r.bottom(), r.w - 1, 3), shadow);
    f.fill(r, bg);
    f.frame(r, frame);
    f.hline(r.x, r.right() - 1, r.y + 1, frame);
    f.hline(r.x, r.right() - 1, r.bottom() - 2, frame);
}

/// A title plate centred on `cx`, straddling the top of a panel.
pub fn plate(f: &mut Frame, font: &Font, cx: i32, y: i32, text: &str, bg: u8, frame: u8, fg: u8) -> Rect {
    let w = font.width(text) + 12;
    let r = Rect::new(cx - w / 2, y, w, 11);
    f.fill(Rect::new(r.x + 1, r.bottom(), r.w, 1), frame);
    f.fill(Rect::new(r.right(), r.y + 1, 1, r.h), frame);
    f.fill(r, bg);
    f.frame(r, frame);
    font.draw(f, r.x + 6, r.y + 1, text, fg, None);
    r
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonLook {
    Normal,
    /// Toggled on (e.g. the selected opponent type).
    Selected,
    Disabled,
}

/// Button: white face, 1px black frame (2px for the default button), black
/// text with a magenta hotkey. Selected toggles are inverted.
pub fn button(f: &mut Frame, font: &Font, r: Rect, label: &str, look: ButtonLook, default: bool) {
    let (bg, fg, frame) = match look {
        ButtonLook::Normal => (WHITE, BLACK, BLACK),
        ButtonLook::Selected => (BLACK, WHITE, WHITE),
        ButtonLook::Disabled => (WHITE, CYAN, BLACK),
    };
    f.fill(r, frame);
    let inset = if default { 2 } else { 1 };
    f.fill(r.inset(inset), bg);
    if look == ButtonLook::Selected {
        f.frame(r, BLACK);
    }
    let w = label_width(font, label);
    let tx = r.x + (r.w - w) / 2;
    let ty = r.y + (r.h - font.cell_h) / 2;
    draw_hot(f, font, tx, ty, label, fg, if look == ButtonLook::Disabled { fg } else { MAGENTA }, None);
}

pub fn button_rect_for(font: &Font, x: i32, y: i32, label: &str, min_w: i32) -> Rect {
    Rect::new(x, y, (label_width(font, label) + 12).max(min_w), 13)
}

/// White list box with a black frame.
pub fn list_box(f: &mut Frame, r: Rect) {
    f.fill(r, WHITE);
    f.frame(r, BLACK);
}

/// Dashed divider used in menus.
pub fn dashed(f: &mut Frame, x0: i32, x1: i32, y: i32, c: u8) {
    let mut x = x0;
    while x <= x1 {
        f.put(x, y, c);
        f.put(x + 1, y, c);
        x += 3;
    }
}

/// Vertical scroll bar track with a thumb; `pos`/`size`/`total` in items.
pub fn vscroll(f: &mut Frame, r: Rect, pos: usize, size: usize, total: usize, track: u8, thumb: u8) {
    f.fill(r, track);
    if total == 0 || size >= total {
        f.fill(r, thumb);
        return;
    }
    let h = r.h as usize;
    let t0 = pos * h / total;
    let t1 = ((pos + size) * h).div_ceil(total).min(h);
    f.fill(Rect::new(r.x, r.y + t0 as i32, r.w, (t1 - t0).max(1) as i32), thumb);
}

pub fn hscroll(f: &mut Frame, r: Rect, pos: usize, size: usize, total: usize, track: u8, thumb: u8) {
    f.fill(r, track);
    if total == 0 || size >= total {
        f.fill(r, thumb);
        return;
    }
    let w = r.w as usize;
    let t0 = pos * w / total;
    let t1 = ((pos + size) * w).div_ceil(total).min(w);
    f.fill(Rect::new(r.x + t0 as i32, r.y, (t1 - t0).max(1) as i32, r.h), thumb);
}

/// Drop-down menu item.
#[derive(Clone, Debug)]
pub struct MenuItem<C: Clone> {
    pub label: &'static str,
    pub shortcut: &'static str,
    pub command: Option<C>,
    pub enabled: bool,
    pub checked: bool,
}

impl<C: Clone> MenuItem<C> {
    pub fn divider() -> Self {
        MenuItem { label: "-", shortcut: "", command: None, enabled: false, checked: false }
    }
    pub fn is_divider(&self) -> bool {
        self.label == "-"
    }
}

#[derive(Clone, Debug)]
pub struct Menu<C: Clone> {
    pub title: &'static str,
    pub items: Vec<MenuItem<C>>,
}

/// Geometry of the menu bar titles (x = 9, 33, 69, 101, 137 at 320 wide).
pub fn menu_title_rects<C: Clone>(font: &Font, menus: &[Menu<C>]) -> Vec<Rect> {
    let mut x = 9;
    menus
        .iter()
        .map(|m| {
            let w = label_width(font, m.title);
            let r = Rect::new(x - 3, 0, w + 6, 10);
            x += w + 2 * font.advance;
            r
        })
        .collect()
}

pub fn draw_menu_bar<C: Clone>(f: &mut Frame, font: &Font, theme: &Theme, menus: &[Menu<C>], open: Option<usize>) {
    let rects = menu_title_rects(font, menus);
    for (i, (m, r)) in menus.iter().zip(rects).enumerate() {
        if open == Some(i) {
            f.fill(Rect::new(r.x, r.y + 1, r.w, r.h - 1), theme.hl_bg);
            draw_hot(f, font, r.x + 3, 1, m.title, theme.hl_fg, theme.hot, None);
        } else {
            draw_hot(f, font, r.x + 3, 1, m.title, theme.text, theme.hot, None);
        }
    }
}

pub fn dropdown_rect<C: Clone>(font: &Font, menus: &[Menu<C>], idx: usize) -> Rect {
    let rects = menu_title_rects(font, menus);
    let m = &menus[idx];
    let lw = m.items.iter().map(|it| label_width(font, it.label)).max().unwrap_or(0);
    let sw = m.items.iter().map(|it| font.width(it.shortcut)).max().unwrap_or(0);
    let w = 5 + font.advance + lw + if sw > 0 { 2 * font.advance + sw } else { 0 } + 5;
    let h: i32 = m.items.len() as i32 * 8 + 5;
    Rect::new(rects[idx].x - 1, 10, w, h)
}

/// Returns the y of each item (dividers included) inside the dropdown.
pub fn dropdown_item_ys<C: Clone>(menu: &Menu<C>, top: i32) -> Vec<i32> {
    (0..menu.items.len()).map(|i| top + 1 + i as i32 * 8).collect()
}

pub fn draw_dropdown<C: Clone>(f: &mut Frame, font: &Font, theme: &Theme, menus: &[Menu<C>], idx: usize, sel: Option<usize>) {
    let r = dropdown_rect(font, menus, idx);
    f.fill(r, theme.bg);
    // 1px left rule, 2px right edge and 2-row bottom read as a shadow.
    f.vline(r.x, r.y, r.bottom() - 1, theme.rule);
    f.fill(Rect::new(r.right() - 2, r.y, 2, r.h), theme.rule);
    f.fill(Rect::new(r.x, r.bottom() - 2, r.w, 2), theme.rule);
    let m = &menus[idx];
    let ys = dropdown_item_ys(m, r.y);
    let sw = m.items.iter().map(|it| font.width(it.shortcut)).max().unwrap_or(0);
    for (i, (it, y)) in m.items.iter().zip(ys).enumerate() {
        if it.is_divider() {
            let mut x = r.x + 2;
            while x < r.right() - 3 {
                for k in 0..3 {
                    if x + k < r.right() - 3 {
                        f.put(x + k, y + 4, theme.divider);
                    }
                }
                x += 4;
            }
            continue;
        }
        let selected = sel == Some(i) && it.enabled;
        if selected {
            f.fill(Rect::new(r.x + 1, y, r.w - 3, 8), theme.hl_bg);
        }
        let fg = if !it.enabled {
            theme.disabled
        } else if selected {
            theme.hl_fg
        } else {
            theme.text
        };
        let label = split_hot(it.label).0;
        let x = r.x + 5;
        if it.checked {
            font.draw(f, x - font.advance + 1, y, "*", fg, None);
        }
        font.draw(f, x, y, &label, fg, None);
        if !it.shortcut.is_empty() {
            font.draw(f, r.right() - 4 - sw, y, it.shortcut, fg, None);
        }
    }
}

/// Word-wrap text to a maximum number of characters per line.
pub fn wrap(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split_whitespace() {
            if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > max_chars {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys() {
        assert_eq!(split_hot("&Random"), ("Random".to_string(), Some(0)));
        assert_eq!(split_hot("Sa&ve As"), ("Save As".to_string(), Some(2)));
        assert_eq!(hot_char("&Cancel"), Some('C'));
        assert_eq!(hot_char("OK"), None);
    }

    #[test]
    fn wrapping() {
        assert_eq!(wrap("one two three four", 9), vec!["one two", "three", "four"]);
    }
}
