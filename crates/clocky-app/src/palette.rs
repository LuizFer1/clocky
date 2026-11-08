//! The 10 project colors of the handoff (`tokens/base.tokens.json`, `project.*`).

/// Project colors in the handoff's order: name (PT-BR) and hex.
pub const COLORS: [(&str, &str); 10] = [
    ("Azul", "#3d8fd6"),
    ("Turquesa", "#2fb3a6"),
    ("Verde", "#3fae6a"),
    ("Lima", "#a3b83a"),
    ("Amarelo", "#d9b23a"),
    ("Laranja", "#e08a3b"),
    ("Vermelho", "#e05f5a"),
    ("Rosa", "#e06a9a"),
    ("Roxo", "#9b7be0"),
    ("Ardósia", "#8a94a6"),
];

/// Color preselected in the "Novo projeto" form (turquesa, as in screen 71).
pub const DEFAULT_INDEX: usize = 1;

/// Fallback for a stored color that is not a valid `#rrggbb` (ardósia).
const FALLBACK: [u8; 3] = [0x8a, 0x94, 0xa6];

/// Hex of the palette entry at `index`, or the default one when out of range.
pub fn hex(index: usize) -> &'static str {
    COLORS
        .get(index)
        .or(COLORS.get(DEFAULT_INDEX))
        .map_or("#2fb3a6", |(_, hex)| hex)
}

/// Index of `hex` in the palette (case-insensitive), if it is one of the 10 colors.
pub fn index_of(hex: &str) -> Option<usize> {
    COLORS
        .iter()
        .position(|(_, color)| color.eq_ignore_ascii_case(hex))
}

/// RGB of a `#rrggbb` string; ardósia when it is not one.
pub fn rgb(hex: &str) -> [u8; 3] {
    parse(hex).unwrap_or(FALLBACK)
}

fn parse(hex: &str) -> Option<[u8; 3]> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// The color as a Slint color.
pub fn color(hex: &str) -> slint::Color {
    let [r, g, b] = rgb(hex);
    slint::Color::from_rgb_u8(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colors() {
        assert_eq!(rgb("#3d8fd6"), [0x3d, 0x8f, 0xd6]);
        assert_eq!(rgb("#FFFFFF"), [255, 255, 255]);
    }

    #[test]
    fn invalid_colors_fall_back_to_slate() {
        for bad in ["", "3d8fd6", "#3d8fd", "#3d8fdz", "#ççç"] {
            assert_eq!(rgb(bad), FALLBACK, "{bad}");
        }
    }

    #[test]
    fn palette_lookup() {
        assert_eq!(hex(DEFAULT_INDEX), "#2fb3a6");
        assert_eq!(hex(99), "#2fb3a6");
        assert_eq!(index_of("#9B7BE0"), Some(8));
        assert_eq!(index_of("#000000"), None);
        assert!(COLORS.iter().all(|(_, hex)| parse(hex).is_some()));
    }
}
