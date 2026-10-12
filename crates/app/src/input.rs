//! Keyboard input, normalised from browser key events.

use empire_core::Dir;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Space,
    Enter,
    Esc,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
    /// Numeric keypad digit, regardless of Num Lock.
    Pad(u8),
    PadPlus,
    PadMinus,
    PadStar,
    PadSlash,
    PadDot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyEvent {
    pub fn plain(key: Key) -> Self {
        KeyEvent { key, ctrl: false, alt: false, shift: false }
    }

    /// Build from a DOM `KeyboardEvent`'s `key` and `code`.
    pub fn from_web(key: &str, code: &str, ctrl: bool, alt: bool, shift: bool) -> Option<KeyEvent> {
        let k = if let Some(rest) = code.strip_prefix("Numpad") {
            match rest {
                d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => Key::Pad(d.as_bytes()[0] - b'0'),
                "Add" => Key::PadPlus,
                "Subtract" => Key::PadMinus,
                "Multiply" => Key::PadStar,
                "Divide" => Key::PadSlash,
                "Decimal" => Key::PadDot,
                "Enter" => Key::Enter,
                _ => return None,
            }
        } else {
            match key {
                "Enter" => Key::Enter,
                "Escape" | "Esc" => Key::Esc,
                "Backspace" => Key::Backspace,
                "Tab" => Key::Tab,
                "ArrowUp" | "Up" => Key::Up,
                "ArrowDown" | "Down" => Key::Down,
                "ArrowLeft" | "Left" => Key::Left,
                "ArrowRight" | "Right" => Key::Right,
                "Home" => Key::Home,
                "End" => Key::End,
                "PageUp" => Key::PageUp,
                "PageDown" => Key::PageDown,
                "Insert" => Key::Insert,
                "Delete" => Key::Delete,
                " " | "Spacebar" => Key::Space,
                f if f.len() >= 2 && f.starts_with('F') && f[1..].chars().all(|c| c.is_ascii_digit()) => {
                    Key::F(f[1..].parse().ok()?)
                }
                s => {
                    let mut chars = s.chars();
                    let c = chars.next()?;
                    if chars.next().is_some() {
                        return None;
                    }
                    // With Alt or Ctrl held, browsers may report odd characters; use the code.
                    if (alt || ctrl) && code.starts_with("Key") && code.len() == 4 {
                        Key::Char(code.as_bytes()[3].to_ascii_lowercase() as char)
                    } else if (alt || ctrl) && code.starts_with("Digit") && code.len() == 6 {
                        Key::Char(code.as_bytes()[5] as char)
                    } else {
                        Key::Char(c)
                    }
                }
            }
        };
        Some(KeyEvent { key: k, ctrl, alt, shift })
    }

    /// Letter key without Ctrl/Alt, uppercased.
    pub fn letter(&self) -> Option<char> {
        match self.key {
            Key::Char(c) if !self.ctrl && !self.alt && c.is_ascii_alphabetic() => Some(c.to_ascii_uppercase()),
            _ => None,
        }
    }

    pub fn ctrl_letter(&self) -> Option<char> {
        match self.key {
            Key::Char(c) if self.ctrl && !self.alt => Some(c.to_ascii_uppercase()),
            _ => None,
        }
    }

    pub fn alt_letter(&self) -> Option<char> {
        match self.key {
            Key::Char(c) if self.alt && !self.ctrl => Some(c.to_ascii_uppercase()),
            _ => None,
        }
    }

    /// Movement direction: numeric keypad, arrows, Home/End/PgUp/PgDn.
    pub fn direction(&self) -> Option<Dir> {
        if self.ctrl || self.alt {
            return None;
        }
        Some(match self.key {
            Key::Pad(6) | Key::Right => 0,
            Key::Pad(9) | Key::PageUp => 1,
            Key::Pad(8) | Key::Up => 2,
            Key::Pad(7) | Key::Home => 3,
            Key::Pad(4) | Key::Left => 4,
            Key::Pad(1) | Key::End => 5,
            Key::Pad(2) | Key::Down => 6,
            Key::Pad(3) | Key::PageDown => 7,
            // The top-row digits work like the keypad for keyboards without one.
            Key::Char(c @ '1'..='9') if c != '5' => match c {
                '6' => 0,
                '9' => 1,
                '8' => 2,
                '7' => 3,
                '4' => 4,
                '1' => 5,
                '2' => 6,
                _ => 7,
            },
            _ => return None,
        })
    }
}
