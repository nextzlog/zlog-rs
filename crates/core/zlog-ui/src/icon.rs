// Glyphs of Phosphor Icons (MIT, Copyright (c) 2023 Phosphor Icons); see this crate's LICENSE.
// Codepoints from the egui-phosphor crate (MIT OR Apache-2.0, Copyright (c) 2023 Romet Tagobert).

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

pub const FAMILY: &str = "phosphor";

pub fn family() -> FontFamily {
    FontFamily::Name(FAMILY.into())
}

pub const FONT: &[u8] = include_bytes!("../data/Phosphor.ttf");

pub fn add_font(fonts: &mut FontDefinitions) {
    fonts
        .font_data
        .insert(FAMILY.into(), Arc::new(FontData::from_static(FONT)));
    fonts.families.insert(family(), vec![FAMILY.into()]);
}

pub const ARROWS_LEFT_RIGHT: &str = "\u{E0A0}";
pub const ARROW_CLOCKWISE: &str = "\u{E036}";
pub const ARROW_COUNTER_CLOCKWISE: &str = "\u{E038}";
pub const BOOK_OPEN: &str = "\u{E0E6}";
pub const BROADCAST: &str = "\u{E0F2}";
pub const BROOM: &str = "\u{EC54}";
pub const CALCULATOR: &str = "\u{E538}";
pub const CHART_LINE: &str = "\u{E154}";
pub const CHAT_CIRCLE: &str = "\u{E168}";
pub const CHECK_SQUARE: &str = "\u{E186}";
pub const CLOCK_COUNTER_CLOCKWISE: &str = "\u{E1A0}";
pub const CLOUD_ARROW_UP: &str = "\u{E1AE}";
pub const CLOUD_LIGHTNING: &str = "\u{E1B2}";
pub const DISCORD_LOGO: &str = "\u{E61A}";
pub const DOWNLOAD_SIMPLE: &str = "\u{E20C}";
pub const EAR: &str = "\u{E70C}";
pub const ERASER: &str = "\u{E21E}";
pub const FILE_ARROW_UP: &str = "\u{E61E}";
pub const FILE_TEXT: &str = "\u{E23A}";
pub const FLAG: &str = "\u{E244}";
pub const FLOPPY_DISK: &str = "\u{E248}";
pub const FOLDER_OPEN: &str = "\u{E256}";
pub const FUNNEL: &str = "\u{E266}";
pub const GEAR: &str = "\u{E270}";
pub const GLOBE: &str = "\u{E288}";
pub const HOURGLASS: &str = "\u{E2B2}";
pub const GRID_FOUR: &str = "\u{E296}";
pub const HASH: &str = "\u{E2A2}";
pub const IDENTIFICATION_CARD: &str = "\u{E2C8}";
pub const KEYBOARD: &str = "\u{E2D8}";
pub const LINK: &str = "\u{E2E2}";
pub const LIST_CHECKS: &str = "\u{EADC}";
pub const LIST_MAGNIFYING_GLASS: &str = "\u{EBE0}";
pub const MAGNIFYING_GLASS: &str = "\u{E30C}";
pub const MAP_TRIFOLD: &str = "\u{E31A}";
pub const MEGAPHONE: &str = "\u{E324}";
pub const MICROPHONE: &str = "\u{E326}";
pub const NETWORK: &str = "\u{EDDE}";
pub const NOTE_PENCIL: &str = "\u{E34C}";
pub const NUMPAD: &str = "\u{E3C8}";
pub const MINUS: &str = "\u{E32A}";
pub const PAPER_PLANE_TILT: &str = "\u{E398}";
pub const PENCIL_SIMPLE: &str = "\u{E3B4}";
pub const PLAY: &str = "\u{E3D0}";
pub const PLUGS: &str = "\u{EB56}";
pub const PLUGS_CONNECTED: &str = "\u{EB5A}";
pub const PLUS: &str = "\u{E3D4}";
pub const PLUS_MINUS: &str = "\u{E3D8}";
pub const RADIO: &str = "\u{E77E}";
pub const RECORD: &str = "\u{E3EE}";
pub const ROWS: &str = "\u{E5A2}";
pub const SIDEBAR: &str = "\u{EAB6}";
pub const SIDEBAR_SIMPLE: &str = "\u{EC24}";
pub const SLACK_LOGO: &str = "\u{E5A8}";
pub const SPEAKER_HIGH: &str = "\u{E44A}";
pub const SQUARES_FOUR: &str = "\u{E464}";
pub const STACK: &str = "\u{E466}";
pub const STOP: &str = "\u{E46C}";
pub const SWAP: &str = "\u{E83C}";
pub const TERMINAL_WINDOW: &str = "\u{EAE8}";
pub const TEXTBOX: &str = "\u{EB0A}";
pub const TRASH: &str = "\u{E4A6}";
pub const TROPHY: &str = "\u{E67E}";
pub const UPLOAD_SIMPLE: &str = "\u{E4C0}";
pub const USB: &str = "\u{E956}";
pub const USERS: &str = "\u{E4D6}";
pub const USER_CHECK: &str = "\u{EAFA}";
pub const USER_LIST: &str = "\u{E73C}";
pub const WAVEFORM: &str = "\u{E802}";
pub const WARNING: &str = "\u{E4E0}";
pub const X_CIRCLE: &str = "\u{E4F8}";

