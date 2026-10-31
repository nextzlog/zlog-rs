use egui::{Event, Key, KeyboardShortcut, Ui};
use zlog_ui::keys::{self, CommandInfo, CommandOverrides};
use zlog_ui::{MenuGroup, Section, SettingsPage};

use crate::keymap::{self, Conflict};
use crate::texts;

#[derive(Default)]
pub struct KeysPage {
    commands: Vec<CommandInfo>,
    reserved: Vec<KeyboardShortcut>,
    recording: Option<String>,
    refused: Option<String>,
    drawn: Option<(egui::Context, u64)>,
    selected: Option<String>,
    filter: String,
    search: Option<egui::Id>,
    source: Vec<CommandInfo>,
    cached: Option<(String, Section, Vec<usize>)>,
    scroll: (f32, f32),
}

fn left(last: Option<u64>, now: u64) -> bool {
    last.is_none_or(|p| p.saturating_add(1) < now)
}

fn position(group: MenuGroup) -> usize {
    let bar = [MenuGroup::File, MenuGroup::Edit]
        .into_iter()
        .chain(MenuGroup::ALL)
        .chain([MenuGroup::Settings, MenuGroup::Help]);
    bar.into_iter()
        .position(|g| g == group)
        .unwrap_or(usize::MAX)
}

fn pressed(ui: &Ui) -> Option<KeyboardShortcut> {
    ui.input(|i| {
        i.events.iter().find_map(|e| match e {
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if !is_modifier(*key) => Some(KeyboardShortcut::new(*modifiers, *key)),
            _ => None,
        })
    })
}

fn is_modifier(key: Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::AltLeft
            | Key::AltRight
            | Key::SuperLeft
            | Key::SuperRight
    )
}

const WIDE: f32 = 720.0;

fn customised(section: &Section, command: &CommandInfo) -> bool {
    section.contains_key(&command.key)
        || [keymap::NAME, keymap::SHORT, keymap::TIP]
            .iter()
            .any(|p| section.contains_key(&format!("{p}{}", command.key)))
}

impl KeysPage {
    fn record(&mut self, ui: &Ui, section: &mut Section, lang: &str) {
        let Some(key) = self.recording.clone() else {
            return;
        };
        let Some(shortcut) = pressed(ui) else {
            ui.ctx().request_repaint();
            return;
        };
        self.recording = None;
        if shortcut.logical_key == Key::Escape && shortcut.modifiers.is_none() {
            return;
        }
        let Some(command) = self.commands.iter().find(|c| c.key == key) else {
            return;
        };
        let t = texts();
        let keys_text = keys::format(&shortcut);
        match keymap::conflict(section, &self.commands, &self.reserved, command, &shortcut) {
            Some(Conflict::Plain) => {
                self.refused = Some(t.format(lang, "refused.plain", &[&keys_text]));
            }
            Some(Conflict::Reserved) => {
                self.refused = Some(t.format(lang, "refused.reserved", &[&keys_text]));
            }
            Some(Conflict::Taken(other)) => {
                self.refused = Some(t.format(lang, "refused.taken", &[&keys_text, &other]));
            }
            None => {
                keymap::set(section, command, Some(shortcut));
                self.refused = None;
            }
        }
    }

    fn shown(&mut self, section: &Section, ctx: &egui::Context) -> Vec<usize> {
        if let Some((filter, s, shown)) = &self.cached {
            if *filter == self.filter && s == section {
                return shown.clone();
            }
        }
        let shown = self.filtered(section, ctx);
        self.cached = Some((self.filter.clone(), section.clone(), shown.clone()));
        shown
    }

