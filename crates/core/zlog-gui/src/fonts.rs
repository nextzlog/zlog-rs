use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily};

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

const SYMBOLS: &[&str] = &[
    "Apple Symbols",
    "Segoe UI Symbol",
    "Noto Sans Symbols 2",
    "DejaVu Sans",
];

const NAME: &str = "japanese";

const SYMBOL: &str = "symbols";

const HACK: &str = "Hack";

static FOUND: OnceLock<Option<FontData>> = OnceLock::new();
static FOUND_SYMBOLS: OnceLock<Option<FontData>> = OnceLock::new();

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

fn definitions(font: FontData, japanese_first: bool) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(NAME.into(), Arc::new(font));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let list = fonts.families.entry(family.clone()).or_default();
        if japanese_first && family == FontFamily::Proportional {
            list.insert(0, NAME.into());
        } else {
            list.push(NAME.into());
        }
    }
    fonts
}

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
