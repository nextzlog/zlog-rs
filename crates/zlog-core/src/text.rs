//! User-interface text by key, for the application and for every extension.
//!
//! Each crate keeps its text in `lang/<code>.lang` files, one `key = text` per line,
//! with `{0}`, `{1}` for arguments, and embeds them with `include_str!`. Source code
//! holds only keys, so no language is written in it. The first table is the
//! fallback (English by convention).

use std::collections::HashMap;
use std::fmt::Display;

pub struct Texts {
    tables: Vec<(&'static str, HashMap<&'static str, &'static str>)>,
}

impl Texts {
    /// Tables as (language code, file contents), fallback first.
    pub fn new(files: &[(&'static str, &'static str)]) -> Self {
        let tables = files
            .iter()
            .map(|(code, text)| (*code, parse(text)))
            .collect();
        Texts { tables }
    }

    /// Text for `key` in `language`, else in the fallback language, else the key itself.
    pub fn get<'a>(&self, language: &str, key: &'a str) -> &'a str
    where
        'static: 'a,
    {
        let find = |code: &str| {
            self.tables
                .iter()
                .find(|(c, _)| *c == code)
                .and_then(|(_, t)| t.get(key).copied())
        };
        find(language)
            .or_else(|| self.tables.first().and_then(|(_, t)| t.get(key).copied()))
            .unwrap_or(key)
    }

    /// [`Texts::get`] with `{0}`, `{1}`, ... replaced by `args`.
    pub fn format(&self, language: &str, key: &str, args: &[&dyn Display]) -> String {
        let mut s = self.get(language, key).to_string();
        for (i, a) in args.iter().enumerate() {
            s = s.replace(&format!("{{{i}}}"), &a.to_string());
        }
        s
    }

    /// Keys of the fallback table missing in another language, for tests.
    pub fn missing(&self) -> Vec<(&'static str, &'static str)> {
        let Some((_, first)) = self.tables.first() else {
            return Vec::new();
        };
        let mut out: Vec<_> = self
            .tables
            .iter()
            .skip(1)
            .flat_map(|(code, t)| {
                first
                    .keys()
                    .filter(|k| !t.contains_key(*k))
                    .map(move |k| (*code, *k))
            })
            .collect();
        out.sort();
        out
    }
}

fn parse(text: &'static str) -> HashMap<&'static str, &'static str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect()
}

/// Define `fn texts() -> &'static Texts` from `lang/en.lang` and `lang/ja.lang` of the
/// calling crate.
#[macro_export]
macro_rules! texts {
    () => {
        fn texts() -> &'static $crate::text::Texts {
            static TEXTS: ::std::sync::OnceLock<$crate::text::Texts> = ::std::sync::OnceLock::new();
            TEXTS.get_or_init(|| {
                $crate::text::Texts::new(&[
                    (
                        "en",
                        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/lang/en.lang")),
                    ),
                    (
                        "ja",
                        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/lang/ja.lang")),
                    ),
                ])
            })
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_and_fallback() {
        let t = Texts::new(&[("en", "a = Apple {0}\nb = Banana"), ("ja", "a = A-ja {0}")]);
        assert_eq!(t.get("ja", "a"), "A-ja {0}");
        assert_eq!(t.get("ja", "b"), "Banana");
        assert_eq!(t.get("fr", "a"), "Apple {0}");
        assert_eq!(t.get("en", "zzz"), "zzz");
        assert_eq!(t.format("en", "a", &[&3]), "Apple 3");
        assert_eq!(t.missing(), vec![("ja", "b")]);
    }
}
