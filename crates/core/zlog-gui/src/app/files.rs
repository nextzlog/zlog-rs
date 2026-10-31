use super::*;

pub(super) fn start_time_dialog(ctx: &egui::Context, s: &mut Session, v: &mut View) {
    let Some(dialog) = crate::registry::builder().start_time() else {
        v.start_time = None;
        return;
    };
    if let Some(form) = v.start_time.as_mut() {
        match dialog(ctx, form, i18n::current()) {
            Some(zlog_ui::time::StartTimeDone::Set { start, judge }) => {
                let changed = s.set_start_time(start, judge);
                s.status = Status::info(i18n::trf("status.start_time_set", &[&changed]));
                v.start_time = None;
            }
            Some(zlog_ui::time::StartTimeDone::Cancelled) => {
                s.asks_start_time = false;
                v.start_time = None;
            }
            None => {}
        }
    }
}

pub(super) fn ask_old_other(ctx: &egui::Context) -> Option<bool> {
    let (_, answer) = crate::dialog::modal_with_buttons(
        ctx,
        "old_other",
        |ui| {
            ui.label(tr("open.old_other"));
        },
        |ui| {
            ui.horizontal(|ui| {
                if ui.button(tr("button.yes")).clicked() {
                    Some(true)
                } else if ui.button(tr("button.no")).clicked()
                    || ui.input(|i| {
                        i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)
                    })
                {
                    Some(false)
                } else {
                    None
                }
            })
            .inner
        },
    );
    answer
}

pub(super) struct Ran<'a> {
    pub(super) ctx: &'a egui::Context,
    pub(super) session: &'a mut Session,
    pub(super) view: &'a mut View,
    pub(super) entry: Option<&'a mut (dyn EntryStyle + 'static)>,
    pub(super) close: &'a mut bool,
    pub(super) overwrite: &'a mut Option<Overwrite>,
}

pub struct Overwrite {
    pub path: std::path::PathBuf,
    pub then: Then,
}

pub enum Then {
    SaveAs(Option<After>),
    Export(usize),
}

impl Ran<'_> {
    pub(super) fn core(&mut self, id: &str) {
        let session = &mut *self.session;
        let view = &mut *self.view;
        match id {
            "file.open" => *self.close = true,
            "file.save" => session.save(),
            "file.save_as" | "file.save_as_zlo" => {
                let format = if id == "file.save_as" {
                    zlog_core::Format::Zlox
                } else {
                    zlog_core::Format::Zlo
                };
                let name = session
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned());
                let picked = crate::dialog::choose_log_file(
                    name.as_deref().unwrap_or("contest.zlox"),
                    format,
                );
                match picked {
                    Some(to) if to.ask => {
                        *self.overwrite = Some(Overwrite {
                            path: to.path,
                            then: Then::SaveAs(None),
                        })
                    }
                    Some(to) => session.save_as(to.path),
                    None => {}
                }
            }
            "file.start_time" => {
                view.start_time = Some(zlog_ui::time::StartTimeForm::new(
                    session.start_time(),
                    false,
                ))
            }
            "file.exit" => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            "edit.next_band" => session.cycle_band(false),
            "edit.next_mode" => session.cycle_mode(false),
            "edit.next_power" => session.entry.cycle_power(false),
            "edit.cq_sp" => session.entry.cq = !session.entry.cq,
            "edit.clear" => {
                session.entry.clear_call_and_number();
                session.status = Default::default();
                if let Some(e) = self.entry.as_deref_mut() {
                    e.entry_changed();
                    e.focus(EntryField::Call);
                }
            }
            _ => {
                if let Some((dir, i)) = format_command(id) {
                    *self.overwrite = file_format(session, dir, i);
                }
            }
        }
    }
}

pub(super) fn format_command(id: &str) -> Option<(bool, usize)> {
    let (dir, i) = id.strip_prefix("file.")?.split_once('.')?;
    let i = i.parse().ok()?;
    match dir {
        "import" => Some((true, i)),
        "export" => Some((false, i)),
        _ => None,
    }
}

