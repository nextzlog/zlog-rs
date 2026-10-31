use std::path::{Path, PathBuf};

use zlog_ui::migration::Migration;
use zlog_ui::{Command, SettingsEnv, SettingsFile};

use crate::Sections;

zlog_core::texts!();

#[derive(Default)]
pub struct IniFile {
    asking: Option<(PathBuf, Sections)>,
    overwrite: Option<PathBuf>,
    told: Option<String>,
}

const IMPORT: &str = "ini.import";
const EXPORT: &str = "ini.export";

impl IniFile {
    fn export(&mut self, path: &Path, env: &SettingsEnv<'_>) {
        let (t, lang) = (texts(), env.language);
        self.told = Some(match export(path, &to_ini(env.sections, env.migrations)) {
            Ok((left_out, backup)) => {
                let mut text = t.format(lang, "exported", &[&path.display()]);
                if let Some(b) = backup {
                    text.push('\n');
                    text.push_str(&t.format(lang, "backup", &[&b.display()]));
                }
                if left_out > 0 {
                    text.push('\n');
                    text.push_str(&t.format(lang, "left_out", &[&left_out]));
                }
                text
            }
            Err(e) => t.format(lang, "write_failed", &[&e]),
        });
    }
}

impl SettingsFile for IniFile {
    fn id(&self) -> &str {
        "ini"
    }

    fn commands(&self, lang: &str) -> Vec<Command> {
        let t = texts();
        vec![
            Command::new(
                IMPORT,
                t.get(lang, "menu.import"),
                t.get(lang, "cmd.import.tip"),
            )
            .dialog(),
            Command::new(
                EXPORT,
                t.get(lang, "menu.export"),
                t.get(lang, "cmd.export.tip"),
            )
            .dialog(),
        ]
    }

    fn run_command(&mut self, id: &str, env: &mut SettingsEnv<'_>) {
        match id {
            IMPORT => {
                if let Some(path) = zlog_ui::dialog::import_file("zlog.ini", &["ini"]) {
                    match std::fs::read(&path) {
                        Ok(bytes) => {
                            let read = from_ini(crate::read(&bytes), env.migrations);
                            self.asking = Some((path, read));
                        }
                        Err(e) => {
                            self.told = Some(texts().format(env.language, "read_failed", &[&e]));
                        }
                    }
                }
            }
            EXPORT => match zlog_ui::dialog::export_file("zlog.ini", &["ini"]) {
                Some(to) if to.ask => self.overwrite = Some(to.path),
                Some(to) => self.export(&to.path, env),
                None => {}
            },
            _ => {}
        }
    }

    fn modal(&self) -> bool {
        self.overwrite.is_some()
    }

    fn dialogs(&mut self, ctx: &egui::Context, env: &mut SettingsEnv<'_>) -> bool {
        let t = texts();
        let lang = env.language;
        if let Some(path) = &self.overwrite {
            let question = t.format(lang, "overwrite", &[&path.display()]);
            if let Some(yes) = zlog_ui::dialog::ask_overwrite(ctx, &question, lang) {
                let path = self.overwrite.take().unwrap_or_default();
                if yes {
                    self.export(&path, env);
                }
            }
        }
        let mut done = false;
        if let Some((path, sections)) = &self.asking {
            let keys: usize = sections.values().map(|s| s.len()).sum();
            let (_, answer) = zlog_ui::dialog::modal_with_buttons(
                ctx,
                "import_ini",
                |ui| {
                    ui.label(t.format(lang, "ask", &[&path.display(), &sections.len(), &keys]));
                },
                |ui| zlog_ui::dialog::ok_cancel(ui, lang, true, true),
            );
            match answer {
                Some(true) => {
                    for (name, section) in sections {
                        let s = env.sections.entry(name.clone()).or_default();
                        for (k, v) in section {
                            s.insert(k.clone(), v.clone());
                        }
                    }
                    self.told = Some(match (env.save)(env.sections) {
                        Ok(()) => t.format(lang, "imported", &[&keys]),
                        Err(e) => t.format(lang, "write_failed", &[&e]),
                    });
                    self.asking = None;
                    done = true;
                }
                Some(false) => self.asking = None,
                None => {}
            }
        }
        if let Some(text) = &self.told {
            let (_, close) = zlog_ui::dialog::modal_with_buttons(
                ctx,
                "ini_result",
                |ui| {
                    ui.label(text);
                },
                |ui| {
                    ui.button(t.get(lang, "ok")).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter))
                },
            );
            if close {
                self.told = None;
            }
        }
        done
    }
}

