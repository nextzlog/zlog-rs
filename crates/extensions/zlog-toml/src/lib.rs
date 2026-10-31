use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::{Context, RichText, Ui};
use zlog_ui::{Command, Section, SectionInfo, SectionKind, Sections, SettingsEnv, SettingsFile};

zlog_core::texts!();

pub const FILE: &str = "zlog.toml";

pub fn to_toml(sections: &Sections, chosen: &BTreeSet<String>) -> String {
    let picked: Sections = sections
        .iter()
        .filter(|(name, s)| chosen.contains(*name) && !s.is_empty())
        .map(|(n, s)| (n.clone(), s.clone()))
        .collect();
    toml::to_string(&picked).unwrap_or_default()
}

pub fn from_toml(text: &str) -> Result<Sections, String> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    let mut out = Sections::new();
    for (name, value) in table {
        let toml::Value::Table(t) = value else {
            continue;
        };
        let section: Section = t
            .into_iter()
            .filter_map(|(k, v)| {
                let v = match v {
                    toml::Value::String(s) => s,
                    toml::Value::Integer(i) => i.to_string(),
                    toml::Value::Float(f) => f.to_string(),
                    toml::Value::Boolean(b) => b.to_string(),
                    _ => return None,
                };
                Some((k, v))
            })
            .collect();
        out.insert(name, section);
    }
    Ok(out)
}

pub fn merge(current: &mut Sections, imported: &Sections, chosen: &BTreeSet<String>) -> usize {
    let mut n = 0;
    for (name, s) in imported.iter().filter(|(n, _)| chosen.contains(*n)) {
        current.insert(name.clone(), s.clone());
        n += 1;
    }
    n
}

pub fn info_of(name: &str, infos: &[SectionInfo]) -> (SectionInfo, bool) {
    match infos.iter().find(|i| i.name == name) {
        Some(i) => (i.clone(), true),
        None => (
            SectionInfo {
                name: name.to_string(),
                title: name.to_string(),
                kind: SectionKind::Personal,
            },
            false,
        ),
    }
}

pub fn listing<'a>(
    names: impl IntoIterator<Item = &'a String>,
    infos: &[SectionInfo],
) -> (Vec<(SectionInfo, bool)>, BTreeSet<String>) {
    let mut rows: Vec<(SectionInfo, bool)> = names.into_iter().map(|n| info_of(n, infos)).collect();
    rows.sort_by(|a, b| (a.0.kind, &a.0.title).cmp(&(b.0.kind, &b.0.title)));
    let ticked = rows
        .iter()
        .filter(|(i, known)| *known && i.kind == SectionKind::Custom)
        .map(|(i, _)| i.name.clone())
        .collect();
    (rows, ticked)
}

struct Pick {
    rows: Vec<(SectionInfo, bool)>,
    ticked: BTreeSet<String>,
    sizes: Vec<usize>,
}

impl Pick {
    fn new(sections: &Sections, infos: &[SectionInfo]) -> Self {
        let names: Vec<&String> = sections
            .iter()
            .filter(|(_, s)| !s.is_empty())
            .map(|(n, _)| n)
            .collect();
        let (rows, ticked) = listing(names, infos);
        let sizes = rows.iter().map(|(i, _)| sections[&i.name].len()).collect();
        Pick {
            rows,
            ticked,
            sizes,
        }
    }

    fn ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = texts();
        for kind in [
            SectionKind::Custom,
            SectionKind::Personal,
            SectionKind::Secret,
        ] {
            let of: Vec<usize> = (0..self.rows.len())
                .filter(|i| self.rows[*i].0.kind == kind)
                .collect();
            if of.is_empty() {
                continue;
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t.get(lang, kind_key(kind))).strong());
                if ui.small_button(t.get(lang, "all")).clicked() {
                    for i in &of {
                        self.ticked.insert(self.rows[*i].0.name.clone());
                    }
                }
                if ui.small_button(t.get(lang, "none")).clicked() {
                    for i in &of {
                        self.ticked.remove(&self.rows[*i].0.name);
                    }
                }
            });
            for i in of {
                let (info, known) = &self.rows[i];
                ui.horizontal(|ui| {
                    let mut on = self.ticked.contains(&info.name);
                    if ui.checkbox(&mut on, &info.title).changed() {
                        if on {
                            self.ticked.insert(info.name.clone());
                        } else {
                            self.ticked.remove(&info.name);
                        }
                    }
                    let size = t.format(lang, "values", &[&self.sizes[i]]);
                    ui.label(
                        RichText::new(format!("[{}] {size}", info.name))
                            .weak()
                            .small(),
                    );
                    if info.kind == SectionKind::Secret {
                        ui.colored_label(zlog_ui::palette::warning(ui), t.get(lang, "secret"))
                            .on_hover_text(t.get(lang, "secret.tip"));
                    }
                    if !known {
                        ui.label(RichText::new(t.get(lang, "unknown")).weak())
                            .on_hover_text(t.get(lang, "unknown.tip"));
                    }
                });
            }
        }
    }
}