pub(super) fn file_format(session: &mut Session, import: bool, i: usize) -> Option<Overwrite> {
    let f = session.formats().get(i)?;
    let name = f.name().to_string();
    let exts: Vec<String> = f.extensions().iter().map(|e| e.to_string()).collect();
    let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
    if import {
        if let Some(p) = crate::dialog::import_file(&name, &exts) {
            session.import(i, &p);
        }
    } else if let Some(to) =
        crate::dialog::export_file_named(&name, &exts, session.path.to_str().unwrap_or(""))
    {
        if to.ask {
            return Some(Overwrite {
                path: to.path,
                then: Then::Export(i),
            });
        }
        session.export(i, &to.path);
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    CloseLog,
    Quit,
}

pub struct SaveFailed {
    pub error: String,
    pub after: After,
}

pub(super) const QUIT_FRAMES: u8 = 3;

impl ZLogApp {
    pub(super) fn saved_before(&mut self, after: After) -> bool {
        if self.leave_unsaved || self.save_failed.is_some() {
            return self.leave_unsaved;
        }
        let Some(s) = self.session.as_mut().filter(|s| s.unsaved()) else {
            return true;
        };
        match s.try_save() {
            Ok(()) => true,
            Err(error) => {
                self.save_failed = Some(SaveFailed { error, after });
                false
            }
        }
    }

    pub(super) fn go_on(&mut self, ctx: &egui::Context, after: After) {
        match after {
            After::CloseLog => self.close_log(),
            After::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    pub fn save_failed_dialog(&mut self, ctx: &egui::Context) {
        let Some(failed) = &self.save_failed else {
            return;
        };
        if self.overwrite.is_some() {
            return;
        }
        let (after, error) = (failed.after, failed.error.clone());
        let discard = match after {
            After::CloseLog => tr("unsaved.close"),
            After::Quit => tr("unsaved.quit"),
        };
        let (_, pressed) = crate::dialog::modal_with_buttons(
            ctx,
            "save_failed",
            |ui| {
                ui.label(crate::i18n::trf("unsaved.failed", &[&error]));
            },
            |ui| {
                ui.horizontal(|ui| {
                    let mut pressed = None;
                    if ui.button(tr("unsaved.retry")).clicked() {
                        pressed = Some(0);
                    }
                    if ui
                        .button(zlog_ui::command::with_ellipsis(
                            tr("menu.file.save_as"),
                            true,
                        ))
                        .clicked()
                    {
                        pressed = Some(1);
                    }
                    if ui.button(discard).clicked() {
                        pressed = Some(2);
                    }
                    if ui.button(tr("button.cancel")).clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        pressed = Some(3);
                    }
                    pressed
                })
                .inner
            },
        );
        let Some(pressed) = pressed else {
            return;
        };
        let saved = match (pressed, self.session.as_mut()) {
            (0, Some(s)) => Some(s.try_save()),
            (1, Some(s)) => {
                let name = s
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match crate::dialog::choose_log_file(&name, zlog_core::Format::Zlox) {
                    Some(to) if to.ask => {
                        self.overwrite = Some(Overwrite {
                            path: to.path,
                            then: Then::SaveAs(Some(after)),
                        });
                        None
                    }
                    to => to.map(|to| s.try_save_as(to.path)),
                }
            }
            _ => None,
        };
        match (pressed, saved) {
            (0 | 1, Some(saved)) => self.after_save(ctx, after, saved),
            (2, _) => {
                self.save_failed = None;
                self.leave_unsaved = true;
                self.go_on(ctx, after);
            }
            (3, _) => {
                self.save_failed = None;
                if after == After::Quit {
                    self.quit_cancelled();
                }
            }
            _ => {}
        }
    }

    pub(super) fn after_save(
        &mut self,
        ctx: &egui::Context,
        after: After,
        saved: Result<(), String>,
    ) {
        match saved {
            Ok(()) => {
                self.save_failed = None;
                self.go_on(ctx, after);
            }
            Err(e) => {
                if let Some(f) = &mut self.save_failed {
                    f.error = e;
                }
            }
        }
    }

    pub fn overwrite_dialog(&mut self, ctx: &egui::Context) {
        let Some(o) = &self.overwrite else {
            return;
        };
        let question = crate::i18n::trf("file.overwrite", &[&o.path.display()]);
        let Some(yes) = crate::dialog::ask_overwrite(ctx, &question, crate::i18n::current()) else {
            return;
        };
        let Some(Overwrite { path, then }) = self.overwrite.take() else {
            return;
        };
        if !yes {
            return;
        }
        match then {
            Then::SaveAs(after) => {
                let Some(s) = self.session.as_mut() else {
                    return;
                };
                match after {
                    Some(after) => {
                        let saved = s.try_save_as(path);
                        self.after_save(ctx, after, saved);
                    }
                    None => s.save_as(path),
                }
            }
            Then::Export(i) => {
                if let Some(s) = self.session.as_mut() {
                    s.export(i, &path);
                }
            }
        }
    }
}
