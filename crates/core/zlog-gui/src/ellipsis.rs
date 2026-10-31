use zlog_ui::Command;

use crate::commands::{list, CoreCommands, CoreState, CORE_GROUPS};

const DOTS: [&str; 4] = ["...", "…", "・・・", "．．．"];

fn walk(c: &Command, path: &str, out: &mut Vec<(String, Command)>) {
    let path = format!("{path}/{}", c.id);
    for child in &c.children {
        walk(child, &path, out);
    }
    out.push((path, c.clone()));
}

pub fn menu_items(lang: &str) -> Vec<(String, Command)> {
    let state = CoreState {
        unsaved: true,
        period: true,
        period_possible: true,
        formats: vec![("ADIF".into(), true, true)],
    };
    let core: Vec<CoreCommands> = CORE_GROUPS
        .iter()
        .map(|&group| CoreCommands {
            group,
            state: state.clone(),
            chosen: None,
        })
        .collect();
    let panels = crate::registry::panels();
    let mut out = Vec::new();
    for l in list(&core, panels.iter().map(|p| (p.as_ref(), false)), lang) {
        for c in &l.commands {
            walk(c, &l.id, &mut out);
        }
    }
    for f in crate::registry::settings_files() {
        for c in f.commands(lang) {
            walk(&c, &format!("file:{}", f.id()), &mut out);
        }
    }
    let qso = zlog_core::Qso::default();
    for p in &panels {
        for it in p.qso_menu(lang, &qso) {
            let mut c = Command::new(it.id, it.label, "");
            c.dialog = it.dialog;
            out.push((format!("qso:{}/{}", p.id(), c.id), c));
        }
    }
    out
}

pub fn classify(dialog: &[&str], plain: &[&str]) {
    let mut seen: Option<Vec<(String, bool)>> = None;
    for lang in ["en", "ja"] {
        let mut found = Vec::new();
        for (id, c) in menu_items(lang) {
            for name in [c.menu_name(), c.label.as_str()] {
                assert!(
                    !DOTS.iter().any(|d| name.contains(d)),
                    "{lang} {id}: {name}"
                );
                assert_eq!(name, name.trim_end(), "{lang} {id}");
            }
            let text = c.menu_text();
            assert_eq!(text.ends_with('…'), c.dialog, "{lang} {id}: {text}");
            assert!(!text.ends_with(" …"), "{lang} {id}");
            if !c.children.is_empty() || c.checkable || c.radio {
                assert!(!c.dialog, "{lang} {id}");
            }
            found.push((id, c.dialog));
        }
        found.sort();
        found.dedup();
        match &seen {
            Some(en) => assert_eq!(&found, en, "en and ja differ"),
            None => seen = Some(found),
        }
    }
    let ids = |want: bool| -> Vec<String> {
        let found = seen.iter().flatten().filter(|(_, d)| *d == want);
        found.map(|(id, _)| id.clone()).collect()
    };
    let sorted = |v: &[&str]| -> Vec<String> {
        let mut v: Vec<String> = v.iter().map(|s| s.to_string()).collect();
        v.sort();
        v
    };
    assert_eq!(ids(true), sorted(dialog), "commands needing more input");
    assert_eq!(ids(false), sorted(plain), "commands completing at once");
}

pub fn lang_files(root: &std::path::Path) {
    let layers = std::fs::read_dir(root.join("crates")).unwrap();
    let crates = layers
        .flatten()
        .flat_map(|l| std::fs::read_dir(l.path()).into_iter().flatten().flatten());
    let mut n = 0;
    for dir in crates {
        for lang in ["en", "ja"] {
            let path = dir.path().join("lang").join(format!("{lang}.lang"));
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            n += 1;
            for line in text.lines() {
                for d in ["...", "・・・", "．．．", " …"] {
                    assert!(!line.contains(d), "{}: {line}", path.display());
                }
            }
        }
    }
    assert!(n > 0);
}
