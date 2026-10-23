//! Japanese text in egui, with a font of the system instead of one embedded.
//!
//! egui's own fonts have no kana or kanji. At start-up the fonts installed on the
//! computer are searched for a Japanese family, in the order below, and the first
//! one found is added to every egui family: first for Japanese, so that kanji take
//! Japanese glyph forms (not Chinese ones from a CJK font for another language), and
//! after egui's fonts otherwise, so that Latin text keeps egui's look and widths.
//! Nothing is redistributed: the font is read from the user's own system.
//! The toolbar's icon font (Phosphor Icons, embedded in zlog-ui) is added as a
//! family of its own whether or not a Japanese font is found.
//!
//! Windows of other crates, such as CW4ISR, draw geometric symbols (◧ ⬒ ◨ ●)
//! that egui's proportional fonts lack. egui's own monospace font, which has
//! most of them, and a symbol font of the system, searched like the Japanese
//! one, are added after every other font of the proportional family, so that
//! they fill only those gaps.

use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily};

/// Japanese families by preference: macOS, Windows, then common free fonts.
const FAMILIES: &[&str] = &[
    "Hiragino Sans",
    "Hiragino Kaku Gothic ProN",
    "Yu Gothic UI",
    "Yu Gothic",
    "Meiryo UI",
    "Meiryo",
    "MS UI Gothic",
    "MS Gothic",
    "Noto Sans CJK JP",
    "Noto Sans JP",
    "Source Han Sans JP",
    "IPAexGothic",
    "IPAGothic",
];

/// Symbol families by preference: macOS, Windows, then common free fonts.
const SYMBOLS: &[&str] = &[
    "Apple Symbols",
    "Segoe UI Symbol",
    "Noto Sans Symbols 2",
    "DejaVu Sans",
];

const NAME: &str = "japanese";

const SYMBOL: &str = "symbols";

/// egui's own monospace font, embedded by egui.
const HACK: &str = "Hack";

/// The fonts found at start-up; each search runs once.
static FOUND: OnceLock<Option<FontData>> = OnceLock::new();
static FOUND_SYMBOLS: OnceLock<Option<FontData>> = OnceLock::new();

/// Add a Japanese font of the system to egui, ordered for the current language,
/// and the icon font; called again when the language changes. Without a Japanese
/// font, Japanese shows as boxes.
pub fn install(ctx: &egui::Context) {
    let mut fonts = match FOUND.get_or_init(|| find(FAMILIES)) {
        Some(font) => definitions(font.clone(), crate::i18n::current() == "ja"),
        None => FontDefinitions::default(),
    };
    add_symbols(
        &mut fonts,
        FOUND_SYMBOLS.get_or_init(|| find(SYMBOLS)).clone(),
    );
    zlog_ui::icon::add_font(&mut fonts);
    ctx.set_fonts(fonts);
}

/// The first of `families` installed, as egui font data (with its index in a
/// font collection such as a `.ttc` file).
fn find(families: &[&str]) -> Option<FontData> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    families.iter().find_map(|family| {
        let query = fontdb::Query {
            families: &[fontdb::Family::Name(family)],
            weight: fontdb::Weight::NORMAL,
            ..Default::default()
        };
        let id = db.query(&query)?;
        db.with_face_data(id, |data, index| {
            let mut font = FontData::from_owned(data.to_vec());
            font.index = index;
            font
        })
    })
}

/// egui's fonts with `font` added to every family, first when the user interface
/// is in Japanese.
fn definitions(font: FontData, japanese_first: bool) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(NAME.into(), Arc::new(font));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let list = fonts.families.entry(family.clone()).or_default();
        // Digits and Latin letters of the monospace family stay egui's, so the
        // columns of the QSO table line up in either language.
        if japanese_first && family == FontFamily::Proportional {
            list.insert(0, NAME.into());
        } else {
            list.push(NAME.into());
        }
    }
    fonts
}

/// egui's monospace font, then `symbols` if found, at the end of the
/// proportional family.
fn add_symbols(fonts: &mut FontDefinitions, symbols: Option<FontData>) {
    let list = fonts.families.entry(FontFamily::Proportional).or_default();
    list.push(HACK.into());
    if let Some(font) = symbols {
        fonts.font_data.insert(SYMBOL.into(), Arc::new(font));
        list.push(SYMBOL.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy() -> FontData {
        FontData::from_static(&[])
    }

    #[test]
    fn japanese_first_only_for_proportional_in_japanese() {
        let f = definitions(dummy(), true);
        assert_eq!(f.families[&FontFamily::Proportional][0], NAME);
        assert_eq!(f.families[&FontFamily::Monospace].last().unwrap(), NAME);
        let f = definitions(dummy(), false);
        assert_eq!(f.families[&FontFamily::Proportional].last().unwrap(), NAME);
        assert_ne!(f.families[&FontFamily::Proportional][0], NAME);
    }

    /// The symbols of the CW4ISR window have a glyph in some font of the
    /// proportional family, even without a symbol font of the system.
    #[test]
    fn symbols_follow_every_other_font() {
        let mut f = FontDefinitions::default();
        add_symbols(&mut f, Some(dummy()));
        let list = &f.families[&FontFamily::Proportional];
        assert_eq!(list[list.len() - 2..], [HACK.to_string(), SYMBOL.into()]);
        assert!(f.font_data.contains_key(HACK));
        let mut f = FontDefinitions::default();
        add_symbols(&mut f, None);
        assert_eq!(f.families[&FontFamily::Proportional].last().unwrap(), HACK);
    }
}