fn copied(migrations: &[Migration]) -> impl Iterator<Item = (&(&str, &str), &(&str, &str))> {
    migrations.iter().filter_map(|m| match m {
        Migration::CopiedKey { from, to } => Some((from, to)),
        _ => None,
    })
}

pub fn to_ini(sections: &Sections, migrations: &[Migration]) -> Sections {
    let mut out = sections.clone();
    for (from, to) in copied(migrations) {
        let Some(value) = out.get_mut(to.0).and_then(|s| s.remove(to.1)) else {
            continue;
        };
        if out.get(to.0).is_some_and(|s| s.is_empty()) {
            out.remove(to.0);
        }
        out.entry(from.0.into())
            .or_default()
            .insert(from.1.into(), value);
    }
    out
}

pub fn from_ini(mut sections: Sections, migrations: &[Migration]) -> Sections {
    for (from, to) in copied(migrations) {
        let value = sections
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(from.0))
            .and_then(|(_, s)| s.iter().find(|(k, _)| k.eq_ignore_ascii_case(from.1)))
            .map(|(_, v)| v.clone());
        if let Some(value) = value {
            sections
                .entry(to.0.into())
                .or_default()
                .insert(to.1.into(), value);
        }
    }
    sections
}

fn export(path: &Path, sections: &Sections) -> std::io::Result<(usize, Option<PathBuf>)> {
    let old = std::fs::read(path).ok();
    let (bytes, left_out) = crate::merge(old.as_deref().unwrap_or_default(), sections);
    let backup = match old {
        Some(old) => {
            let b = path.with_extension("ini.bak");
            std::fs::write(&b, old)?;
            Some(b)
        }
        None => None,
    };
    std::fs::write(path, bytes)?;
    Ok((left_out, backup))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_merges_and_keeps_a_backup() {
        let dir = std::env::temp_dir().join(format!("zlog-ini-menu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zlog.ini");
        std::fs::write(&path, crate::encode("[Categories]\r\nCall=JA1ZLO\r\n")).unwrap();
        let mut s = Sections::new();
        s.entry("Categories".into())
            .or_default()
            .insert("Call".into(), "JA1YAA".into());
        s.entry("keyer".into())
            .or_default()
            .insert("pad".into(), "a\nb".into());
        let (left_out, backup) = export(&path, &s).unwrap();
        assert_eq!(left_out, 1);
        assert_eq!(backup, Some(dir.join("zlog.ini.bak")));
        let back = crate::read(&std::fs::read(&path).unwrap());
        assert_eq!(back["Categories"]["Call"], "JA1YAA");
        let old = crate::read(&std::fs::read(dir.join("zlog.ini.bak")).unwrap());
        assert_eq!(old["Categories"]["Call"], "JA1ZLO");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_copied_key_goes_back_to_its_zlog_section_and_returns() {
        let m = [Migration::CopiedKey {
            from: ("Preferences", "BackUpPath"),
            to: ("panel.backup", "BackUpPath"),
        }];
        let mut s = Sections::new();
        s.entry("panel.backup".into())
            .or_default()
            .insert("BackUpPath".into(), "D:\\bak".into());
        let ini = to_ini(&s, &m);
        assert_eq!(ini["Preferences"]["BackUpPath"], "D:\\bak");
        assert!(!ini.contains_key("panel.backup"));
        let dir = std::env::temp_dir().join(format!("zlog-ini-copied-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zlog.ini");
        std::fs::write(
            &path,
            crate::encode("[Preferences]\r\nBackUpPath=C:\\old\r\n"),
        )
        .unwrap();
        export(&path, &ini).unwrap();
        let back = from_ini(crate::read(&std::fs::read(&path).unwrap()), &m);
        assert_eq!(back["panel.backup"]["BackUpPath"], "D:\\bak");
        assert_eq!(to_ini(&Sections::new(), &m), Sections::new());
        let lower = from_ini(crate::read(b"[preferences]\r\nbackuppath=E:\\x\r\n"), &m);
        assert_eq!(lower["panel.backup"]["BackUpPath"], "E:\\x");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn every_text_is_in_every_language() {
        assert!(texts().missing().is_empty(), "{:?}", texts().missing());
    }
}
