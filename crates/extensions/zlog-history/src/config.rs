use zlog_ui::Section;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Source {
    pub path: String,
    pub contest: String,
}

impl Source {
    pub fn applies_to(&self, contest: &str) -> bool {
        let own = self.contest.trim();
        own.is_empty() || own.eq_ignore_ascii_case(contest.trim())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    pub name_memo: bool,
    pub sources: Vec<Source>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            name_memo: true,
            sources: Vec::new(),
        }
    }
}

fn flag(section: &Section, key: &str, default: bool) -> bool {
    section.get(key).map_or(default, |v| v == "true")
}

impl Config {
    pub fn load(section: &Section) -> Config {
        let d = Config::default();
        let mut numbers: Vec<u32> = section
            .keys()
            .filter_map(|k| k.strip_prefix("file")?.parse().ok())
            .collect();
        numbers.sort_unstable();
        let sources = numbers
            .into_iter()
            .map(|n| Source {
                path: section[&format!("file{n}")].clone(),
                contest: section
                    .get(&format!("contest{n}"))
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect();
        Config {
            enabled: flag(section, "enabled", d.enabled),
            name_memo: flag(section, "name_memo", d.name_memo),
            sources,
        }
    }

    pub fn save(&self, section: &mut Section) {
        section.retain(|k, _| !k.starts_with("file") && !k.starts_with("contest"));
        section.insert("enabled".into(), self.enabled.to_string());
        section.insert("name_memo".into(), self.name_memo.to_string());
        for (i, s) in self.sources.iter().enumerate() {
            section.insert(format!("file{}", i + 1), s.path.clone());
            section.insert(format!("contest{}", i + 1), s.contest.clone());
        }
    }

    pub fn sources_for(&self, contest: &str) -> Vec<Source> {
        self.sources
            .iter()
            .filter(|s| !s.path.trim().is_empty() && s.applies_to(contest))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_and_loaded_again() {
        let c = Config {
            enabled: false,
            name_memo: true,
            sources: vec![
                Source {
                    path: "a.zlox".into(),
                    contest: String::new(),
                },
                Source {
                    path: "b.txt".into(),
                    contest: "ALL JA1".into(),
                },
            ],
        };
        let mut s = Section::new();
        s.insert("file3".into(), "stale".into());
        c.save(&mut s);
        assert_eq!(Config::load(&s), c);
        assert_eq!(Config::load(&Section::new()), Config::default());
    }

    #[test]
    fn a_missing_number_skips_no_file_after_it() {
        let mut s = Section::new();
        s.insert("file1".into(), "a".into());
        s.insert("file3".into(), "c".into());
        s.insert("file10".into(), "j".into());
        let paths: Vec<String> = Config::load(&s)
            .sources
            .into_iter()
            .map(|s| s.path)
            .collect();
        assert_eq!(paths, ["a", "c", "j"]);
    }

    #[test]
    fn files_for_one_contest_or_every_contest() {
        let mut c = Config::default();
        for (path, contest) in [("a", ""), ("b", "all ja1"), ("c", "ACAG"), ("", "")] {
            c.sources.push(Source {
                path: path.into(),
                contest: contest.into(),
            });
        }
        let paths =
            |name| -> Vec<String> { c.sources_for(name).into_iter().map(|s| s.path).collect() };
        assert_eq!(paths("ALL JA1"), ["a", "b"]);
        assert_eq!(paths("JA0"), ["a"]);
    }
}