    fn filtered(&self, section: &Section, ctx: &egui::Context) -> Vec<usize> {
        let words: Vec<String> = self
            .filter
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        (0..self.commands.len())
            .filter(|&i| {
                let c = &self.commands[i];
                let shortcut = keymap::current(section, c).map(|k| ctx.format_shortcut(&k));
                let text = [
                    Some(c.label.as_str()),
                    keymap::name(section, keymap::NAME, c),
                    Some(c.group_title.as_str()),
                    shortcut.as_deref(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
                let clash = clashing(section, &self.reserved, c).is_some();
                words.iter().all(|w| text.contains(w) || clash && w == "!")
            })
            .collect()
    }

    fn step(&mut self, ui: &Ui, shown: &[usize]) -> bool {
        if self.recording.is_some() || shown.is_empty() {
            return false;
        }
        let typing =
            ui.ctx().text_edit_focused() && ui.ctx().memory(|m| m.focused()) != self.search;
        if typing {
            return false;
        }
        let (up, down) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            )
        });
        if !up && !down {
            return false;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|k| shown.iter().position(|&i| &self.commands[i].key == k));
        let next = match (at, down) {
            (None, _) => 0,
            (Some(n), true) => (n + 1).min(shown.len() - 1),
            (Some(n), false) => n.saturating_sub(1),
        };
        self.selected = Some(self.commands[shown[next]].key.clone());
        self.refused = None;
        true
    }

    fn list(&mut self, ui: &mut Ui, section: &Section, shown: &[usize], moved: bool, lang: &str) {
        let t = texts();
        if shown.is_empty() {
            ui.label(egui::RichText::new(t.get(lang, "nothing")).weak());
            return;
        }
        let mut rows: Vec<(bool, usize)> = Vec::new();
        for (n, &i) in shown.iter().enumerate() {
            if n == 0 || self.commands[shown[n - 1]].group != self.commands[i].group {
                rows.push((true, i));
            }
            rows.push((false, i));
        }
        let height = ui.spacing().interact_size.y;
        let step = height + ui.spacing().item_spacing.y;
        let mut area = egui::ScrollArea::vertical()
            .id_salt("bind_list")
            .auto_shrink([false, false]);
        let at = self.selected.as_ref().and_then(|k| {
            rows.iter()
                .position(|&(h, i)| !h && &self.commands[i].key == k)
        });
        if let Some(at) = at.filter(|_| moved) {
            let (offset, view) = self.scroll;
            let top = at as f32 * step;
            if top < offset {
                area = area.vertical_scroll_offset(top);
            } else if top + step > offset + view {
                area = area.vertical_scroll_offset(top + step - view);
            }
        }
        let out = area.show_rows(ui, height, rows.len(), |ui, range| {
            for &(header, i) in &rows[range] {
                if header {
                    let title = egui::RichText::new(&self.commands[i].group_title).strong();
                    ui.add_sized(
                        [ui.available_width(), height],
                        egui::Label::new(title).halign(egui::Align::Min),
                    );
                    continue;
                }
                let c = &self.commands[i];
                let name = keymap::name(section, keymap::NAME, c).unwrap_or(&c.label);
                let mut right = keymap::current(section, c)
                    .map(|k| ui.ctx().format_shortcut(&k))
                    .unwrap_or_default();
                right.push_str(&marks(section, &self.reserved, c));
                let clash = clashing(section, &self.reserved, c);
                let selected = self.selected.as_deref() == Some(c.key.as_str());
                let mut text = egui::RichText::new(right).monospace();
                if clash.is_some() {
                    text = text.color(ui.visuals().warn_fg_color);
                }
                let row = egui::Button::selectable(selected, name.to_string())
                    .right_text(text)
                    .min_size(egui::vec2(ui.available_width(), height));
                let mut r = ui.add(row);
                if customised(section, c) {
                    r = r.on_hover_text(t.get(lang, "customised"));
                }
                if let Some(k) = clash {
                    r = r.on_hover_text(t.format(lang, "clash", &[&keys::format(&k)]));
                }
                if r.clicked() {
                    r.request_focus();
                }
                if r.clicked() && !selected {
                    self.selected = Some(c.key.clone());
                    self.recording = None;
                    self.refused = None;
                }
                if selected && moved {
                    r.request_focus();
                }
            }
        });
        self.scroll = (out.state.clamped_offset().y, out.inner_rect.height());
    }

    fn detail(&mut self, ui: &mut Ui, section: &mut Section, lang: &str) {
        let t = texts();
        let Some(command) = self
            .selected
            .as_ref()
            .and_then(|k| self.commands.iter().find(|c| &c.key == k))
            .cloned()
        else {
            ui.label(egui::RichText::new(t.get(lang, "choose")).weak());
            return;
        };
        ui.label(
            egui::RichText::new(format!("{} > {}", command.group_title, command.label)).strong(),
        );
        ui.add_space(4.0);
        let labels: Vec<&str> = ["field.name", "field.short", "field.tip", "field.shortcut"]
            .iter()
            .map(|k| t.get(lang, k))
            .collect();
        zlog_ui::form(ui, "bind_detail", &labels, |ui, form| {
            let width = form.width();
            ui.label(labels[0]);
            name_field(
                ui,
                section,
                &command,
                keymap::NAME,
                &command.label,
                width,
                lang,
            );
            ui.end_row();
            if let Some(own) = &command.short_label {
                ui.label(labels[1]);
                name_field(ui, section, &command, keymap::SHORT, own, width, lang);
                ui.end_row();
            }
            ui.label(labels[2]);
            name_field(
                ui,
                section,
                &command,
                keymap::TIP,
                &command.description,
                width,
                lang,
            );
            ui.end_row();
            ui.label(labels[3]);
            ui.horizontal_wrapped(|ui| self.shortcut(ui, section, &command, lang));
            ui.end_row();
        });
        ui.add_space(6.0);
        if ui
            .add_enabled(
                customised(section, &command),
                egui::Button::new(t.get(lang, "reset.one")),
            )
            .on_hover_text(t.get(lang, "reset.one.tip"))
            .clicked()
        {
            self.recording = None;
            self.refused = reset_one(section, &self.commands, &command, lang);
        }
    }

    fn shortcut(&mut self, ui: &mut Ui, section: &mut Section, command: &CommandInfo, lang: &str) {
        let t = texts();
        let now = keymap::current(section, command);
        let recording = self.recording.as_deref() == Some(command.key.as_str());
        let shown = now.map_or_else(
            || t.get(lang, "none").to_string(),
            |k| ui.ctx().format_shortcut(&k),
        );
        let text = egui::RichText::new(shown).monospace();
        if let Some(k) = clashing(section, &self.reserved, command) {
            let why = t.format(lang, "clash", &[&keys::format(&k)]);
            ui.colored_label(ui.visuals().warn_fg_color, text)
                .on_hover_text(why);
        } else {
            ui.label(if now.is_some() { text } else { text.weak() });
        }
        let label = if recording { "recording" } else { "record" };
        if ui
            .selectable_label(recording, t.get(lang, label))
            .on_hover_text(t.get(lang, "record.tip"))
            .clicked()
        {
            self.recording = (!recording).then(|| command.key.clone());
            self.refused = None;
        }
        if ui
            .add_enabled(now.is_some(), egui::Button::new(t.get(lang, "clear")))
            .on_hover_text(t.get(lang, "clear.tip"))
            .clicked()
        {
            keymap::set(section, command, None);
        }
        if ui
            .add_enabled(
                section.contains_key(&command.key),
                egui::Button::new(t.get(lang, "default")),
            )
            .on_hover_text(t.get(lang, "default.tip"))
            .clicked()
        {
            self.refused = default_refused(section, &self.commands, command, lang);
            if self.refused.is_none() {
                keymap::set(section, command, command.default);
            }
        }
    }
}

