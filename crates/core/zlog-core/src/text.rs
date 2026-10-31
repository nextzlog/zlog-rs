use std::collections::HashMap;
use std::fmt::Display;
use std::path::Path;

pub struct Texts {
    tables: Vec<(&'static str, HashMap<&'static str, &'static str>)>,
}

impl Texts {
    pub fn new(files: &[(&'static str, &'static str)]) -> Self {
        let tables = files
            .iter()
            .map(|(code, text)| (*code, parse(text)))
            .collect();
        Texts { tables }
    }

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

    pub fn format(&self, language: &str, key: &str, args: &[&dyn Display]) -> String {
        substitute(self.get(language, key), args)
    }

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

    pub fn undefined_in(&self, dir: &Path) -> Vec<String> {
        let prefixes: Vec<&str> = self
            .tables
            .iter()
            .flat_map(|(_, t)| t.keys())
            .filter_map(|k| k.split_once('.').map(|(p, _)| p))
            .collect();
        let mut out: Vec<String> = sources(dir)
            .iter()
            .flat_map(|text| key_literals(text))
            .filter(|k| prefixes.contains(&k.split_once('.').map_or("", |(p, _)| p)))
            .filter(|k| self.tables.iter().any(|(_, t)| !t.contains_key(k.as_str())))
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

fn sources(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for path in entries.flatten().map(|e| e.path()) {
        if path.is_dir() {
            out.extend(sources(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let end = text.find("#[cfg(test)]").unwrap_or(text.len());
            out.push(text[..end].to_string());
        }
    }
    out
}

const LOOKUPS: [&str; 7] = ["tr", "trf", "tr_in", "t", "get", "format", "text"];

fn looked_up(code: &str) -> bool {
    code.rfind('(').is_some_and(|open| {
        let name = code[..open].trim_end();
        let name = &name[name
            .rfind(|c: char| !c.is_alphanumeric() && c != '_')
            .map_or(0, |i| i + 1)..];
        let args = &code[open + 1..];
        LOOKUPS.contains(&name)
            && !args.contains(')')
            && (args.trim().is_empty() || args.trim_end().ends_with(','))
    })
}

fn key_literals(text: &str) -> Vec<String> {
    let parts: Vec<&str> = text.split('"').collect();
    parts
        .iter()
        .enumerate()
        .skip(1)
        .step_by(2)
        .filter(|(i, _)| looked_up(parts[i - 1]))
        .map(|(_, s)| *s)
        .filter(|s| {
            s.starts_with(|c: char| c.is_ascii_lowercase())
                && s.contains('.')
                && !s.ends_with('.')
                && !s.contains("..")
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
        })
        .map(str::to_string)
        .collect()
}

fn parse(text: &'static str) -> HashMap<&'static str, &'static str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect()
}

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

        #[cfg(test)]
        #[test]
        fn lang_keys_are_defined() {
            let dir = ::std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
            let undefined = texts().undefined_in(&dir);
            assert!(undefined.is_empty(), "{undefined:?}");
        }
    };
}

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

    #[test]
    fn key_literals_pick_dotted_keys() {
        let src = r#"tr(l, "a.b"); x("p.q"); t("c.d_1"); t("v1.2."); get("x"); "toolbar.{id}""#;
        assert_eq!(key_literals(src), ["a.b", "c.d_1"]);
        assert_eq!(
            key_literals(r#"cmd(l, "f.open", "m.open"); get(&f(x), "a.b")"#),
            [""; 0]
        );
    }
}
