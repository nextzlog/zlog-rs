use zlog_ui::{Section, SettingsPage};

use crate::config::{Config, Source};
use crate::texts;

#[derive(Default)]
pub struct HistOptions;

impl SettingsPage for HistOptions {
    fn id(&self) -> &str {
        "panel.hist"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "options.title").into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, section: &mut Section, lang: &str) {
        let t = texts();
        let mut c = Config::load(section);
        let before = c.clone();
        ui.checkbox(&mut c.enabled, t.get(lang, "options.enabled"));
        ui.checkbox(&mut c.name_memo, t.get(lang, "options.name_memo"));
        ui.separator();
        ui.label(egui::RichText::new(t.get(lang, "options.files")).strong());
        let mut remove = None;
        let remove_width = {
            let label = egui::WidgetText::from(t.get(lang, "options.remove"));
            let galley = label.into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Button,
            );
            galley.size().x + 2.0 * ui.spacing().button_padding.x
        };
        let column = ((ui.available_width() - remove_width) / 2.0
            - 2.0 * ui.spacing().item_spacing.x)
            .max(ui.spacing().interact_size.x);
        egui::Grid::new("hist_files")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                ui.label(t.get(lang, "options.file"));
                ui.label(t.get(lang, "options.contest"));
                ui.label("");
                ui.end_row();
                for (i, s) in c.sources.iter_mut().enumerate() {
                    ui.add_sized(
                        [column, ui.spacing().interact_size.y],
                        egui::Label::new(&s.path).truncate(),
                    )
                    .on_hover_text(&s.path);
                    ui.add(
                        egui::TextEdit::singleline(&mut s.contest)
                            .hint_text(t.get(lang, "options.every"))
                            .desired_width(column),
                    )
                    .on_hover_text(t.get(lang, "options.contest.tip"));
                    if ui.button(t.get(lang, "options.remove")).clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
        if c.sources.is_empty() {
            ui.label(t.get(lang, "options.no_files"));
        }
        if let Some(i) = remove {
            c.sources.remove(i);
        }
        if ui
            .button(t.get(lang, "options.add"))
            .on_hover_text(t.get(lang, "options.add.tip"))
            .clicked()
        {
            for p in zlog_ui::dialog::import_files() {
                c.sources.push(Source {
                    path: p.display().to_string(),
                    contest: String::new(),
                });
            }
        }
        ui.add_space(6.0);
        ui.add(egui::Label::new(t.get(lang, "options.note")).wrap());
        if c != before {
            c.save(section);
        }
    }
}
