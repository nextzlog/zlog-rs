use super::*;

impl ZLogApp {
    fn set_title(&mut self, ctx: &egui::Context, title: String) {
        if title != self.window_title {
            self.window_title = title.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
    }

    pub(super) fn start_frame(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        self.set_title(ctx, zlog_ui::product::NAME.to_string());
        if let Some(bar) = self.menu_bar.as_mut() {
            let menus = chrome::start_menus();
            let pick = egui::Panel::top("menu")
                .show(ui, |ui| bar.show(ui, &menus))
                .inner;
            if let Some(code) = pick.and_then(|p| p.id.strip_prefix("language:").map(String::from))
            {
                choose_language(ctx, &mut self.settings, &code);
            }
        }
        let Some(menu) = self.menu.as_mut() else {
            egui::CentralPanel::default().show(ui, |ui| ui.label(tr("start_screen.none")));
            return;
        };
        {
            let linked = registry();
            let mut view = zlog_ui::StartView {
                registry: &linked,
                language: i18n::current(),
                settings: &mut self.settings.sections_named(menu.sections()),
                save: false,
                back: self.reopen.is_some(),
                cancel: false,
            };
            let shown = menu.show(ui, &mut view);
            let (save, cancel) = (view.save, view.cancel);
            let sections = std::mem::take(view.settings);
            self.settings.set_sections(sections);
            if save {
                let _ = self.settings.save();
            }
            if let Some(setup) = self.reopen.clone().filter(|_| cancel) {
                self.open(setup);
            } else if let Some(mut setup) = shown.filter(|_| self.asking_old_other.is_none()) {
                setup.use_contest_period = self.settings.use_contest_period();
                if crate::session::asks_old_other(&setup.path) {
                    self.asking_old_other = Some(setup);
                } else {
                    self.open(setup);
                }
            }
        }
    }

    pub(super) fn log_frame(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        session: &mut Session,
    ) -> bool {
        let ctx = ctx.clone();
        let held = crate::main_window::held_row(self.view.selected, &session.log().qsos);
        session.poll();
        self.view.selected =
            crate::main_window::follow_row(self.view.selected, held, &session.log().qsos);
        if self.overwrite.is_none() && !self.files.iter().any(|f| f.modal()) {
            let rig = rig_now(&self.panels, self.rig_number);
            route_keys(
                &ctx,
                session,
                &mut self.view,
                self.entry.as_deref_mut(),
                &mut self.panels,
                &self.options,
                &self.spots,
                rig,
            );
        }
        if let Some(role) = self.view.run_role.take() {
            self.run_role = Some(role);
        }
        if let Some(hz) = self.view.qsy.take() {
            self.connection_actions
                .push(zlog_ui::EntryAction::Qsy(hz, None));
        }
        let n = session.log().qsos.len();
        if self.view.selected.is_some_and(|i| i >= n) {
            self.view.selected = None;
        }
        if std::mem::take(&mut self.view.toggle_rig) {
            if let Some(rig) = rig_of_mut(&mut self.panels) {
                if rig.toggle() {
                    session.status = rig_selected(rig.number());
                }
            }
        }
        self.view.has_rig = rig_of(&self.panels).is_some();
        if let Some(rig) = rig_of_mut(&mut self.panels) {
            let mut entry = crate::entry::Follow {
                entry: &mut session.entry,
                station: &session.station,
            };
            let now = self.started + std::time::Duration::from_secs_f64(ctx.input(|i| i.time));
            rig.follow(now, &mut entry);
        }

        let mut close = false;
        let mut close_all = false;
        let dialogs = open_dialogs(&self.options, &self.panels, self.show_about);
        crate::dialog::set_keep_on_top(&ctx, self.settings.keep_dialogs_on_top());
        let state = CoreState {
            unsaved: session.unsaved(),
            period: session.rules().uses_period() && session.use_contest_period,
            period_possible: crate::registry::has_periods(),
            formats: session
                .formats()
                .iter()
                .map(|f| (f.name().to_string(), f.can_import(), f.can_export()))
                .collect(),
        };
        let core: Vec<CoreCommands> = commands::CORE_GROUPS
            .iter()
            .map(|g| CoreCommands {
                group: *g,
                state: state.clone(),
                chosen: None,
            })
            .collect();
        let lang = i18n::current();
        let mut all = commands::list(
            &core,
            self.panels.iter().map(|s| (s.panel.as_ref(), s.open)),
            lang,
        );
        let reopen = self.settings.reopen_windows();
        all.iter_mut()
            .flat_map(|l| &mut l.commands)
            .filter(|c| c.id == commands::REOPEN)
            .for_each(|c| c.selected = reopen);
        let tools = self
            .panels
            .iter()
            .any(|s| s.panel.window_kind() == zlog_ui::WindowKind::Tool)
            || own_window(self.entry.as_deref(), lang).is_some();
        if let Some(h) = self.window_host.as_ref().filter(|_| tools) {
            all.push(commands::Listed {
                source: Source::Host,
                id: h.section().to_string(),
                title: tr("menu.window").into(),
                group: zlog_ui::MenuGroup::Window,
                commands: h.commands(
                    &host_windows(&self.panels, self.entry.as_deref(), lang),
                    lang,
                ),
            });
        }
        self.listed = all
            .iter()
            .flat_map(|l| {
                let kids = l.commands.iter().flat_map(|c| &c.children);
                l.commands
                    .iter()
                    .chain(kids)
                    .map(|c| format!("{}/{}", l.id, c.id))
            })
            .collect();
        if self.options.open {
            options_commands(
                &mut self.options,
                &self.toolbar_rows,
                &all,
                &self.shortcuts,
                self.entry.as_deref(),
                lang,
            );
        }
        commands::apply_shortcuts(&mut all, &self.shortcuts);
        for l in &all {
            if let Source::Panel(i) = l.source {
                let window = l.commands.iter().find(|c| c.is_window());
                if let (Some(slot), Some(w)) = (self.panels.get_mut(i), window) {
                    slot.title = w.window_title();
                }
            }
        }
        let roles = zlog_ui::role_commands(all.iter().flat_map(|l| &l.commands));
        for slot in &mut self.panels {
            slot.panel.role_commands(&roles);
        }
        let asking = self.overwrite.is_some() || self.files.iter().any(|f| f.modal());
        let mut chosen = if self.options.wants_keys() || asking {
            None
        } else {
            commands::pressed(&ctx, &all)
        };
        let v = &mut self.view;
        let empty_entry = no_text_to_undo(
            focus(&ctx, self.entry.as_deref()).is_some(),
            nothing_typed(&session.entry),
            ctx.egui_wants_keyboard_input(),
            self.options.open,
        );
        if !asking && !modal(&self.panels) && v.start_time.is_none() {
            let activate = std::mem::take(&mut v.activate) && v.selected.is_some();
            let typing =
                focus(&ctx, self.entry.as_deref()).is_none() && zlog_ui::keys::text_focused(&ctx);
            let rows = v.selected.is_some() && !typing;
            match row_command(&ctx, &all, rows, empty_entry, activate) {
                Some(Ok(c)) => chosen = Some(c),
                Some(Err(why)) => session.status = Status::warning(why),
                None => {}
            }
        }
        let mut file_pick: Option<(usize, String)> = None;
        for pick in self.chrome(&ctx, ui, session, &all, &dialogs, lang) {
            if pick.source != chrome::HOST {
                if let Some(c) = chrome::listed(&all, &pick) {
                    chosen = Some(c);
                }
                continue;
            }
            if let Some(item) = chrome::HostItem::of(&pick.id, session) {
                let ran = self.host_item(&ctx, session, item, &all, &dialogs, lang);
                close_all |= ran.close_all;
                if ran.file.is_some() {
                    file_pick = ran.file;
                }
            }
        }
        let grid_command = self
            .view
            .grid_action
            .take_if(|(_, a)| matches!(a, main_window::GridAction::Command(..)));
        if let Some((_, main_window::GridAction::Command(source, id))) = grid_command {
            chosen = Some((source, id));
        }
        let by_role = self
            .run_role
            .take()
            .and_then(|role| command_of_role(&all, &role));
        let by_role = by_role.is_some() && {
            chosen = by_role;
            true
        };
        if let Some(c) = chosen {
            self.run_chosen(&ctx, session, &all, state, c, by_role, &mut close);
        }
        if close_all {
            self.options.open = false;
            self.show_about = false;
            for slot in &mut self.panels {
                slot.open = false;
                slot.panel.close_dialogs();
            }
        }
        self.settings_files(&ctx, session, lang, file_pick);
        self.main_area(&ctx, ui, session, &all);
        self.panel_windows(&ctx, session);
        self.entry_actions(session);
        self.about(&ctx);
        let title = format!(
            "{} - {} - {}",
            zlog_ui::product::NAME,
            session.rules().name(),
            session.path.display()
        );
        self.set_title(&ctx, title);
        let clock = until_next_minute(chrono::Utc::now());
        let tick = self
            .panels
            .iter()
            .filter_map(|s| s.panel.tick_after())
            .min();
        ctx.request_repaint_after(tick.map_or(clock, |t| t.min(clock)));
        close
    }

    fn chrome(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        session: &Session,
        all: &[commands::Listed],
        dialogs: &[(String, String)],
        lang: &str,
    ) -> Vec<zlog_ui::chrome::Pick> {
        let _ = ctx;
        if self.menu_bar.is_none() && self.tool_bar.is_none() {
            return Vec::new();
        }
        let state = chrome::MenuState {
            session,
            selected: self.view.selected,
            panels: &self.panels,
            files: self.files.iter().map(|f| f.commands(lang)).collect(),
            dialogs,
            on_top: self.settings.keep_dialogs_on_top(),
        };
        let menus = match self.menu_bar {
            Some(_) => chrome::menus(all, &state),
            None => Vec::new(),
        };
        let style = self.settings.toolbar_style();
        let buttons = chrome::toolbar(all, &mut self.settings);
        let connections = chrome::connections(&self.panels, lang);
        let (menu_bar, tool_bar) = (&mut self.menu_bar, &mut self.tool_bar);
        let (picks, chosen) = egui::Panel::top("menu")
            .show(ui, |ui| {
                let mut picks = Vec::new();
                if let Some(m) = menu_bar {
                    picks.extend(m.show(ui, &menus));
                }
                let chosen = chrome::bar_with_connections(ui, &connections, lang, |ui| {
                    if let Some(t) = tool_bar {
                        picks.extend(t.show(ui, &buttons, style));
                    }
                });
                (picks, chosen)
            })
            .inner;
        if let Some((slot, pick)) = chosen {
            self.connection_pick(slot, pick);
        }
        picks
    }

    fn host_item(
        &mut self,
        ctx: &egui::Context,
        session: &Session,
        item: chrome::HostItem,
        all: &[commands::Listed],
        dialogs: &[(String, String)],
        lang: &str,
    ) -> HostRan {
        let ctx = ctx.clone();
        let mut close_all = false;
        let mut file_pick = None;
        match Some(item) {
            Some(chrome::HostItem::Wizard) => self.options.start_wizard(&mut self.settings),
            Some(chrome::HostItem::Options) => {
                options_commands(
                    &mut self.options,
                    &self.toolbar_rows,
                    all,
                    &self.shortcuts,
                    self.entry.as_deref(),
                    lang,
                );
                self.options.show_with(&mut self.settings);
            }
            Some(chrome::HostItem::Language(code)) => {
                choose_language(&ctx, &mut self.settings, &code)
            }
            Some(chrome::HostItem::Front) => {
                for (id, _) in dialogs {
                    crate::dialog::focus(&ctx, id);
                }
            }
            Some(chrome::HostItem::OnTop) => {
                let on_top = !self.settings.keep_dialogs_on_top();
                self.settings.set_keep_dialogs_on_top(on_top);
                let _ = self.settings.save();
                crate::dialog::set_keep_on_top(&ctx, on_top);
                let level = if on_top {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                };
                for (id, _) in dialogs {
                    ctx.send_viewport_cmd_to(
                        crate::dialog::viewport_id(id),
                        egui::ViewportCommand::WindowLevel(level),
                    );
                }
            }
            Some(chrome::HostItem::CloseAll) => close_all = true,
            Some(chrome::HostItem::Focus(n)) => {
                if let Some((id, _)) = dialogs.get(n) {
                    if !self.window_host.as_mut().is_some_and(|h| h.focus(id)) {
                        crate::dialog::focus(&ctx, id);
                    }
                }
            }
            Some(chrome::HostItem::About) => self.show_about = true,
            Some(chrome::HostItem::Manual) => {
                ctx.open_url(egui::OpenUrl::new_tab(MANUAL_URL));
            }
            Some(chrome::HostItem::Qso(action)) => {
                let selected = self.view.selected.and_then(|i| session.log().qsos.get(i));
                if let Some(q) = selected {
                    self.view.grid_action = Some((zlog_core::QsoId::of(q), action));
                }
            }
            Some(chrome::HostItem::File(i, id)) => file_pick = Some((i, id)),
            None => {}
        }
        HostRan {
            close_all,
            file: file_pick,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_chosen(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        all: &[commands::Listed],
        state: CoreState,
        (source, id): (Source, String),
        by_role: bool,
        close: &mut bool,
    ) {
        let ctx = ctx.clone();
        let lang = i18n::current();
        if source == Source::Core(zlog_ui::MenuGroup::File) && id == commands::STATION {
            self.options.show_with(&mut self.settings);
            self.options.show_page("station");
        }
        if source == Source::Core(zlog_ui::MenuGroup::Settings) && id == commands::REOPEN {
            let on = !self.settings.reopen_windows();
            self.settings.set_reopen_windows(on);
            let _ = self.settings.save();
        }
        if source == Source::Core(zlog_ui::MenuGroup::Settings) && id == commands::OPTIONS {
            options_commands(
                &mut self.options,
                &self.toolbar_rows,
                all,
                &self.shortcuts,
                self.entry.as_deref(),
                lang,
            );
            self.options.show_with(&mut self.settings);
        }
        let mut ran = Ran {
            ctx: &ctx,
            session,
            view: &mut self.view,
            entry: self.entry.as_deref_mut(),
            close,
            overwrite: &mut self.overwrite,
        };
        match source {
            Source::Core(group) => {
                let mut provider = CoreCommands {
                    group,
                    state,
                    chosen: None,
                };
                provider.run(&id);
                if let Some(id) = provider.chosen {
                    ran.core(&id);
                }
            }
            Source::Host => {
                if let Some(h) = self.window_host.as_mut() {
                    h.run(
                        &id,
                        &host_windows(&self.panels, self.entry.as_deref(), lang),
                    );
                }
            }
            Source::Panel(i) => {
                if let Some(slot) = self.panels.get_mut(i) {
                    let dialog = slot.panel.window_kind() == zlog_ui::WindowKind::Dialog;
                    if dialog && slot.open && id == zlog_ui::Command::WINDOW {
                        crate::dialog::focus(&ctx, slot.panel.id());
                    }
                    let was_open = slot.open;
                    let opens = if by_role && id == zlog_ui::Command::WINDOW {
                        slot.open = true;
                        true
                    } else {
                        zlog_ui::PanelCommands {
                            panel: slot.panel.as_mut(),
                            open: &mut slot.open,
                        }
                        .run(&id);
                        let opens = commands::opens_window(all, source, &id)
                            && slot.panel.window_kind().has_window();
                        slot.open |= opens;
                        opens
                    };
                    if opens && was_open && slot.open {
                        let id = slot.panel.id();
                        if !self.window_host.as_mut().is_some_and(|h| h.focus(id)) {
                            crate::dialog::focus(&ctx, id);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn settings_files(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        lang: &str,
        file_pick: Option<(usize, String)>,
    ) {
        let ctx = ctx.clone();
        if file_pick.is_some() {
            let style = style_section(self.entry.as_deref(), &mut self.settings);
            let hosted = host_section(self.window_host.as_deref(), &mut self.settings);
            if panel_sections(&self.panels, &mut self.settings) | style | hosted {
                let _ = self.settings.save();
            }
        }
        let infos = section_infos(&self.options, &self.panels, &self.menu, lang);
        let mut sections = self.settings.sections().clone();
        let save = |s: &zlog_ui::Sections| {
            let mut all = Settings::default();
            all.set_sections(s.clone());
            all.save()
        };
        let migrations = crate::registry::migrations();
        let mut imported = false;
        for (i, f) in self.files.iter_mut().enumerate() {
            let mut env = zlog_ui::SettingsEnv {
                sections: &mut sections,
                infos: &infos,
                language: lang,
                save: &save,
                migrations: &migrations,
            };
            if let Some((_, id)) = file_pick.as_ref().filter(|(n, _)| *n == i) {
                f.run_command(id, &mut env);
            }
            imported |= f.dialogs(&ctx, &mut env);
        }
        if imported {
            self.settings.set_sections(sections);
            let _ = self.settings.save();
            for slot in &mut self.panels {
                load_panel(slot.panel.as_mut(), &mut self.settings);
            }
            load_rig_sections(&mut self.panels, &mut self.settings, &[]);
            if let Some(h) = self.window_host.as_mut() {
                h.load(&self.settings.section(h.section()).clone());
            }
            self.options.open = false;
            self.shortcuts = self.options.shortcuts(&mut self.settings);
            session.station = self.settings.station();
            session.use_contest_period = self.settings.use_contest_period();
            session.apply_preferences(&self.settings.preferences());
            load_start_screen(&mut self.menu, &mut self.settings);
            self.entry = loaded_style(&mut self.settings);
            self.restyle = true;
        }
    }

    fn main_area(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        session: &mut Session,
        all: &[commands::Listed],
    ) {
        let ctx = ctx.clone();
        if let Some(bar) = self.status_bar.as_mut() {
            let line = chrome::status_line(session, &self.status_items);
            egui::Panel::bottom("status").show(ui, |ui| bar.show(ui, &line));
        }
        let margin = ui.style().spacing.window_margin.sum().x;
        let grid = main_window::grid_width(ui, &mut self.view) + margin;
        let entry_need = self.view.entry_width.unwrap_or(grid);
        if let Some(h) = self.window_host.as_mut() {
            h.reserve_log(entry_need);
        }
        self.hosted_windows(&ctx, ui, session);
        let hz = rig_now(&self.panels, self.rig_number).hz;
        let mut typed = match self.entry.as_deref_mut() {
            Some(entry) if !entry.uses_main_row() => {
                let host = self.window_host.as_deref_mut();
                main_window::entry_row(ui, session, &mut self.view, entry, hz, host)
            }
            _ => false,
        };
        if !self.view.sends.is_empty() {
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
        let panels = &self.panels;
        let extra = |q: &zlog_core::Qso| qso_items(panels, q);
        let context = context_commands(all);
        let (view, entry) = (&mut self.view, self.entry.as_deref_mut());
        let log_table = self.log_table.as_deref_mut();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let out = egui::ScrollArea::horizontal()
                    .id_salt("log_unit")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let shown = ui.max_rect();
                        let width = shown.width().max(grid).max(entry_need);
                        let rect =
                            egui::Rect::from_min_size(shown.min, egui::vec2(width, shown.height()));
                        let mut unit = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                        if let Some(entry) = entry.filter(|e| e.uses_main_row()) {
                            typed = egui::Panel::bottom("entry")
                                .show(&mut unit, |ui| {
                                    let inner = ui.available_rect_before_wrap();
                                    let mut row =
                                        ui.new_child(egui::UiBuilder::new().max_rect(inner));
                                    let typed = main_window::entry_row(
                                        &mut row, session, view, entry, hz, None,
                                    );
                                    let used = row.min_rect();
                                    view.entry_width = Some(
                                        used.right() - rect.left() + rect.right() - inner.right(),
                                    );
                                    let used = egui::vec2(inner.width(), used.height());
                                    ui.advance_cursor_after_rect(egui::Rect::from_min_size(
                                        inner.min, used,
                                    ));
                                    typed
                                })
                                .inner;
                        }
                        egui::CentralPanel::default().show(&mut unit, |ui| {
                            if let Some(grid) = log_table {
                                main_window::log_table(ui, grid, session, view, &context, &extra)
                            }
                        });
                        ui.advance_cursor_after_rect(rect);
                    });
                zlog_ui::dialog::scroll_bars(ui, &out, egui::Vec2b::new(true, false));
                let shown = out.inner_rect;
                ui.interact(shown, out.id.with("view"), egui::Sense::empty());
                ui.ctx().accesskit_node_builder(out.id.with("view"), |n| {
                    n.set_role(egui::accesskit::Role::ScrollView);
                    n.set_label(tr("log.view"));
                    n.set_bounds(egui::accesskit::Rect {
                        x0: shown.min.x.into(),
                        y0: shown.min.y.into(),
                        x1: shown.max.x.into(),
                        y1: shown.max.y.into(),
                    });
                });
            });

        if typed {
            let view = panel_view(
                focus(&ctx, self.entry.as_deref()),
                editing(&self.panels),
                self.view.selected,
                session,
                &self.spots,
                rig_now(&self.panels, self.rig_number),
            );
            for slot in &mut self.panels {
                slot.panel.on_key(zlog_ui::EntryKey::Typed, &view);
            }
        }

        match self.view.grid_action.take() {
            Some((id, main_window::GridAction::Extension(i, item))) => {
                if let Some(q) = session.find(id).map(|n| session.log().qsos[n].clone()) {
                    if let Some(slot) = self.panels.get_mut(i) {
                        if slot.panel.on_qso_menu(&item, &q) {
                            slot.open = true;
                        }
                    }
                }
            }
            Some(action @ (_, main_window::GridAction::Command(..))) => {
                self.view.grid_action = Some(action)
            }
            Some((id, action)) => main_window::apply_grid_action(session, id, action),
            None => {}
        }
        start_time_dialog(&ctx, session, &mut self.view);
        self.wizard_tick();
        if let Some(changed) = self.options.ui(&ctx, &mut self.settings) {
            self.shortcuts = self.options.shortcuts(&mut self.settings);
            session.station = self.settings.station();
            session.use_contest_period = self.settings.use_contest_period();
            session.apply_preferences(&self.settings.preferences());
            load_start_screen(&mut self.menu, &mut self.settings);
            self.restyle = true;
            let style = self.settings.entry_style();
            if self.entry.as_ref().is_some_and(|e| e.id() != style) {
                self.entry = entry_style(&style);
            }
            if let Some(e) = self.entry.as_mut() {
                e.load(self.settings.section(zlog_ui::enter::SECTION));
            }
            for slot in &mut self.panels {
                if panel_affected(slot.panel.as_ref(), &changed) {
                    load_panel(slot.panel.as_mut(), &mut self.settings);
                }
            }
            load_rig_sections(&mut self.panels, &mut self.settings, &changed);
            if let Some(h) = self.window_host.as_mut() {
                if changed.iter().any(|s| s == h.section()) {
                    h.load(&self.settings.section(h.section()).clone());
                }
            }
        }
    }

    fn hosted_windows(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, session: &mut Session) {
        let Some(host) = self.window_host.as_mut() else {
            return;
        };
        let focus = focus(ctx, self.entry.as_deref());
        let editing = editing(&self.panels);
        let selected = self.view.selected;
        let rig = rig_now(&self.panels, self.rig_number);
        let lang = i18n::current();
        let spots = &self.spots;
        {
            let view = panel_view(focus, editing, selected, session, spots, rig);
            for slot in self.panels.iter_mut() {
                if slot.open && !slot.was_open && hosted(Some(host.as_ref()), slot.panel.as_ref()) {
                    slot.panel.opened(&view);
                    slot.was_open = true;
                    if !self.places.restored.remove(slot.panel.id()) {
                        host.focus(slot.panel.id());
                    }
                }
            }
        }
        let session = std::cell::RefCell::new(session);
        let mut windows = Vec::new();
        for slot in self.panels.iter_mut() {
            if !slot.open || !hosted(Some(host.as_ref()), slot.panel.as_ref()) {
                continue;
            }
            let title = slot.window_title(lang);
            let id = slot.panel.id().to_string();
            let (panel, session) = (&mut slot.panel, &session);
            windows.push(zlog_ui::chrome::HostedWindow {
                id,
                title,
                open: &mut slot.open,
                body: Box::new(move |ui| {
                    let s = session.borrow();
                    let view = panel_view(focus, editing, selected, &s, spots, rig);
                    zlog_ui::layout::pane(ui, |ui| {
                        panel.ui(ui, &view);
                        match zlog_ui::buttons_of(panel.as_mut()) {
                            Some(b) => {
                                ui.separator();
                                b.buttons(ui, &view)
                            }
                            None => false,
                        }
                    })
                }),
            });
        }
        let mut entry_open = true;
        let own = own_window(self.entry.as_deref(), lang).filter(|(id, _)| host.hosts(id));
        if let (Some((id, title)), Some(style)) = (own, self.entry.as_deref_mut()) {
            let (view, session) = (&mut self.view, &session);
            windows.push(zlog_ui::chrome::HostedWindow {
                id,
                title,
                open: &mut entry_open,
                body: Box::new(move |ui| {
                    let mut s = session.borrow_mut();
                    let mut row = crate::row::Row {
                        session: &mut s,
                        view,
                        hz: rig.hz,
                    };
                    style.docked_ui(ui, &mut row);
                    false
                }),
            });
        }
        host.show(ui, &mut windows, lang);
        drop(windows);
        if !entry_open {
            if let Some(style) = self.entry.as_deref_mut() {
                style.close_window();
            }
        }
        if host.take_changed() {
            self.entry_focus.moved();
            host.save(self.settings.section(host.section()));
            let _ = self.settings.save();
        }
    }

    fn panel_windows(&mut self, ctx: &egui::Context, session: &mut Session) {
        let ctx = ctx.clone();
        let view = panel_view(
            focus(&ctx, self.entry.as_deref()),
            editing(&self.panels),
            self.view.selected,
            session,
            &self.spots,
            rig_now(&self.panels, self.rig_number),
        );
        for slot in &mut self.panels {
            slot.panel.tick(&view);
        }
        let items: Vec<zlog_ui::StatusItem> = self
            .panels
            .iter()
            .flat_map(|s| s.panel.status(&view))
            .collect();
        if items != self.status_items {
            self.status_items = items;
            ctx.request_repaint();
        }
        let suggestions: Vec<zlog_ui::Suggestion> = self
            .panels
            .iter()
            .flat_map(|s| s.panel.suggestions(&view))
            .collect();
        let messages = self
            .panels
            .iter()
            .find_map(|s| s.panel.key_messages(&view))
            .unwrap_or_default();
        if messages != self.view.messages {
            self.view.messages = messages;
            ctx.request_repaint();
        }
        if suggestions != self.view.suggestions {
            self.view.suggestions = suggestions;
            ctx.request_repaint();
        }
        for slot in &mut self.panels {
            if slot.open && !slot.was_open {
                slot.panel.opened(&view);
                let restored = self.places.restored.remove(slot.panel.id());
                if let Some(h) = self.window_host.as_mut().filter(|_| !restored) {
                    h.focus(slot.panel.id());
                }
            }
            if slot.was_open != slot.open {
                self.entry_focus.moved();
            }
            slot.was_open = slot.open;
        }
        let host = &mut self.window_host;
        for slot in self
            .panels
            .iter_mut()
            .filter(|s| s.panel.window_kind().has_window())
        {
            if hosted(host.as_deref(), slot.panel.as_ref()) {
                continue;
            }
            let tool = slot.panel.window_kind() == zlog_ui::WindowKind::Tool;
            let mut wrap =
                |ui: &mut egui::Ui, id: &str, body: &mut dyn FnMut(&mut egui::Ui)| match host
                    .as_mut()
                    .filter(|_| tool)
                {
                    Some(h) => h.floating(ui, id, view.language, body),
                    None => body(ui),
                };
            let (id, title, size) = (
                slot.panel.id().to_string(),
                slot.window_title(view.language),
                slot.panel.default_size(),
            );
            if let Some(&r) = self.places.rects.get(&id).filter(|_| tool) {
                crate::dialog::place(&ctx, &id, r);
            }
            if zlog_ui::buttons_of(slot.panel.as_mut()).is_some() {
                let panel = std::cell::RefCell::new(&mut slot.panel);
                let mut close = false;
                crate::dialog::dialog_with_buttons(
                    &ctx,
                    &id,
                    &title,
                    size,
                    &mut slot.open,
                    |ui| wrap(ui, &id, &mut |ui| panel.borrow_mut().ui(ui, &view)),
                    |ui| {
                        let mut p = panel.borrow_mut();
                        if let Some(b) = zlog_ui::buttons_of(p.as_mut()) {
                            close |= b.buttons(ui, &view);
                        }
                    },
                );
                if close {
                    slot.open = false;
                }
            } else {
                let panel = &mut slot.panel;
                dialog(&ctx, &id, &title, size, &mut slot.open, |ui| {
                    wrap(ui, &id, &mut |ui| panel.ui(ui, &view))
                });
            }
            if tool && slot.open {
                if let Some(r) = crate::dialog::geometry(&ctx, &id) {
                    self.places.rects.insert(id.clone(), r);
                }
            }
        }
        for slot in &mut self.panels {
            slot.panel.dialogs(&ctx, &view);
        }
        let now = chrono::Utc::now().timestamp();
        self.spots.retain(|s| now - s.time < SPOT_LIFETIME);
    }

    fn wizard_tick(&mut self) {
        let first = !cfg!(any(test, feature = "testing"))
            && crate::wizard::first_run(
                &self.settings.section("station").clone(),
                &self.settings.section(crate::wizard::SECTION).clone(),
            );
        if first && !self.options.open {
            self.options.start_wizard(&mut self.settings);
        }
        match self.options.ask.take() {
            Some(crate::wizard::Ask::TestRig) => {
                let changed = self.options.apply_now(&mut self.settings, &["rig", "rig2"]);
                let force = if changed.is_empty() {
                    vec!["rig".to_string()]
                } else {
                    changed
                };
                load_rig_sections(&mut self.panels, &mut self.settings, &force);
                self.rig_test = Some(std::time::Instant::now());
            }
            Some(crate::wizard::Ask::TestKeyer) => {
                self.options.apply_now(&mut self.settings, &["panel.keyer"]);
                for slot in &mut self.panels {
                    if slot.panel.id() == "keyer" {
                        load_panel(slot.panel.as_mut(), &mut self.settings);
                        slot.panel.run_command("keyer.test");
                    }
                }
            }
            None => {}
        }
        if let Some(at) = self.rig_test {
            let hz = rig_of(&self.panels).and_then(|r| r.hz());
            let status = match hz {
                Some(hz) => Some((
                    crate::i18n::trf("wizard.rig_ok", &[&format!("{:.1}", hz as f64 / 1000.0)]),
                    true,
                )),
                None if at.elapsed() > std::time::Duration::from_secs(6) => {
                    let note = self
                        .panels
                        .iter()
                        .find_map(|s| {
                            zlog_ui::menu_note_of(s.panel.as_ref(), crate::i18n::current())
                        })
                        .unwrap_or_default();
                    Some((
                        format!("{} {note}", crate::i18n::tr("wizard.rig_fail")),
                        false,
                    ))
                }
                None => None,
            };
            if let (Some(st), Some(w)) = (status, self.options.wizard.as_mut()) {
                w.status = Some(st);
                self.rig_test = None;
            }
        }
    }

    fn connection_pick(&mut self, slot: usize, pick: zlog_ui::connection::ConnectionPick) {
        use zlog_ui::connection::ConnectionPick;
        match pick {
            ConnectionPick::Run(id) => {
                if let Some(s) = self.panels.get_mut(slot) {
                    s.panel.run_command(&id);
                }
                self.save_panels();
            }
            ConnectionPick::Settings(page) => self
                .connection_actions
                .push(zlog_ui::EntryAction::Options(page)),
            ConnectionPick::Stop => self
                .connection_actions
                .push(zlog_ui::EntryAction::StopSending),
        }
    }

    fn entry_actions(&mut self, session: &mut Session) {
        let mut actions: Vec<zlog_ui::EntryAction> = self
            .panels
            .iter_mut()
            .flat_map(|s| s.panel.take_actions())
            .collect();
        actions.append(&mut self.connection_actions);
        for action in device_first(actions) {
            if matches!(
                action,
                zlog_ui::EntryAction::Callsign(_)
                    | zlog_ui::EntryAction::Edit { .. }
                    | zlog_ui::EntryAction::LogEntry
            ) {
                if let Some(e) = &mut self.entry {
                    e.entry_changed();
                }
            }
            match action {
                zlog_ui::EntryAction::Callsign(c) => {
                    pick_callsign(&mut session.entry, c);
                    session.check_call();
                    if let Some(e) = &mut self.entry {
                        e.focus(EntryField::Call);
                    }
                }
                zlog_ui::EntryAction::TakeCallsign(c) => {
                    session.entry.callsign = c;
                    session.check_call();
                    if let Some(e) = &mut self.entry {
                        e.caret_to_end(EntryField::Call);
                    }
                }
                zlog_ui::EntryAction::Number(n) => {
                    session.entry.nr_rcvd = n;
                    session.check_number();
                    if let Some(e) = &mut self.entry {
                        e.focus(EntryField::Number);
                    }
                }
                zlog_ui::EntryAction::RstRcvd(rst) => {
                    session.entry.set_rst_rcvd(rst);
                }
                zlog_ui::EntryAction::Qsy(hz, mode) => {
                    let mut entry = crate::entry::Follow {
                        entry: &mut session.entry,
                        station: &session.station,
                    };
                    match rig_of_mut(&mut self.panels) {
                        Some(rig) => rig.qsy(hz, mode, &mut entry),
                        None => zlog_ui::rig::qsy_entry(&mut entry, zlog_core::band_of(hz), mode),
                    }
                }
                zlog_ui::EntryAction::Cq(cq) => session.entry.cq = cq,
                zlog_ui::EntryAction::SelectRig(n) => {
                    let selected = match rig_of_mut(&mut self.panels) {
                        Some(rig) => rig.select(n),
                        None => zlog_ui::rig::select_number(&mut self.rig_number, n),
                    };
                    if selected {
                        session.status = rig_selected(n);
                    }
                }
                zlog_ui::EntryAction::Transmit(on) => {
                    let keyed = self.panels.iter_mut().any(|s| s.panel.transmit(on));
                    if on && !keyed {
                        session.status = Status::warning(tr("status.no_ptt"));
                    }
                }
                zlog_ui::EntryAction::Edit { batch, done } => session.edit(batch, done),
                zlog_ui::EntryAction::Say(item) => {
                    session.status = Status {
                        text: item.text,
                        warning: item.warning,
                    }
                }
                zlog_ui::EntryAction::Operator(op) => {
                    self.settings
                        .section("station")
                        .insert("operator".into(), op.clone());
                    let _ = self.settings.save();
                    session.station.operator = op;
                }
                zlog_ui::EntryAction::Power(p) => session.entry.power = p,
                zlog_ui::EntryAction::Log(qso) => session.log_external(*qso),
                zlog_ui::EntryAction::LogEntry => {
                    let hz = rig_now(&self.panels, self.rig_number).hz;
                    let now = crate::session::now_for(session.log());
                    let _ = session.log_entry_with(now, hz, crate::session::Period::Warn);
                }
                zlog_ui::EntryAction::Run(role) => self.run_role = Some(role),
                zlog_ui::EntryAction::UseRig(n, port) => {
                    if let Some(rig) = rig_of_mut(&mut self.panels) {
                        let names = rig.sections();
                        if let Some(name) = names.get(usize::from(n.max(1)) - 1) {
                            rig.use_port(n, &port, self.settings.section(name));
                            let _ = self.settings.save();
                        }
                    }
                }
                zlog_ui::EntryAction::Options(page) => {
                    self.options.show_with(&mut self.settings);
                    self.options.show_page(&page);
                }
                zlog_ui::EntryAction::StopSending => {
                    let rig = rig_now(&self.panels, self.rig_number);
                    let view = panel_view(None, None, None, session, &self.spots, rig);
                    for slot in &mut self.panels {
                        slot.panel.on_key(zlog_ui::EntryKey::Escape, &view);
                    }
                }
                zlog_ui::EntryAction::Focus(field) => {
                    if let Some(e) = &mut self.entry {
                        e.focus(field);
                    }
                }
                zlog_ui::EntryAction::Spot(spot) => self.spots.push(spot),
                zlog_ui::EntryAction::SelectQso(id) => {
                    if let Some(i) = session.find(id) {
                        self.view.selected = Some(i);
                    }
                }
            }
        }
    }

    fn about(&mut self, ctx: &egui::Context) {
        let ctx = ctx.clone();
        let shown = crate::dialog::dialog_with_buttons(
            &ctx,
            "about",
            tr("menu.help.about"),
            [520.0, 360.0],
            &mut self.show_about,
            |ui| {
                ui.heading(zlog_ui::product::NAME);
                ui.label(env!("CARGO_PKG_VERSION"));
                ui.separator();
                ui.label(tr("about.contributors"));
                for call in CONTRIBUTORS {
                    ui.label(format!("  {call}"));
                }
            },
            |ui| {
                ui.button(tr("button.close")).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
            },
        );
        if shown.is_some_and(|(_, close)| close) {
            self.show_about = false;
        }
    }
}

pub(super) struct HostRan {
    close_all: bool,
    file: Option<(usize, String)>,
}

const CLOCK_MARGIN: std::time::Duration = std::time::Duration::from_millis(20);

fn until_next_minute(now: chrono::DateTime<chrono::Utc>) -> std::time::Duration {
    use chrono::Timelike;
    let into = std::time::Duration::from_secs(now.second().into())
        + std::time::Duration::from_nanos(now.nanosecond().into());
    std::time::Duration::from_secs(60).saturating_sub(into) + CLOCK_MARGIN
}

fn own_window(style: Option<&dyn zlog_ui::EntryStyle>, lang: &str) -> Option<(String, String)> {
    let style = style?;
    style
        .own_window()
        .map(|id| (id.to_string(), style.title(lang)))
}

fn host_windows<'a>(
    panels: &'a [PanelSlot],
    style: Option<&'a dyn zlog_ui::EntryStyle>,
    lang: &str,
) -> Vec<(&'a str, String)> {
    let mut all = tool_windows(panels, lang);
    if let Some(style) = style {
        if let Some(id) = style.own_window() {
            all.push((id, style.title(lang)));
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_repaints_just_after_the_next_minute() {
        let at = |s: &str| s.parse::<chrono::DateTime<chrono::Utc>>().unwrap();
        let ms = std::time::Duration::from_millis;
        assert_eq!(until_next_minute(at("2026-10-25T07:20:00Z")), ms(60_020));
        assert_eq!(until_next_minute(at("2026-10-25T07:20:59.900Z")), ms(120));
        assert_eq!(
            until_next_minute(at("2026-10-25T07:20:30.250Z")),
            ms(29_770)
        );
        assert_eq!(until_next_minute(at("2016-12-31T23:59:60.500Z")), ms(20));
    }
}
