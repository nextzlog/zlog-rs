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
        substitute(self.get(language, key), args)
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

/// Replace `{0}`, `{1}`, ... in `template` by `args` in one left-to-right pass, so a
/// placeholder inside an argument stays as it is; unknown placeholders are kept.
pub fn substitute(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let arg = after.find('}').and_then(|close| {
            let i = after[..close].parse::<usize>().ok()?;
            Some((args.get(i)?, close))
        });
        match arg {
            Some((a, close)) => {
                out.push_str(&a.to_string());
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_in_arguments_are_kept() {
        let t = Texts::new(&[("en", "a = {0} and {1} {2} {x")]);
        assert_eq!(t.format("en", "a", &[&"{1}", &"B"]), "{1} and B {2} {x");
    }

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
