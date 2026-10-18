//! Top level: the zLog Menu until a contest is open, then the main window.

use crate::dialog::dialog;
use crate::i18n::{self, tr};
use crate::main_window::{self, Field, View};
use crate::menu_form::MenuForm;
use crate::options::{rig_window, Options};
use crate::registry::{panels, registry, settings_pages};
use crate::rig::{RigConfig, RigControl};
use crate::session::Session;
use crate::settings::Settings;
use crate::windows;

/// A window added by an extension, and whether it is open.
struct PanelSlot {
    panel: Box<dyn zlog_ui::Panel>,
    open: bool,
}

pub struct ZLogApp {
    session: Option<Session>,
    panels: Vec<PanelSlot>,
    menu: MenuForm,
    view: View,
    show_about: bool,
    settings: Settings,
    options: Options,
    rig: RigControl,
    show_rig: bool,
}

impl ZLogApp {
    pub fn new() -> Self {
        let mut settings = Settings::load();
        let station = settings.station();
        let recent = settings.recent_contests();
        ZLogApp {
            session: None,
            panels: panels()
                .into_iter()
                .map(|panel| PanelSlot { panel, open: false })
                .collect(),
            menu: MenuForm::with_station(station, recent),
            view: View::default(),
            show_about: false,
            options: Options::new(settings_pages()),
            settings,
            rig: RigControl::default(),
            show_rig: false,
        }
    }

    fn close(&mut self) {
        if let Some(mut s) = self.session.take() {
            s.close();
        }
        self.view = View::default();
    }
}

fn language_menu(ui: &mut egui::Ui) {
    ui.menu_button(tr("menu.language"), |ui| {
        for l in i18n::LANGUAGES {
            if ui.radio(i18n::current() == l.code, l.name).clicked() {
                i18n::set_language(l.code);
            }
        }
    });
}