fn name_field(
    ui: &mut Ui,
    section: &mut Section,
    command: &CommandInfo,
    prefix: &str,
    own: &str,
    width: f32,
    lang: &str,
) {
    let t = texts();
    let what = match prefix {
        keymap::SHORT => "short",
        keymap::TIP => "tip",
        _ => "name",
    };
    let mut name = keymap::name(section, prefix, command)
        .unwrap_or("")
        .to_string();
    let r = if prefix == keymap::TIP {
        ui.add(
            egui::TextEdit::multiline(&mut name)
                .hint_text(own)
                .desired_rows(3)
                .desired_width(width),
        )
    } else {
        zlog_ui::sized_field(
            ui,
            egui::TextEdit::singleline(&mut name).hint_text(own),
            width,
        )
    };
    if r.on_hover_text(t.format(lang, &format!("{what}.tip"), &[&own]))
        .changed()
    {
        keymap::set_name(section, prefix, command, own, &name);
    }
}

fn clashing(
    section: &Section,
    reserved: &[KeyboardShortcut],
    command: &CommandInfo,
) -> Option<KeyboardShortcut> {
    let k = keymap::current(section, command)?;
    reserved.iter().any(|r| keys::overlap(r, &k)).then_some(k)
}

fn marks(section: &Section, reserved: &[KeyboardShortcut], command: &CommandInfo) -> String {
    let mut out = String::new();
    if customised(section, command) {
        out.push_str(" •");
    }
    if clashing(section, reserved, command).is_some() {
        out.push_str(" !");
    }
    out
}

