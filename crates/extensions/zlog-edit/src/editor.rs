use egui::{Key, KeyboardShortcut, Modifiers};
use zlog_core::qso::QslState;
use zlog_core::{Band, Batch, Command as BookCommand, Mode, Power, Qso, QsoId};
use zlog_ui::dialog::{dialog_with_buttons, modal_with_buttons, ok_cancel};
use zlog_ui::{Command, EntryAction, MenuGroup, Panel, StatusItem, View};

use crate::change;
use crate::texts;

struct Editing {
    id: QsoId,
    qso: Qso,
    inserting: bool,
    time: String,
}

#[derive(Default)]
pub struct Editor {
    language: String,
    selected: Option<Qso>,
    editing: Option<Editing>,
    deleting: Option<QsoId>,
    actions: Vec<EntryAction>,
}

const COMMANDS: [(&str, Option<Key>); 4] = [
    ("qso", Some(Key::Enter)),
    ("delete", Some(Key::Delete)),
    ("insert", Some(Key::Insert)),
    ("invalid", None),
];

impl Editor {
    fn say(&mut self, key: &str, warning: bool, call: &str) {
        let text = texts().format(&self.language, key, &[&call]);
        self.actions
            .push(EntryAction::Say(StatusItem::new(text).warning(warning)));
    }

    fn edit(&mut self, commands: Vec<BookCommand>, done: String) {
        self.actions.push(EntryAction::Edit {
            batch: Batch {
                commands,
                ..Batch::default()
            },
            done: Some(done),
        });
    }

    fn edit_dialog(&mut self, ctx: &egui::Context, view: &View<'_>) {
        let Some(e) = self.editing.as_mut() else {
            return;
        };
        let Some(index) = change::find(view, e.id) else {
            self.editing = None;
            self.say("gone", true, "");
            return;
        };
        let lang = self.language.clone();
        let t = |key: &str| texts().get(&lang, key).to_string();
        let (lo, hi) = view.contest.band_range();
        let warc = view.contest.uses_warc();
        let title = t(if e.inserting {
            "dialog.insert"
        } else {
            "dialog.edit"
        });
        let mut open = true;
        let shown = dialog_with_buttons(
            ctx,
            "edit",
            &title,
            [520.0, 560.0],
            &mut open,
            |ui| {
                let labels = [
                    "field.time",
                    "field.call",
                    "field.rst",
                    "field.sent",
                    "field.rcvd",
                    "field.band",
                    "field.mode",
                    "field.power",
                    "field.tx",
                    "field.freq",
                    "field.qsl",
                    "field.memo",
                ]
                .map(t);
                let width = zlog_ui::form_field_width(ui, &labels.each_ref().map(String::as_str));
                let field = |ui: &mut egui::Ui, text: &mut String| {
                    zlog_ui::sized_field(ui, egui::TextEdit::singleline(text), width);
                };
                let qso = &mut e.qso;
                egui::Grid::new("edit_grid").num_columns(2).show(ui, |ui| {
                    ui.label(t("field.time"));
                    let ok = zlog_ui::parse_time(&e.time).is_some();
                    zlog_ui::sized_field(
                        ui,
                        egui::TextEdit::singleline(&mut e.time)
                            .text_color_opt((!ok).then_some(ui.visuals().error_fg_color)),
                        width,
                    );
                    ui.end_row();
                    ui.label(t("field.call"));
                    field(ui, &mut qso.callsign);
                    ui.end_row();
                    ui.label(t("field.rst"));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut qso.rst_sent));
                        ui.add(egui::DragValue::new(&mut qso.rst_rcvd));
                    });
                    ui.end_row();
                    ui.label(t("field.sent"));
                    field(ui, &mut qso.nr_sent);
                    ui.end_row();
                    ui.label(t("field.rcvd"));
                    field(ui, &mut qso.nr_rcvd);
                    ui.end_row();
                    ui.label(t("field.band"));
                    egui::ComboBox::from_id_salt("edit_band")
                        .selected_text(qso.band.label())
                        .show_ui(ui, |ui| {
                            for b in Band::range(lo, hi).filter(|b| warc || !b.is_warc()) {
                                ui.selectable_value(&mut qso.band, b, b.label());
                            }
                        });
                    ui.end_row();
                    ui.label(t("field.mode"));
                    egui::ComboBox::from_id_salt("edit_mode")
                        .selected_text(qso.mode.label())
                        .show_ui(ui, |ui| {
                            for m in Mode::ALL {
                                ui.selectable_value(&mut qso.mode, *m, m.label());
                            }
                        });
                    ui.end_row();
                    ui.label(t("field.power"));
                    egui::ComboBox::from_id_salt("edit_power")
                        .selected_text(qso.power.label())
                        .show_ui(ui, |ui| {
                            for p in Power::ALL {
                                ui.selectable_value(&mut qso.power, *p, p.label());
                            }
                        });
                    ui.end_row();
                    ui.label(t("field.tx"));
                    ui.add(egui::DragValue::new(&mut qso.tx).range(0..=99));
                    ui.end_row();
                    ui.label(t("field.freq"));
                    field(ui, &mut qso.freq);
                    ui.end_row();
                    ui.label(t("field.qsl"));
                    ui.horizontal(|ui| {
                        for (state, key) in [
                            (QslState::None, "qsl.none"),
                            (QslState::PseQsl, "qsl.pse"),
                            (QslState::NoQsl, "qsl.no"),
                        ] {
                            ui.radio_value(&mut qso.qsl_state, state, t(key));
                        }
                    });
                    ui.end_row();
                    ui.label(t("field.memo"));
                    field(ui, &mut qso.memo);
                    ui.end_row();
                    ui.label("");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut qso.cq, t("flag.cq"));
                        ui.checkbox(&mut qso.dupe, t("flag.dupe"));
                        ui.checkbox(&mut qso.forced, t("flag.forced"));
                        ui.checkbox(&mut qso.invalid, t("flag.invalid"));
                    });
                    ui.end_row();
                });
            },
            |ui| ok_cancel(ui, &lang, true, true),
        );
        let pressed = shown.and_then(|(_, pressed)| pressed);
        if pressed == Some(true) {
            if let Some(time) = zlog_ui::parse_time(&e.time) {
                e.qso.time = time;
            }
            let mut q = e.qso.clone();
            change::finish(view, &mut q);
            let call = q.callsign.clone();
            let (command, done) = if e.inserting {
                let original = &view.log.qsos[index];
                (change::insert_before(original, q), "inserted")
            } else {
                (BookCommand::Update(e.id, q), "edited")
            };
            let done = texts().format(&lang, done, &[&call]);
            self.editing = None;
            self.edit(vec![command], done);
        } else if pressed == Some(false) || !open {
            self.editing = None;
        }
    }

    fn confirm_delete(&mut self, ctx: &egui::Context, view: &View<'_>) {
        let Some(id) = self.deleting else {
            return;
        };
        let Some(index) = change::find(view, id) else {
            self.deleting = None;
            self.say("gone", true, "");
            return;
        };
        let call = view.log.qsos[index].callsign.clone();
        let lang = self.language.clone();
        let (_, response) = modal_with_buttons(
            ctx,
            "confirm_delete",
            |ui| ui.label(texts().format(&lang, "confirm", &[&call])),
            |ui| ok_cancel(ui, &lang, true, true),
        );
        match response {
            Some(true) => {
                self.deleting = None;
                let done = texts().format(&lang, "deleted", &[&call]);
                self.edit(vec![BookCommand::Delete(id)], done);
                if let Some(next) = change::after_delete(&view.log.qsos, index) {
                    self.actions.push(EntryAction::SelectQso(next));
                }
            }
            Some(false) => self.deleting = None,
            None => {}
        }
    }
}

