//! Settings > Import zlog.ini and Export zlog.ini, through zlog-ini: every
//! section and key of the settings is a section and key of the INI file, with
//! no knowledge of what they mean (keeping zLog's names is each extension's
//! business).

use std::path::PathBuf;

use crate::i18n::{tr, trf};
use crate::settings::Settings;

/// The questions and answers of the two menu items.
#[derive(Default)]
pub struct IniPort {
    /// A file read for import, waiting for the operator's OK.
    asking: Option<(PathBuf, zlog_ini::Sections)>,
    /// What happened, until the operator closes it.
    told: Option<String>,
}

/// Whether the settings were changed by an import (the panels then read them
/// again).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Done {
    Nothing,
    Imported,
}

impl IniPort {
    /// The two items of the Settings menu; returns the file to export to, which
    /// the caller passes to [`IniPort::export`] once the panels saved theirs.
    pub fn menu(&mut self, ui: &mut egui::Ui) -> Option<crate::dialog::SaveTo> {
        let mut export_to = None;
        if ui.button(tr("menu.settings.import_ini")).clicked() {
            ui.close();
            if let Some(path) = crate::dialog::import_file("zlog.ini", &["ini"]) {
                match std::fs::read(&path) {
                    Ok(bytes) => self.asking = Some((path, zlog_ini::read(&bytes))),
                    Err(e) => self.told = Some(trf("ini.read_failed", &[&e])),
                }
            }
        }
        if ui.button(tr("menu.settings.export_ini")).clicked() {
            ui.close();
            export_to = crate::dialog::export_file("zlog.ini", &["ini"]);
        }
        export_to
    }

    /// Export the settings to `path` and say how it went.
    pub fn export(&mut self, path: &std::path::Path, settings: &Settings) {
        self.told = Some(match export(path, settings) {
            Ok((left_out, backup)) => {
                let mut t = trf("ini.exported", &[&path.display()]);
                if let Some(b) = backup {
                    t.push('\n');
                    t.push_str(&trf("ini.backup", &[&b.display()]));
                }
                if left_out > 0 {
                    t.push('\n');
                    t.push_str(&trf("ini.left_out", &[&left_out]));
                }
                t
            }
            Err(e) => trf("ini.write_failed", &[&e]),
        });
    }

    /// The question before an import and the result of either item.
    pub fn dialogs(&mut self, ctx: &egui::Context, settings: &mut Settings) -> Done {
        let mut done = Done::Nothing;
        if let Some((path, sections)) = &self.asking {
            let keys: usize = sections.values().map(|s| s.len()).sum();
            let (_, answer) = crate::dialog::modal_with_buttons(
                ctx,
                "import_ini",
                |ui| {
                    ui.label(trf("ini.ask", &[&path.display(), &sections.len(), &keys]));
                },
                |ui| crate::dialog::ok_cancel(ui, crate::i18n::current(), true, true),
            );
            match answer {
                Some(true) => {
                    for (name, section) in sections {
                        let s = settings.section(name);
                        for (k, v) in section {
                            s.insert(k.clone(), v.clone());
                        }
                    }
                    self.told = Some(match settings.save() {
                        Ok(()) => trf("ini.imported", &[&keys]),
                        Err(e) => trf("ini.write_failed", &[&e]),
                    });
                    self.asking = None;
                    done = Done::Imported;
                }
                Some(false) => self.asking = None,
                None => {}
            }
        }
        if let Some(text) = &self.told {
            let (_, close) = crate::dialog::modal_with_buttons(
                ctx,
                "ini_result",
                |ui| {
                    ui.label(text);
                },
                |ui| {
                    ui.button(tr("button.ok")).clicked()
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

/// Write the settings into the INI file at `path`, merged into the file that is
/// there, which is kept as `path.bak` first. Returns how many values were left
/// out and the backup.
fn export(
    path: &std::path::Path,
    settings: &Settings,
) -> std::io::Result<(usize, Option<PathBuf>)> {
    let old = std::fs::read(path).ok();
    let (bytes, left_out) =
        zlog_ini::merge(old.as_deref().unwrap_or_default(), settings.sections());
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
        let dir = std::env::temp_dir().join(format!("zlog-gui-ini-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zlog.ini");
        std::fs::write(&path, zlog_ini::encode("[Categories]\r\nCall=JA1ZLO\r\n")).unwrap();
        let mut s = Settings::default();
        s.section("Categories")
            .insert("Call".into(), "JA1YAA".into());
        s.section("keyer").insert("pad".into(), "a\nb".into());
        let (left_out, backup) = export(&path, &s).unwrap();
        assert_eq!(left_out, 1);
        assert_eq!(backup, Some(dir.join("zlog.ini.bak")));
        let back = zlog_ini::read(&std::fs::read(&path).unwrap());
        assert_eq!(back["Categories"]["Call"], "JA1YAA");
        let old = zlog_ini::read(&std::fs::read(dir.join("zlog.ini.bak")).unwrap());
        assert_eq!(old["Categories"]["Call"], "JA1ZLO");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
