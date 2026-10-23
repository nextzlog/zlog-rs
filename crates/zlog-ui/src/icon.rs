//! Toolbar icons: glyphs of Phosphor Icons (MIT, Copyright (c) 2023 Phosphor
//! Icons), regular weight, with the font embedded (see this crate's `LICENSE`).
//!
//! The codepoints are those of the egui-phosphor crate (MIT OR Apache-2.0,
//! Copyright (c) 2023 Romet Tagobert); the crate itself is not a dependency
//! because no release of it builds against this egui. Only the glyphs used are
//! listed. Extensions use these constants; the application calls [`add_font`].

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// The name of the font and of the egui family that holds only it.
pub const FAMILY: &str = "phosphor";

/// The family to draw an icon glyph with.
pub fn family() -> FontFamily {
    FontFamily::Name(FAMILY.into())
}

/// The regular Phosphor font.
pub const FONT: &[u8] = include_bytes!("../data/Phosphor.ttf");

/// Add the icon font to `fonts` as the family [`FAMILY`]. It is not added to
/// the text families: its private-use codepoints are drawn only where asked.
pub fn add_font(fonts: &mut FontDefinitions) {
    fonts
        .font_data
        .insert(FAMILY.into(), Arc::new(FontData::from_static(FONT)));
    fonts.families.insert(family(), vec![FAMILY.into()]);
}

pub const BROADCAST: &str = "\u{E0F2}";
pub const CHART_LINE: &str = "\u{E154}";
pub const CHECK_SQUARE: &str = "\u{E186}";
pub const CLOUD_LIGHTNING: &str = "\u{E1B2}";
pub const CLOCK_COUNTER_CLOCKWISE: &str = "\u{E1A0}";
pub const DOWNLOAD_SIMPLE: &str = "\u{E20C}";
pub const EAR: &str = "\u{E70C}";
pub const FILE_TEXT: &str = "\u{E23A}";
pub const FLOPPY_DISK: &str = "\u{E248}";
pub const FOLDER_OPEN: &str = "\u{E256}";
pub const GRID_FOUR: &str = "\u{E296}";
pub const IDENTIFICATION_CARD: &str = "\u{E2C8}";
pub const KEYBOARD: &str = "\u{E2D8}";
pub const LIST_MAGNIFYING_GLASS: &str = "\u{EBE0}";
pub const MAGNIFYING_GLASS: &str = "\u{E30C}";
pub const MAP_TRIFOLD: &str = "\u{E31A}";
pub const MEGAPHONE: &str = "\u{E324}";
pub const MICROPHONE: &str = "\u{E326}";
pub const NETWORK: &str = "\u{EDDE}";
pub const PLUGS: &str = "\u{EB56}";
pub const PLUGS_CONNECTED: &str = "\u{EB5A}";
pub const RADIO: &str = "\u{E77E}";
pub const STOP: &str = "\u{E46C}";
pub const TERMINAL_WINDOW: &str = "\u{EAE8}";
pub const TROPHY: &str = "\u{E67E}";
pub const UPLOAD_SIMPLE: &str = "\u{E4C0}";
pub const USB: &str = "\u{E956}";
pub const USER_CHECK: &str = "\u{EAFA}";
pub const USERS: &str = "\u{E4D6}";
pub const WAVEFORM: &str = "\u{E802}";

/// Every glyph listed here, by name (for tests).
pub const ALL: &[(&str, &str)] = &[
    ("BROADCAST", BROADCAST),
    ("CHART_LINE", CHART_LINE),
    ("CHECK_SQUARE", CHECK_SQUARE),
    ("CLOUD_LIGHTNING", CLOUD_LIGHTNING),
    ("CLOCK_COUNTER_CLOCKWISE", CLOCK_COUNTER_CLOCKWISE),
    ("DOWNLOAD_SIMPLE", DOWNLOAD_SIMPLE),
    ("EAR", EAR),
    ("FILE_TEXT", FILE_TEXT),
    ("FLOPPY_DISK", FLOPPY_DISK),
    ("FOLDER_OPEN", FOLDER_OPEN),
    ("GRID_FOUR", GRID_FOUR),
    ("IDENTIFICATION_CARD", IDENTIFICATION_CARD),
    ("KEYBOARD", KEYBOARD),
    ("LIST_MAGNIFYING_GLASS", LIST_MAGNIFYING_GLASS),
    ("MAGNIFYING_GLASS", MAGNIFYING_GLASS),
    ("MAP_TRIFOLD", MAP_TRIFOLD),
    ("MEGAPHONE", MEGAPHONE),
    ("MICROPHONE", MICROPHONE),
    ("NETWORK", NETWORK),
    ("PLUGS", PLUGS),
    ("PLUGS_CONNECTED", PLUGS_CONNECTED),
    ("RADIO", RADIO),
    ("STOP", STOP),
    ("TERMINAL_WINDOW", TERMINAL_WINDOW),
    ("TROPHY", TROPHY),
    ("UPLOAD_SIMPLE", UPLOAD_SIMPLE),
    ("USB", USB),
    ("USER_CHECK", USER_CHECK),
    ("USERS", USERS),
    ("WAVEFORM", WAVEFORM),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_is_embedded_and_added_as_its_own_family() {
        assert!(FONT.len() > 100_000);
        let mut f = FontDefinitions::default();
        let text = f.families[&FontFamily::Proportional].clone();
        add_font(&mut f);
        assert_eq!(f.families[&family()], vec![FAMILY.to_string()]);
        assert_eq!(f.families[&FontFamily::Proportional], text);
    }

    /// Every listed codepoint has a glyph in the embedded font.
    #[test]
    fn every_icon_is_in_the_font() {
        let face = ttf_parser::Face::parse(FONT, 0).unwrap();
        for (name, glyph) in ALL {
            let c = glyph.chars().next().unwrap();
            assert_eq!(glyph.chars().count(), 1, "{name}");
            assert!(face.glyph_index(c).is_some(), "{name} U+{:04X}", c as u32);
        }
    }
}
