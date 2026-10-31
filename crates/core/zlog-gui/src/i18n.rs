use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

pub(crate) struct Language {
    pub code: &'static str,
    pub name: &'static str,
    text: &'static str,
}

pub(crate) const LANGUAGES: &[Language] = &[
    Language {
        code: "en",
        name: "English",
        text: include_str!("../lang/en.lang"),
    },
    Language {
        code: "ja",
        name: "日本語",
        text: include_str!("../lang/ja.lang"),
    },
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

fn tables() -> &'static [HashMap<&'static str, &'static str>] {
    static TABLES: OnceLock<Vec<HashMap<&'static str, &'static str>>> = OnceLock::new();
    TABLES.get_or_init(|| LANGUAGES.iter().map(|l| parse(l.text)).collect())
}

fn parse(text: &'static str) -> HashMap<&'static str, &'static str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect()
}

pub fn set_language(code: &str) {
    if let Some(i) = LANGUAGES.iter().position(|l| l.code == code) {
        CURRENT.store(i, Ordering::Relaxed);
    }
}

pub fn current() -> &'static str {
    LANGUAGES[CURRENT.load(Ordering::Relaxed)].code
}

pub(crate) fn set_from_environment() {
    let lang = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()));
    set_language(match lang {
        Some(l) if !l.starts_with("ja") => "en",
        _ => "ja",
    });
}

pub fn tr(key: &str) -> &'static str {
    let t = tables();
    t[CURRENT.load(Ordering::Relaxed)]
        .get(key)
        .or_else(|| t[0].get(key))
        .copied()
        .unwrap_or("?")
}

pub(crate) fn tr_in(code: &str, key: &str) -> &'static str {
    let t = tables();
    let i = LANGUAGES
        .iter()
        .position(|l| l.code == code)
        .unwrap_or(CURRENT.load(Ordering::Relaxed));
    t[i].get(key)
        .or_else(|| t[0].get(key))
        .copied()
        .unwrap_or("?")
}

pub fn trf(key: &str, args: &[&dyn Display]) -> String {
    zlog_core::text::substitute(tr(key), args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_keys_are_defined() {
        let files: Vec<_> = LANGUAGES.iter().map(|l| (l.code, l.text)).collect();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let undefined = zlog_core::text::Texts::new(&files).undefined_in(&dir);
        assert!(undefined.is_empty(), "{undefined:?}");
    }

    #[test]
    fn every_language_has_every_key() {
        let t = tables();
        for (lang, table) in LANGUAGES.iter().zip(t) {
            for key in t[0].keys() {
                assert!(table.contains_key(key), "{} lacks {key}", lang.code);
            }
        }
    }

    #[test]
    fn arguments_are_filled_in() {
        set_language("en");
        assert_eq!(trf("status.logged", &[&"JA1ZLO"]), "JA1ZLO logged");
        assert_eq!(
            trf("status.imported_refused", &[&"{1}", &2]),
            "{1} QSOs imported, 2 refused"
        );
    }
}
