//! Human-readable names for directions and ordinals.

use crate::geom::Dir;

pub fn dir_name(d: Dir) -> &'static str {
    ["East", "Northeast", "North", "Northwest", "West", "Southwest", "South", "Southeast"][d as usize & 7]
}

/// 1 -> "1st", 22 -> "22nd", 113 -> "113th".
pub fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn ordinals() {
        assert_eq!(super::ordinal(1), "1st");
        assert_eq!(super::ordinal(2), "2nd");
        assert_eq!(super::ordinal(3), "3rd");
        assert_eq!(super::ordinal(11), "11th");
        assert_eq!(super::ordinal(112), "112th");
        assert_eq!(super::ordinal(143), "143rd");
    }
}