fn kind_key(kind: SectionKind) -> &'static str {
    match kind {
        SectionKind::Custom => "kind.custom",
        SectionKind::Personal => "kind.personal",
        SectionKind::Secret => "kind.secret",
    }
}

#[derive(Default)]
pub struct Share {
    export: Option<Pick>,
    import: Option<(PathBuf, Sections, Pick)>,
    told: Option<String>,
}

const IMPORT: &str = "toml.import";
const EXPORT: &str = "toml.export";

impl SettingsFile for Share {
    fn id(&self) -> &str {
        "toml"
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
                if let Some(path) = zlog_ui::dialog::import_file(FILE, &["toml"]) {
                    self.read(&path, env.infos, env.language);
                }
            }
            EXPORT => self.export = Some(Pick::new(env.sections, env.infos)),
            _ => {}
        }
    }

    fn dialogs(&mut self, ctx: &Context, env: &mut SettingsEnv<'_>) -> bool {
        self.show(ctx, env.sections, env.language)
    }
}

impl Share {
    pub fn read(&mut self, path: &Path, infos: &[SectionInfo], lang: &str) {
        let t = texts();
        let read = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| from_toml(&text));
        match read {
            Ok(s) if s.values().all(|s| s.is_empty()) => {
                self.told = Some(t.get(lang, "empty").into());
            }
            Ok(s) => {
                let pick = Pick::new(&s, infos);
                self.import = Some((path.to_path_buf(), s, pick));
            }
            Err(e) => {
                self.told = Some(t.format(lang, "read_failed", &[&path.display(), &e]));
            }
        }
    }

    pub fn show(&mut self, ctx: &Context, sections: &mut Sections, lang: &str) -> bool {
        let t = texts();
        let mut done = false;
        if let Some(pick) = &mut self.export {
            let mut open = true;
            let mut answer = None;
            let any = !pick.ticked.is_empty();
            zlog_ui::dialog::dialog_with_buttons(
                ctx,
                "toml_export",
                t.get(lang, "export.title"),
                [460.0, 480.0],
                &mut open,
                |ui| {
                    ui.add(egui::Label::new(t.get(lang, "export.note")).wrap());
                    pick.ui(ui, lang);
                },
                |ui| {
                    zlog_ui::form::buttons(ui, |ui| {
                        if ui
                            .add_enabled(any, egui::Button::new(t.get(lang, "export")))
                            .on_disabled_hover_text(t.get(lang, "nothing"))
                            .clicked()
                        {
                            answer = Some(true);
                        }
                        if ui.button(t.get(lang, "cancel")).clicked() {
                            answer = Some(false);
                        }
                    });
                },
            );
            if answer == Some(true) {
                if let Some(to) = zlog_ui::dialog::export_file(FILE, &["toml"]) {
                    let text = to_toml(sections, &pick.ticked);
                    let n = pick.ticked.len();
                    self.told = Some(match write_toml(&to.path, &text) {
                        Ok(()) => t.format(lang, "exported", &[&n, &to.path.display()]),
                        Err(e) => t.format(lang, "write_failed", &[&to.path.display(), &e]),
                    });
                    self.export = None;
                }
            } else if answer == Some(false) || !open {
                self.export = None;
            }
        }
        if let Some((path, file, pick)) = &mut self.import {
            let mut open = true;
            let mut answer = None;
            let any = !pick.ticked.is_empty();
            zlog_ui::dialog::dialog_with_buttons(
                ctx,
                "toml_import",
                t.get(lang, "import.title"),
                [460.0, 480.0],
                &mut open,
                |ui| {
                    ui.add(egui::Label::new(t.get(lang, "import.note")).wrap());
                    pick.ui(ui, lang);
                },
                |ui| {
                    zlog_ui::form::buttons(ui, |ui| {
                        if ui
                            .add_enabled(any, egui::Button::new(t.get(lang, "import")))
                            .on_disabled_hover_text(t.get(lang, "nothing"))
                            .clicked()
                        {
                            answer = Some(true);
                        }
                        if ui.button(t.get(lang, "cancel")).clicked() {
                            answer = Some(false);
                        }
                    });
                },
            );
            if answer == Some(true) {
                let n = merge(sections, file, &pick.ticked);
                self.told = Some(t.format(lang, "imported", &[&n, &path.display()]));
                done = true;
                self.import = None;
            } else if answer == Some(false) || !open {
                self.import = None;
            }
        }
        if let Some(text) = &self.told {
            let (_, close) = zlog_ui::dialog::modal_with_buttons(
                ctx,
                "toml_result",
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

pub fn register(b: &mut zlog_ui::Builder) {
    b.settings_file(20, Share::default);
}

fn write_toml(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    zlog_core::file::write_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(pairs: &[(&str, &str)]) -> Section {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn info(name: &str, kind: SectionKind) -> SectionInfo {
        SectionInfo {
            name: name.into(),
            title: name.into(),
            kind,
        }
    }

    fn current() -> Sections {
        let mut s = Sections::new();
        s.insert("window".into(), section(&[("toolbar_order", "a,b")]));
        s.insert("bind".into(), section(&[("name:file/file.save", "Write")]));
        s.insert("station".into(), section(&[("callsign", "JA1ZLO")]));
        s.insert("panel.dc".into(), section(&[("url", "https://hook")]));
        s.insert("old".into(), section(&[("x", "1")]));
        s.insert("empty".into(), Section::new());
        s
    }

    fn infos() -> Vec<SectionInfo> {
        vec![
            info("window", SectionKind::Custom),
            info("bind", SectionKind::Custom),
            info("station", SectionKind::Personal),
            info("panel.dc", SectionKind::Secret),
        ]
    }

    #[test]
    fn the_export_is_written_whole_as_utf8() {
        let dir = std::env::temp_dir().join(format!("zlog-toml-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zlog.toml");
        std::fs::write(&path, "old text that is longer than the new one").unwrap();
        write_toml(&path, "[a]\nk = \"東京\"\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[a]\nk = \"東京\"\n"
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        assert!(write_toml(&dir, "x").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn customization_ticked_secrets_never() {
        let pick = Pick::new(&current(), &infos());
        let want: BTreeSet<String> = ["bind", "window"].map(String::from).into();
        assert_eq!(pick.ticked, want);
        let names: Vec<&str> = pick.rows.iter().map(|(i, _)| i.name.as_str()).collect();
        assert_eq!(names, ["bind", "window", "old", "station", "panel.dc"]);
        assert!(!pick.rows[2].1, "old is unknown");
        assert_eq!(pick.rows[2].0.kind, SectionKind::Personal);
    }

    #[test]
    fn export_only_ticked_and_round_trip() {
        let pick = Pick::new(&current(), &infos());
        let text = to_toml(&current(), &pick.ticked);
        assert!(!text.contains("JA1ZLO") && !text.contains("hook"));
        let back = from_toml(&text).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back["bind"], current()["bind"]);
        assert_eq!(back["window"], current()["window"]);
    }

    #[test]
    fn import_merges_only_ticked() {
        let mut file = Sections::new();
        file.insert("window".into(), section(&[("toolbar_order", "z")]));
        file.insert("station".into(), section(&[("callsign", "JA1YAA")]));
        file.insert("future".into(), section(&[("y", "2")]));
        let pick = Pick::new(&file, &infos());
        let mut now = current();
        assert_eq!(merge(&mut now, &file, &pick.ticked), 1);
        assert_eq!(now["window"]["toolbar_order"], "z");
        assert_eq!(now["station"]["callsign"], "JA1ZLO");
        assert!(!now.contains_key("future"));
        let all: BTreeSet<String> = file.keys().cloned().collect();
        merge(&mut now, &file, &all);
        assert_eq!(now["future"]["y"], "2", "kept as it is");
        assert_eq!(now["old"]["x"], "1", "sections not in the file stay");
    }

    #[test]
    fn hand_written_values_and_bad_files() {
        let s = from_toml("top = 1\n[a]\nn = 3\nb = true\nlist = [1]\n").unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s["a"], section(&[("n", "3"), ("b", "true")]));
        assert!(from_toml("[a").is_err());
    }
}
