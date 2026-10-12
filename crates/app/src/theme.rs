//! Colour roles for the user interface, taken from the original CGA screens.

use crate::gfx::*;

#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: u8,
    pub text: u8,
    pub hot: u8,
    pub disabled: u8,
    pub rule: u8,
    pub divider: u8,
    pub hl_bg: u8,
    pub hl_fg: u8,
    /// Dialog panels (production, opponents, info...).
    pub panel_bg: u8,
    pub panel_frame: u8,
    pub panel_shadow: u8,
    pub panel_text: u8,
    /// Title plates and buttons.
    pub plate_bg: u8,
    pub plate_frame: u8,
    pub plate_text: u8,
    /// Message boxes.
    pub box_bg: u8,
    pub box_text: u8,
    /// Report windows (war report, ship report).
    pub report_bg: u8,
    pub report_shadow: u8,
    pub report_text: u8,
    pub report_accent: u8,
    pub report_list_bg: u8,
    /// Row/column number strips around the map.
    pub strip_bg: u8,
    pub strip_text: u8,
    pub players: [u8; 6],
}

impl Theme {
    pub fn cga() -> Theme {
        Theme {
            bg: BLACK,
            text: WHITE,
            hot: MAGENTA,
            disabled: CYAN,
            rule: WHITE,
            divider: CYAN,
            hl_bg: WHITE,
            hl_fg: BLACK,
            panel_bg: MAGENTA,
            panel_frame: WHITE,
            panel_shadow: WHITE,
            panel_text: WHITE,
            plate_bg: BLACK,
            plate_frame: WHITE,
            plate_text: WHITE,
            box_bg: WHITE,
            box_text: BLACK,
            report_bg: CYAN,
            report_shadow: MAGENTA,
            report_text: BLACK,
            report_accent: MAGENTA,
            report_list_bg: WHITE,
            strip_bg: MAGENTA,
            strip_text: WHITE,
            players: [WHITE, MAGENTA, CYAN, YELLOW, LIGHT_GREEN, LIGHT_RED],
        }
    }

    pub fn player(&self, p: u8) -> u8 {
        self.players[(p.max(1) as usize - 1) % 6]
    }
}
