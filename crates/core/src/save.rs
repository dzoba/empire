//! Saved games.
//!
//! A save is the game's JSON, deflated and written as base64 text behind a
//! short tag, so it can live anywhere a string can (browser storage, a
//! download, the clipboard). Plain JSON saves from older versions still load.

use crate::game::Game;
use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::decompress_to_vec_with_limit;

/// Marks a compressed save, and its format version.
pub const SAVE_TAG: &str = "EMPIRE-SAVE-1:";

/// Saves larger than this when inflated are refused (a damaged or hostile file).
const MAX_INFLATED: usize = 256 << 20;

impl Game {
    /// The game as a compact save string.
    pub fn to_save(&self) -> String {
        let json = self.to_json();
        let packed = compress_to_vec(json.as_bytes(), 9);
        let mut out = String::with_capacity(SAVE_TAG.len() + packed.len() * 4 / 3 + 4);
        out.push_str(SAVE_TAG);
        base64::encode_into(&packed, &mut out);
        out
    }

    /// Load a save written by [`Game::to_save`], or a plain JSON save.
    pub fn from_save(text: &str) -> Result<Game, String> {
        let text = text.trim();
        match text.strip_prefix(SAVE_TAG) {
            Some(body) => {
                let packed = base64::decode(body).ok_or("not a valid save (bad encoding)")?;
                let json = decompress_to_vec_with_limit(&packed, MAX_INFLATED).map_err(|e| format!("not a valid save ({e:?})"))?;
                let json = String::from_utf8(json).map_err(|_| "not a valid save (bad text)")?;
                Game::from_json(&json)
            }
            None => Game::from_json(text),
        }
    }
}

/// Standard base64 (RFC 4648) with padding.
pub mod base64 {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode_into(data: &[u8], out: &mut String) {
        for chunk in data.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char);
                } else {
                    out.push('=');
                }
            }
        }
    }

    pub fn encode(data: &[u8]) -> String {
        let mut s = String::new();
        encode_into(data, &mut s);
        s
    }

    /// Decode, ignoring whitespace. None if the text isn't base64.
    pub fn decode(text: &str) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(text.len() * 3 / 4);
        let mut acc = 0u32;
        let mut bits = 0;
        for c in text.bytes() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => break,
                b' ' | b'\n' | b'\r' | b'\t' => continue,
                _ => return None,
            };
            acc = acc << 6 | v as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }
        Some(out)
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn roundtrip() {
            for n in 0..40usize {
                let data: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
                assert_eq!(super::decode(&super::encode(&data)).unwrap(), data);
            }
            assert_eq!(super::encode(b"Man"), "TWFu");
            assert_eq!(super::encode(b"Ma"), "TWE=");
            assert_eq!(super::encode(b"M"), "TQ==");
            assert!(super::decode("not base64!").is_none());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::GameConfig;
    use crate::game::Game;

    #[test]
    fn compact_and_plain_saves_load() {
        let mut g = Game::new(&GameConfig::default()).unwrap();
        for _ in 0..20 {
            g.advance(50);
        }
        let json = g.to_json();
        let save = g.to_save();
        assert!(save.starts_with(super::SAVE_TAG));
        assert!(save.len() * 4 < json.len(), "save {} vs json {}", save.len(), json.len());
        assert_eq!(Game::from_save(&save).unwrap().to_json(), json);
        assert_eq!(Game::from_save(&json).unwrap().to_json(), json);
        assert!(Game::from_save("EMPIRE-SAVE-1:AAAA").is_err());
    }
}