fn reset_one(
    section: &mut Section,
    commands: &[CommandInfo],
    command: &CommandInfo,
    lang: &str,
) -> Option<String> {
    let refused = default_refused(section, commands, command, lang);
    if refused.is_none() {
        for p in [keymap::NAME, keymap::SHORT, keymap::TIP] {
            section.remove(&format!("{p}{}", command.key));
        }
        keymap::set(section, command, command.default);
    }
    refused
}

fn default_refused(
    section: &Section,
    commands: &[CommandInfo],
    command: &CommandInfo,
    lang: &str,
) -> Option<String> {
    let own = command.default?;
    match keymap::conflict(section, commands, &[], command, &own) {
        Some(Conflict::Taken(other)) => {
            Some(texts().format(lang, "refused.default", &[&keys::format(&own), &other]))
        }
        _ => None,
    }
}

impl zlog_ui::ShortcutPage for KeysPage {
    fn set_commands(&mut self, commands: &[CommandInfo], reserved: &[KeyboardShortcut]) {
        if self.source == commands && self.reserved == reserved {
            return;
        }
        self.source = commands.to_vec();
        self.cached = None;
        let mut list = commands.to_vec();
        list.sort_by_cached_key(|c| {
            (
                position(c.group),
                zlog_ui::section::sort_key(c.section, c.weight, &c.label),
            )
        });
        self.commands = list;
        self.reserved = reserved.to_vec();
    }

    fn overrides(&self, section: &Section) -> CommandOverrides {
        keymap::overrides(section)
    }

    fn wants_keys(&self) -> bool {
        self.recording.is_some()
            && self
                .drawn
                .as_ref()
                .is_some_and(|(ctx, p)| !left(Some(*p), ctx.cumulative_pass_nr()))
    }
}

