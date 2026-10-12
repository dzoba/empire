//! A small retained-mode dialog system. A dialog is a list of elements built
//! from state; input produces [`DlgEvent`]s that the app handles, after which
//! the dialog is usually rebuilt.

use crate::gfx::*;
use crate::input::{Key, KeyEvent};
use crate::mapdraw::{CellArt, MapPainter};
use crate::text::Font;
use crate::theme::Theme;
use crate::ui::{self, ButtonLook};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DlgStyle {
    /// Magenta panel with a white frame (production, opponents, info).
    Panel,
    /// White message box with black text.
    Box,
    /// Cyan report window with a magenta shadow (war report, ship report).
    Report,
    /// White window with a cyan inner border (world map).
    MapWindow,
}

#[derive(Clone, Debug)]
pub enum Elem {
    Text { x: i32, y: i32, text: String, color: u8 },
    /// Text with `&` hotkey markup.
    HotText { x: i32, y: i32, text: String, color: u8 },
    Button { r: Rect, label: String, id: u32, look: ButtonLook },
    List { r: Rect, rows: Vec<String>, icons: Vec<Option<CellArt>>, sel: usize, top: usize, id: u32, marker: bool },
    Input { r: Rect, text: String, max: usize },
    Icon { x: i32, y: i32, art: CellArt },
    Fill { r: Rect, color: u8 },
    Frame { r: Rect, color: u8 },
    /// A slider: track with arrows; value 0..=100.
    Slider { r: Rect, value: u8, id: u32 },
    /// Placeholder the app paints (e.g. the world map).
    Custom { r: Rect, id: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DlgEvent {
    Button(u32),
    ListSelect(u32, usize),
    ListActivate(u32, usize),
    Submit,
    Slider(u32, i32),
    Key(KeyEvent),
    Click(i32, i32),
}

#[derive(Clone, Debug)]
pub struct Dialog<K> {
    pub kind: K,
    pub style: DlgStyle,
    pub rect: Rect,
    pub title: Option<String>,
    pub elems: Vec<Elem>,
    /// Button ids for Enter and Escape.
    pub default_button: Option<u32>,
    pub cancel_button: Option<u32>,
}

impl<K> Dialog<K> {
    pub fn new(kind: K, style: DlgStyle, rect: Rect) -> Self {
        Dialog { kind, style, rect, title: None, elems: Vec::new(), default_button: None, cancel_button: None }
    }

    pub fn centered(kind: K, style: DlgStyle, w: i32, h: i32, screen_w: i32, screen_h: i32) -> Self {
        let x = (screen_w - w) / 2;
        let y = 22 + (screen_h - 22 - h) / 2;
        Self::new(kind, style, Rect::new(x, y.max(12), w, h))
    }

    pub fn text_color(&self, theme: &Theme) -> u8 {
        match self.style {
            DlgStyle::Panel => theme.panel_text,
            DlgStyle::Box | DlgStyle::MapWindow => theme.box_text,
            DlgStyle::Report => theme.report_text,
        }
    }

    // Builder helpers, positions relative to the dialog's top-left.
    pub fn text(&mut self, x: i32, y: i32, s: impl Into<String>, color: u8) {
        let (rx, ry) = (self.rect.x, self.rect.y);
        self.elems.push(Elem::Text { x: rx + x, y: ry + y, text: s.into(), color });
    }
    pub fn hot_text(&mut self, x: i32, y: i32, s: impl Into<String>, color: u8) {
        let (rx, ry) = (self.rect.x, self.rect.y);
        self.elems.push(Elem::HotText { x: rx + x, y: ry + y, text: s.into(), color });
    }
    pub fn centered_text(&mut self, font: &Font, y: i32, s: impl Into<String>, color: u8) {
        let s = s.into();
        let x = (self.rect.w - font.width(&s)) / 2;
        self.text(x, y, s, color);
    }
    pub fn button(&mut self, x: i32, y: i32, w: i32, label: &str, id: u32, look: ButtonLook) -> Rect {
        let r = Rect::new(self.rect.x + x, self.rect.y + y, w, 13);
        self.elems.push(Elem::Button { r, label: label.into(), id, look });
        r
    }
    pub fn list(&mut self, r: Rect, rows: Vec<String>, sel: usize, id: u32, marker: bool) {
        let r = Rect::new(self.rect.x + r.x, self.rect.y + r.y, r.w, r.h);
        let n = rows.len();
        self.elems.push(Elem::List { r, rows, icons: vec![None; n], sel, top: 0, id, marker });
    }
    pub fn rel(&self, x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new(self.rect.x + x, self.rect.y + y, w, h)
    }

    pub fn list_mut(&mut self, id: u32) -> Option<(&mut usize, &mut usize, usize, i32)> {
        self.elems.iter_mut().find_map(|e| match e {
            Elem::List { sel, top, rows, r, id: lid, .. } if *lid == id => {
                let visible = ((r.h - 4) / 8).max(1);
                Some((sel, top, rows.len(), visible))
            }
            _ => None,
        })
    }

    pub fn input_text(&self) -> Option<&str> {
        self.elems.iter().find_map(|e| match e {
            Elem::Input { text, .. } => Some(text.as_str()),
            _ => None,
        })
    }

    pub fn draw(&self, f: &mut Frame, font: &Font, theme: &Theme, painter: &MapPainter, blink: bool) {
        let r = self.rect;
        match self.style {
            DlgStyle::Panel => ui::panel(f, r, theme.panel_bg, theme.panel_frame, theme.panel_shadow),
            DlgStyle::Box => ui::panel(f, r, theme.box_bg, theme.box_bg, theme.box_bg),
            DlgStyle::Report => {
                ui::panel(f, r, theme.report_bg, theme.report_bg, theme.report_shadow);
                f.fill(Rect::new(r.right(), r.y + 2, 2, r.h), theme.report_shadow);
                f.fill(Rect::new(r.x + 2, r.bottom(), r.w, 2), theme.report_shadow);
            }
            DlgStyle::MapWindow => {
                ui::panel(f, r, WHITE, WHITE, MAGENTA);
                f.fill(Rect::new(r.right(), r.y + 2, 2, r.h), MAGENTA);
                f.fill(Rect::new(r.x + 2, r.bottom(), r.w, 2), MAGENTA);
            }
        }
        if let Some(t) = &self.title {
            let (bg, fr, fg) = match self.style {
                DlgStyle::Panel => (theme.plate_bg, CYAN, theme.plate_text),
                DlgStyle::Report | DlgStyle::MapWindow => (theme.report_accent, WHITE, WHITE),
                DlgStyle::Box => (theme.plate_bg, theme.plate_frame, theme.plate_text),
            };
            ui::plate(f, font, r.x + r.w / 2, r.y + 3, t, bg, fr, fg);
        }
        for e in &self.elems {
            match e {
                Elem::Text { x, y, text, color } => {
                    font.draw(f, *x, *y, text, *color, None);
                }
                Elem::HotText { x, y, text, color } => {
                    let hot = if self.style == DlgStyle::Panel { CYAN } else { theme.hot };
                    ui::draw_hot(f, font, *x, *y, text, *color, hot, None);
                }
                Elem::Button { r, label, look, id } => {
                    ui::button(f, font, *r, label, *look, self.default_button == Some(*id))
                }
                Elem::List { r, rows, icons, sel, top, marker, .. } => {
                    let has_icons = icons.iter().any(|i| i.is_some());
                    let text_r = if has_icons {
                        // Icons sit in a black column to the left, as in the original.
                        let col = Rect::new(r.x - 12, r.y, 10, r.h);
                        f.fill(col, BLACK);
                        *r
                    } else {
                        *r
                    };
                    ui::list_box(f, text_r);
                    let visible = ((r.h - 4) / 8).max(1) as usize;
                    for (i, row) in rows.iter().enumerate().skip(*top).take(visible) {
                        let y = r.y + 2 + (i - top) as i32 * 8;
                        let selected = i == *sel;
                        if selected && !*marker {
                            f.fill(Rect::new(r.x + 1, y, r.w - 2, 8), BLACK);
                        }
                        let fg = if selected && !*marker { WHITE } else { BLACK };
                        if *marker && selected {
                            font.draw(f, r.x + 3, y, "*", BLACK, None);
                        }
                        font.draw(f, r.x + 3 + if *marker { 2 * font.advance } else { 0 }, y, row, fg, None);
                        if let Some(Some(art)) = icons.get(i) {
                            painter.draw_art(f, r.x - 11, y, *art);
                        }
                    }
                    if rows.len() > visible {
                        let track = Rect::new(r.right() - 7, r.y + 1, 6, r.h - 2);
                        f.fill(track, WHITE);
                        f.frame(track, BLACK);
                        font.draw(f, track.x + 1, track.y, "^", BLACK, None);
                        font.draw(f, track.x + 1, track.bottom() - 8, "V", BLACK, None);
                        let inner = Rect::new(track.x + 1, track.y + 8, 4, track.h - 16);
                        ui::vscroll(f, inner, *top, visible, rows.len(), WHITE, BLACK);
                    }
                }
                Elem::Input { r, text, .. } => {
                    // Black field, cyan frame (2px right and bottom), white block cursor.
                    f.fill(*r, CYAN);
                    f.fill(Rect::new(r.x + 1, r.y + 1, r.w - 3, r.h - 3), BLACK);
                    let tx = r.x + 4;
                    let ty = r.y + (r.h - font.cell_h) / 2;
                    let cx = font.draw(f, tx, ty, text, WHITE, None);
                    if blink {
                        f.fill(Rect::new(cx, ty, font.advance, font.cell_h), WHITE);
                    }
                }
                Elem::Icon { x, y, art } => painter.draw_art(f, *x, *y, *art),
                Elem::Fill { r, color } => f.fill(*r, *color),
                Elem::Frame { r, color } => f.frame(*r, *color),
                Elem::Slider { r, value, .. } => {
                    // [<] [track with thumb] [>]
                    f.fill(*r, BLACK);
                    let inner = r.inset(1);
                    f.fill(inner, WHITE);
                    let lb = Rect::new(r.x, r.y, 8, r.h);
                    let rb = Rect::new(r.right() - 8, r.y, 8, r.h);
                    f.frame(lb, BLACK);
                    f.frame(rb, BLACK);
                    font.draw(f, lb.x + 2, r.y + (r.h - 8) / 2, "<", BLACK, None);
                    font.draw(f, rb.x + 2, r.y + (r.h - 8) / 2, ">", BLACK, None);
                    let track = Rect::new(lb.right(), r.y + 1, rb.x - lb.right(), r.h - 2);
                    let tx = track.x + (*value as i32 * (track.w - 6)) / 100;
                    f.fill(Rect::new(tx, track.y, 6, track.h), BLACK);
                }
                Elem::Custom { .. } => {}
            }
        }
    }

    /// Handle a key. Returns an event for the app, or None if consumed/ignored.
    pub fn key(&mut self, ev: KeyEvent) -> Option<DlgEvent> {
        // Text input takes printable keys first.
        if let Some(Elem::Input { text, max, .. }) = self.elems.iter_mut().find(|e| matches!(e, Elem::Input { .. })) {
            match ev.key {
                Key::Char(c) if !ev.ctrl && !ev.alt && (c.is_ascii_graphic() || c == ' ') => {
                    if text.chars().count() < *max {
                        text.push(c.to_ascii_uppercase());
                    }
                    return None;
                }
                Key::Space => {
                    if text.chars().count() < *max {
                        text.push(' ');
                    }
                    return None;
                }
                Key::Backspace => {
                    text.pop();
                    return None;
                }
                Key::Enter => return Some(DlgEvent::Submit),
                Key::Esc => return self.cancel_button.map(DlgEvent::Button),
                _ => {}
            }
        }
        // Lists take arrows.
        if let Some(Elem::List { sel, top, rows, r, id, .. }) = self.elems.iter_mut().find(|e| matches!(e, Elem::List { .. })) {
            let n = rows.len();
            let visible = ((r.h - 4) / 8).max(1) as usize;
            let mut moved = None;
            match ev.key {
                Key::Up | Key::Pad(8) if *sel > 0 => moved = Some(*sel - 1),
                Key::Down | Key::Pad(2) if *sel + 1 < n => moved = Some(*sel + 1),
                Key::PageUp | Key::Pad(9) => moved = Some(sel.saturating_sub(visible)),
                Key::PageDown | Key::Pad(3) if n > 0 => moved = Some((*sel + visible).min(n - 1)),
                Key::Home | Key::Pad(7) => moved = Some(0),
                Key::End | Key::Pad(1) if n > 0 => moved = Some(n - 1),
                _ => {}
            }
            if let Some(m) = moved {
                *sel = m;
                if *sel < *top {
                    *top = *sel;
                }
                if *sel >= *top + visible {
                    *top = *sel + 1 - visible;
                }
                return Some(DlgEvent::ListSelect(*id, m));
            }
        }
        match ev.key {
            Key::Enter => {
                if let Some(id) = self.default_button {
                    return Some(DlgEvent::Button(id));
                }
            }
            Key::Esc => {
                if let Some(id) = self.cancel_button {
                    return Some(DlgEvent::Button(id));
                }
            }
            _ => {}
        }
        // Button hotkeys.
        if let Some(letter) = ev.letter().or(ev.alt_letter()) {
            for e in &self.elems {
                if let Elem::Button { label, id, look, .. } = e {
                    if *look != ButtonLook::Disabled && ui::hot_char(label) == Some(letter) {
                        return Some(DlgEvent::Button(*id));
                    }
                }
            }
        }
        Some(DlgEvent::Key(ev))
    }

    pub fn click(&mut self, x: i32, y: i32, double: bool) -> Option<DlgEvent> {
        for e in self.elems.iter_mut() {
            match e {
                Elem::Button { r, id, look, .. } if r.contains(x, y) && *look != ButtonLook::Disabled => {
                    return Some(DlgEvent::Button(*id));
                }
                Elem::List { r, rows, sel, top, id, .. } if r.contains(x, y) => {
                    let visible = ((r.h - 4) / 8).max(1) as usize;
                    if rows.len() > visible && x >= r.right() - 7 {
                        if y < r.y + r.h / 2 {
                            *top = top.saturating_sub(visible);
                        } else {
                            *top = (*top + visible).min(rows.len() - visible);
                        }
                        return None;
                    }
                    let row = ((y - r.y - 2) / 8).max(0) as usize + *top;
                    if row < rows.len() {
                        *sel = row;
                        return Some(if double { DlgEvent::ListActivate(*id, row) } else { DlgEvent::ListSelect(*id, row) });
                    }
                    return None;
                }
                Elem::Slider { r, value, id } if r.contains(x, y) => {
                    let step = if x < r.x + 8 {
                        -1
                    } else if x >= r.right() - 8 {
                        1
                    } else if x < r.x + 8 + (*value as i32 * (r.w - 22)) / 100 {
                        -10
                    } else {
                        10
                    };
                    return Some(DlgEvent::Slider(*id, step));
                }
                _ => {}
            }
        }
        if self.rect.contains(x, y) {
            Some(DlgEvent::Click(x, y))
        } else {
            None
        }
    }
}
