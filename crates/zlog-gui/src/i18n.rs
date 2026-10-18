//! User-interface text by key.
//!
//! Each language is one file in `lang/` with lines `key = text`; `{0}`, `{1}` are
//! replaced by arguments. A key missing in a language falls back to English.
//! To add a language: add `lang/xx.lang` and one entry to [`LANGUAGES`].

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

pub struct Language {
    pub code: &'static str,
    /// Name of the language in itself, for the menu.
    pub name: &'static str,
    text: &'static str,
}

pub const LANGUAGES: &[Language] = &[
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

/// Choose the language by code; unknown codes are ignored.
pub fn set_language(code: &str) {
    if let Some(i) = LANGUAGES.iter().position(|l| l.code == code) {
        CURRENT.store(i, Ordering::Relaxed);
    }
}

pub fn current() -> &'static str {
    LANGUAGES[CURRENT.load(Ordering::Relaxed)].code
}

/// Language from the locale variables; without them (as on Windows) Japanese.
pub fn set_from_environment() {
    let lang = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()));
    set_language(match lang {
        Some(l) if !l.starts_with("ja") => "en",
        _ => "ja",
    });
}

/// Text for a key in the current language.
pub fn tr(key: &str) -> &'static str {
    let t = tables();
    t[CURRENT.load(Ordering::Relaxed)]
        .get(key)
        .or_else(|| t[0].get(key))
        .copied()
        .unwrap_or("?")
}

/// Text for a key with `{0}`, `{1}`, ... replaced.
pub fn trf(key: &str, args: &[&dyn Display]) -> String {
    let mut s = tr(key).to_string();
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), &a.to_string());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