impl Panel for Editor {
    fn id(&self) -> &str {
        "edit"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::None
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Edit
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Qso, 0)
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        let t = texts();
        COMMANDS
            .into_iter()
            .map(|(id, key)| {
                let tip = format!("{id}.tip");
                let tip = t.get(language, &tip);
                let glyph = match id {
                    "qso" => zlog_ui::icon::PENCIL_SIMPLE,
                    "delete" => zlog_ui::icon::TRASH,
                    "insert" => zlog_ui::icon::NOTE_PENCIL,
                    _ => zlog_ui::icon::X_CIRCLE,
                };
                let mut c = Command::new(id, t.get(language, &format!("toolbar.{id}")), tip)
                    .menu_label(t.get(language, id))
                    .icon(glyph)
                    .in_context();
                if matches!(id, "qso" | "delete") {
                    c = c.toolbar_key(format!("editor.{id}"));
                }
                if let Some(k) = key {
                    c = c.row_key(KeyboardShortcut::new(Modifiers::NONE, k));
                }
                if self.selected.is_some() {
                    c
                } else {
                    c.disabled_because(t.get(language, "select_row"))
                }
            })
            .collect()
    }

    fn run_command(&mut self, id: &str) {
        let Some(q) = self.selected.clone() else {
            return;
        };
        let qso_id = QsoId::of(&q);
        match id {
            "qso" | "insert" => {
                let inserting = id == "insert";
                let qso = if inserting {
                    change::insertion_from(&q)
                } else {
                    q
                };
                self.editing = Some(Editing {
                    id: qso_id,
                    time: qso.time.to_string(),
                    qso,
                    inserting,
                });
            }
            "delete" => self.deleting = Some(qso_id),
            "invalid" => {
                self.actions.push(EntryAction::Edit {
                    batch: Batch {
                        commands: vec![change::toggle_invalid(&q)],
                        ..Batch::default()
                    },
                    done: None,
                });
            }
            _ => {}
        }
    }

    fn tick(&mut self, view: &View<'_>) {
        self.language = view.language.to_string();
        self.selected = view.selected.cloned();
    }

    fn dialogs(&mut self, ctx: &egui::Context, view: &View<'_>) {
        self.language = view.language.to_string();
        self.edit_dialog(ctx, view);
        self.confirm_delete(ctx, view);
    }

    fn modal(&self) -> bool {
        self.editing.is_some() || self.deleting.is_some()
    }

    fn dialog_windows(&self, language: &str) -> Vec<(String, String)> {
        self.editing
            .iter()
            .map(|_| ("edit".into(), texts().get(language, "dialog.edit").into()))
            .collect()
    }

    fn close_dialogs(&mut self) {
        self.editing = None;
        self.deleting = None;
    }

    fn editing(&self) -> Option<QsoId> {
        self.editing.as_ref().map(|e| e.id)
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }
}