impl eframe::App for ZLogApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let Some(session) = self.session.as_mut() else {
            let linked = registry();
            let contests = linked.contests();
            egui::Panel::top("menu").show(ui, |ui| egui::MenuBar::new().ui(ui, language_menu));
            egui::CentralPanel::default().show(ui, |ui| {
                if let Some(setup) = self.menu.show(ui, contests) {
                    self.settings.set_station(&setup.station);
                    if let Some(c) = contests.get(setup.contest) {
                        self.settings.add_recent_contest(c.id());
                        self.menu.recent = self.settings.recent_contests();
                    }
                    let _ = self.settings.save();
                    match Session::open(setup, registry()) {
                        Ok(s) => {
                            self.session = Some(s);
                            self.view.focus = Some(Field::Call);
                        }
                        Err(e) => self.menu.error = Some(e),
                    }
                }
            });
            return;
        };

        session.poll();
        main_window::handle_keys(&ctx, session, &mut self.view);
        self.rig.tick(
            std::time::Instant::now(),
            &mut session.entry,
            &session.station,
        );

        let mut close = false;
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(tr("menu.file"), |ui| {
                    if ui.button(tr("menu.file.open")).clicked() {
                        close = true;
                    }
                    if ui.button(tr("menu.file.save_as")).clicked() {
                        let picked = rfd::FileDialog::new()
                            .add_filter("zLog", &["zlox", "zlo"])
                            .save_file();
                        if let Some(p) = picked {
                            session.save_as(p);
                        }
                    }
                    file_formats(ui, session);
                    ui.separator();
                    if ui.button(tr("menu.file.exit")).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button(tr("menu.windows"), |ui| {
                    ui.checkbox(&mut self.show_rig, tr("window.rig"));
                    for slot in &mut self.panels {
                        ui.checkbox(&mut slot.open, slot.panel.title(i18n::current()));
                    }
                });
                ui.menu_button(tr("menu.settings"), |ui| {
                    if ui.button(tr("menu.settings.options")).clicked() {
                        self.options.show_with(&mut self.settings);
                    }
                });
                language_menu(ui);
                ui.menu_button(tr("menu.help"), |ui| {
                    if ui.button(tr("menu.help.about")).clicked() {
                        self.show_about = true;
                    }
                });
            });
            ui.horizontal(|ui| {
                if ui.button(tr("toolbar.save")).clicked() {
                    session.save();
                }
                ui.separator();
                for slot in self.panels.iter_mut().filter(|s| s.panel.on_toolbar()) {
                    ui.toggle_value(&mut slot.open, slot.panel.title(i18n::current()));
                }
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| main_window::status_bar(ui, session));
        egui::Panel::bottom("entry")
            .show(ui, |ui| main_window::entry_row(ui, session, &mut self.view));
        egui::CentralPanel::default()
            .show(ui, |ui| main_window::log_grid(ui, session, &mut self.view));

        windows::edit_dialog(&ctx, session, &mut self.view);
        windows::confirm_delete(&ctx, session, &mut self.view);
        if self.options.ui(&ctx, &mut self.settings) {
            session.station = self.options.station().clone();
            if self.rig.connected() {
                self.rig.connect(self.options.rig().clone());
            }
        }
        let rig_config = RigConfig::from_section(self.settings.section("rig"));
        rig_window(&ctx, &mut self.rig, &mut self.show_rig, &rig_config);
        let view = zlog_ui::View {
            log: session.log(),
            contest: session.rules(),
            sheet: session.sheet(),
            callsign: &session.entry.callsign,
            band: session.entry.band,
            mode: session.entry.mode,
            language: i18n::current(),
        };
        for slot in &mut self.panels {
            let (id, title, size) = (
                slot.panel.id().to_string(),
                slot.panel.title(view.language),
                slot.panel.default_size(),
            );
            dialog(&ctx, &id, &title, size, &mut slot.open, |ui| {
                slot.panel.ui(ui, &view)
            });
        }
        // Requests from panels to the entry row (a decoded callsign clicked).
        let actions: Vec<zlog_ui::EntryAction> = self
            .panels
            .iter_mut()
            .flat_map(|s| s.panel.take_actions())
            .collect();
        for action in actions {
            match action {
                zlog_ui::EntryAction::Callsign(c) => {
                    session.entry.callsign = c;
                    session.check_call();
                    self.view.focus = Some(Field::Call);
                }
                zlog_ui::EntryAction::Number(n) => {
                    session.entry.nr_rcvd = n;
                    session.check_number();
                    self.view.focus = Some(Field::Number);
                }
            }
        }
        dialog(
            &ctx,
            "about",
            tr("menu.help.about"),
            [300.0, 200.0],
            &mut self.show_about,
            |ui| {
                ui.heading("zLog");
                ui.label(env!("CARGO_PKG_VERSION"));
                ui.separator();
                ui.label(tr("about.extensions"));
                for name in registry().names() {
                    ui.label(format!("  {name}"));
                }
            },
        );

        let title = format!(
            "zLog - {} - {}",
            session.rules().name(),
            session.path.display()
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        // The clock in the entry row, and extensions waiting on the outside.
        ctx.request_repaint_after(std::time::Duration::from_millis(250));

        if close {
            self.close();
        }
    }

    fn on_exit(&mut self) {
        self.close();
    }
}

/// Import and Export submenus, one item per format an extension provides.
fn file_formats(ui: &mut egui::Ui, session: &mut Session) {
    let items: Vec<(usize, String, Vec<String>, bool, bool)> = session
        .formats()
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let exts = f.extensions().iter().map(|e| e.to_string()).collect();
            (
                i,
                f.name().to_string(),
                exts,
                f.can_import(),
                f.can_export(),
            )
        })
        .collect();
    ui.add_enabled_ui(!items.is_empty(), |ui| {
        ui.menu_button(tr("menu.file.import"), |ui| {
            for (i, name, exts, can_import, _) in &items {
                if *can_import && ui.button(name).clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter(name, exts).pick_file() {
                        session.import(*i, &p);
                    }
                }
            }
        });
        ui.menu_button(tr("menu.file.export"), |ui| {
            for (i, name, exts, _, can_export) in &items {
                if *can_export && ui.button(name).clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter(name, exts).save_file() {
                        session.export(*i, &p);
                    }
                }
            }
        });
    });
}