impl SettingsPage for KeysPage {
    fn id(&self) -> &str {
        "bind"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn shortcuts(&self) -> Option<&dyn zlog_ui::ShortcutPage> {
        Some(self)
    }

    fn shortcuts_mut(&mut self) -> Option<&mut dyn zlog_ui::ShortcutPage> {
        Some(self)
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut Section, lang: &str) {
        let t = texts();
        let now = ui.ctx().cumulative_pass_nr();
        if left(self.drawn.as_ref().map(|d| d.1), now) {
            self.recording = None;
            self.refused = None;
        }
        self.drawn = Some((ui.ctx().clone(), now));
        self.record(ui, section, lang);
        let search = egui::TextEdit::singleline(&mut self.filter).hint_text(t.get(lang, "search"));
        self.search = Some(zlog_ui::form::text(ui, search).id);
        zlog_ui::form::buttons(ui, |ui| {
            if ui
                .add_enabled(!section.is_empty(), egui::Button::new(t.get(lang, "reset")))
                .on_hover_text(t.get(lang, "reset.tip"))
                .clicked()
            {
                section.clear();
                self.recording = None;
            }
            if let Some(why) = &self.refused {
                ui.colored_label(ui.visuals().warn_fg_color, why);
            }
        });
        ui.add(egui::Label::new(egui::RichText::new(t.get(lang, "note")).weak()).wrap());
        ui.add_space(4.0);
        let shown = self.shown(section, ui.ctx());
        let moved = self.step(ui, &shown);
        let size = ui.available_size();
        let top = egui::Layout::top_down(egui::Align::Min);
        if size.x >= WIDE {
            ui.horizontal_top(|ui| {
                let list = egui::vec2((size.x * 0.45).max(280.0), size.y);
                ui.allocate_ui_with_layout(list, top, |ui| {
                    self.list(ui, section, &shown, moved, lang)
                });
                ui.separator();
                ui.vertical(|ui| self.detail(ui, section, lang));
            });
        } else {
            let list = egui::vec2(size.x, (size.y * 0.5).max(160.0));
            ui.allocate_ui_with_layout(list, top, |ui| self.list(ui, section, &shown, moved, lang));
            ui.separator();
            self.detail(ui, section, lang);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Modifiers;
    use zlog_ui::MenuSection;
    use zlog_ui::ShortcutPage;

    fn info(key: &str, group: MenuGroup, section: MenuSection) -> CommandInfo {
        CommandInfo {
            key: key.into(),
            label: key.into(),
            group,
            group_title: String::new(),
            section,
            weight: 0,
            default: None,
            short_label: None,
            description: String::new(),
        }
    }

    #[test]
    fn commands_in_menu_order_and_choices_handed_back() {
        let mut p = KeysPage::default();
        p.set_commands(
            &[
                info("score/window", MenuGroup::Report, MenuSection::Score),
                info("edit/edit.clear", MenuGroup::Edit, MenuSection::Entry),
                info("undo/undo", MenuGroup::Edit, MenuSection::Undo),
                info("file/file.save", MenuGroup::File, MenuSection::Log),
            ],
            &[],
        );
        let keys: Vec<&str> = p.commands.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "file/file.save",
                "undo/undo",
                "edit/edit.clear",
                "score/window"
            ]
        );
        let mut s = Section::new();
        s.insert("gone/window".into(), "Ctrl+G".into());
        s.insert("name:gone/window".into(), "Gone".into());
        let o = p.overrides(&s);
        let k = o.shortcuts["gone/window"].unwrap();
        assert_eq!(o.names["gone/window"], "Gone");
        assert_eq!(k, KeyboardShortcut::new(Modifiers::COMMAND, Key::G));
        assert!(!p.wants_keys());
    }

    #[test]
    fn modifiers_alone_are_not_recorded() {
        for key in [
            Key::AltLeft,
            Key::ControlRight,
            Key::ShiftLeft,
            Key::SuperLeft,
        ] {
            assert!(is_modifier(key));
        }
        assert!(!is_modifier(Key::S));
        assert!(!is_modifier(Key::F5));
    }

    #[test]
    fn recording_stops_when_the_tab_is_left() {
        assert!(left(None, 5));
        assert!(!left(Some(4), 5));
        assert!(!left(Some(5), 5));
        assert!(left(Some(3), 5));
        let mut p = KeysPage {
            recording: Some("file/file.save".into()),
            ..KeysPage::default()
        };
        assert!(!p.wants_keys());
        let ctx = egui::Context::default();
        p.drawn = Some((ctx.clone(), ctx.cumulative_pass_nr()));
        assert!(p.wants_keys());
    }

    #[test]
    fn shortcuts_the_entry_keeps_are_flagged() {
        let alt_n = KeyboardShortcut::new(Modifiers::ALT, Key::N);
        let mut save = info("file/file.save", MenuGroup::File, MenuSection::Log);
        let mut s = Section::new();
        assert_eq!(clashing(&s, &[alt_n], &save), None);
        keymap::set(&mut s, &save, Some(alt_n));
        assert_eq!(clashing(&s, &[alt_n], &save), Some(alt_n));
        assert_eq!(clashing(&s, &[], &save), None);
        keymap::set(&mut s, &save, None);
        save.default = Some(alt_n);
        s.remove(&save.key);
        assert_eq!(
            clashing(&s, &[alt_n], &save),
            Some(alt_n),
            "a default clashes too"
        );
        assert_eq!(marks(&s, &[alt_n], &save), " !");
    }

    #[test]
    fn the_search_finds_clashes_by_the_mark() {
        let alt_n = KeyboardShortcut::new(Modifiers::ALT, Key::N);
        let mut save = info("file/file.save", MenuGroup::File, MenuSection::Log);
        save.default = Some(alt_n);
        let open = info("file/file.open", MenuGroup::File, MenuSection::Log);
        let mut p = KeysPage::default();
        p.set_commands(&[save, open], &[alt_n]);
        let ctx = egui::Context::default();
        let s = Section::new();
        p.filter = "!".into();
        assert_eq!(p.shown(&s, &ctx).len(), 1);
        p.filter = String::new();
        assert_eq!(p.shown(&s, &ctx).len(), 2);
    }

    #[test]
    fn default_checks_conflicts() {
        let ctrl_s = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let mut save = info("file/file.save", MenuGroup::File, MenuSection::Log);
        save.default = Some(ctrl_s);
        let open = info("file/file.open", MenuGroup::File, MenuSection::Log);
        let commands = vec![save.clone(), open.clone()];
        let mut s = Section::new();
        keymap::set(&mut s, &save, None);
        keymap::set(&mut s, &open, Some(ctrl_s));
        assert!(default_refused(&s, &commands, &save, "en").is_some());
        keymap::set(&mut s, &open, None);
        assert!(default_refused(&s, &commands, &save, "en").is_none());
    }

    #[test]
    fn reset_one_checks_the_default_first() {
        let ctrl_s = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let mut save = info("file/file.save", MenuGroup::File, MenuSection::Log);
        save.default = Some(ctrl_s);
        let open = info("file/file.open", MenuGroup::File, MenuSection::Log);
        let commands = vec![save.clone(), open.clone()];
        let mut s = Section::new();
        keymap::set(&mut s, &save, None);
        keymap::set(&mut s, &open, Some(ctrl_s));
        s.insert(format!("{}{}", keymap::NAME, save.key), "Mine".into());
        let before = s.clone();
        assert!(reset_one(&mut s, &commands, &save, "en").is_some());
        assert_eq!(s, before, "nothing reset");
        keymap::set(&mut s, &open, None);
        assert!(reset_one(&mut s, &commands, &save, "en").is_none());
        assert!(!customised(&s, &save));
        assert_eq!(keymap::current(&s, &save), Some(ctrl_s));
    }

    #[test]
    fn the_list_marks_clashes() {
        let alt_n = KeyboardShortcut::new(Modifiers::ALT, Key::N);
        let save = info("file/file.save", MenuGroup::File, MenuSection::Log);
        let mut s = Section::new();
        assert_eq!(marks(&s, &[alt_n], &save), "");
        keymap::set(&mut s, &save, Some(alt_n));
        assert_eq!(marks(&s, &[alt_n], &save), " • !");
        assert_eq!(marks(&s, &[], &save), " •");
    }

    #[test]
    fn the_sorted_and_filtered_lists_are_cached() {
        let mut p = KeysPage::default();
        let list = [
            info("b/x", MenuGroup::Edit, MenuSection::Entry),
            info("a/y", MenuGroup::File, MenuSection::Log),
        ];
        p.set_commands(&list, &[]);
        let ctx = egui::Context::default();
        let s = Section::new();
        assert_eq!(p.shown(&s, &ctx), [0, 1]);
        p.commands.clear();
        p.set_commands(&list, &[]);
        assert!(p.commands.is_empty(), "same commands are not sorted again");
        assert_eq!(p.shown(&s, &ctx), [0, 1], "cached");
        p.filter = "y".into();
        p.set_commands(&list[..1], &[]);
        assert_eq!(p.shown(&s, &ctx), Vec::<usize>::new());
    }
}