pub const ALL: &[(&str, &str)] = &[
    ("ARROWS_LEFT_RIGHT", ARROWS_LEFT_RIGHT),
    ("ARROW_CLOCKWISE", ARROW_CLOCKWISE),
    ("ARROW_COUNTER_CLOCKWISE", ARROW_COUNTER_CLOCKWISE),
    ("BOOK_OPEN", BOOK_OPEN),
    ("BROADCAST", BROADCAST),
    ("BROOM", BROOM),
    ("CALCULATOR", CALCULATOR),
    ("CHART_LINE", CHART_LINE),
    ("CHAT_CIRCLE", CHAT_CIRCLE),
    ("CHECK_SQUARE", CHECK_SQUARE),
    ("CLOCK_COUNTER_CLOCKWISE", CLOCK_COUNTER_CLOCKWISE),
    ("CLOUD_ARROW_UP", CLOUD_ARROW_UP),
    ("CLOUD_LIGHTNING", CLOUD_LIGHTNING),
    ("DISCORD_LOGO", DISCORD_LOGO),
    ("DOWNLOAD_SIMPLE", DOWNLOAD_SIMPLE),
    ("EAR", EAR),
    ("ERASER", ERASER),
    ("FILE_ARROW_UP", FILE_ARROW_UP),
    ("FILE_TEXT", FILE_TEXT),
    ("FLAG", FLAG),
    ("FLOPPY_DISK", FLOPPY_DISK),
    ("FOLDER_OPEN", FOLDER_OPEN),
    ("FUNNEL", FUNNEL),
    ("GEAR", GEAR),
    ("GLOBE", GLOBE),
    ("HOURGLASS", HOURGLASS),
    ("GRID_FOUR", GRID_FOUR),
    ("HASH", HASH),
    ("IDENTIFICATION_CARD", IDENTIFICATION_CARD),
    ("KEYBOARD", KEYBOARD),
    ("LINK", LINK),
    ("LIST_CHECKS", LIST_CHECKS),
    ("LIST_MAGNIFYING_GLASS", LIST_MAGNIFYING_GLASS),
    ("MAGNIFYING_GLASS", MAGNIFYING_GLASS),
    ("MAP_TRIFOLD", MAP_TRIFOLD),
    ("MEGAPHONE", MEGAPHONE),
    ("MICROPHONE", MICROPHONE),
    ("NETWORK", NETWORK),
    ("NOTE_PENCIL", NOTE_PENCIL),
    ("NUMPAD", NUMPAD),
    ("MINUS", MINUS),
    ("PAPER_PLANE_TILT", PAPER_PLANE_TILT),
    ("PENCIL_SIMPLE", PENCIL_SIMPLE),
    ("PLAY", PLAY),
    ("PLUGS", PLUGS),
    ("PLUGS_CONNECTED", PLUGS_CONNECTED),
    ("PLUS", PLUS),
    ("PLUS_MINUS", PLUS_MINUS),
    ("RADIO", RADIO),
    ("RECORD", RECORD),
    ("ROWS", ROWS),
    ("SIDEBAR", SIDEBAR),
    ("SIDEBAR_SIMPLE", SIDEBAR_SIMPLE),
    ("SLACK_LOGO", SLACK_LOGO),
    ("SPEAKER_HIGH", SPEAKER_HIGH),
    ("SQUARES_FOUR", SQUARES_FOUR),
    ("STACK", STACK),
    ("STOP", STOP),
    ("SWAP", SWAP),
    ("TERMINAL_WINDOW", TERMINAL_WINDOW),
    ("TEXTBOX", TEXTBOX),
    ("TRASH", TRASH),
    ("TROPHY", TROPHY),
    ("UPLOAD_SIMPLE", UPLOAD_SIMPLE),
    ("USB", USB),
    ("USERS", USERS),
    ("USER_CHECK", USER_CHECK),
    ("USER_LIST", USER_LIST),
    ("WAVEFORM", WAVEFORM),
    ("WARNING", WARNING),
    ("X_CIRCLE", X_CIRCLE),
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
