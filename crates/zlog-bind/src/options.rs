//! The Shortcuts tab: every menu command by menu and section, its key now, and
//! buttons to press a new one, to take it away or to give it its own back; one
//! button gives every command its own. A key the entry row or the log grid keeps,
//! or one another command has, is refused with the reason.

use egui::{Event, Key, KeyboardShortcut, Ui};
use zlog_ui::keys::{self, CommandInfo, Overrides};
use zlog_ui::{MenuGroup, Section, SettingsPage};

use crate::keymap::{self, Conflict};
use crate::texts;

#[derive(Default)]
pub struct KeysPage {
    /// The commands of the menus, in menu order.
    commands: Vec<CommandInfo>,
    reserved: Vec<KeyboardShortcut>,
    /// The command waiting for its new key.
    recording: Option<String>,
    /// Why the last key was refused.
    refused: Option<String>,
    /// The pass of egui the tab was last drawn in, and its context: a recording
    /// stops when the tab was not drawn on the pass before (another tab, Options
    /// closed or cancelled).
    drawn: Option<(egui::Context, u64)>,
}

/// Whether a tab last drawn on pass `last` is not being drawn without a break on
/// pass `now`.
fn left(last: Option<u64>, now: u64) -> bool {
    last.is_none_or(|p| p.saturating_add(1) < now)
}

/// Where `group` stands in the menu bar.
fn position(group: MenuGroup) -> usize {
    let bar = [MenuGroup::File, MenuGroup::Edit]
        .into_iter()
        .chain(MenuGroup::ALL)
        .chain([MenuGroup::Settings, MenuGroup::Help]);
    bar.into_iter()
        .position(|g| g == group)
        .unwrap_or(usize::MAX)
}

/// The key pressed this frame, when it is not a modifier alone.
fn pressed(ui: &Ui) -> Option<KeyboardShortcut> {
    ui.input(|i| {
        i.events.iter().find_map(|e| match e {
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => Some(KeyboardShortcut::new(*modifiers, *key)),
            _ => None,
        })
    })
}

impl KeysPage {
    /// Take the key pressed for the command being recorded, unless it conflicts.
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

    /// One command: its label, its key now and its buttons.
    fn row(&mut self, ui: &mut Ui, section: &mut Section, i: usize, lang: &str) {
        let t = texts();
        let command = self.commands[i].clone();
        let now = keymap::current(section, &command);
        let recording = self.recording.as_deref() == Some(command.key.as_str());
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(&command.label).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let own = section.contains_key(&command.key);
                if ui
                    .add_enabled(own, egui::Button::new(t.get(lang, "default")))
                    .on_hover_text(t.get(lang, "default.tip"))
                    .clicked()
                {
                    self.refused = default_refused(section, &self.commands, &command, lang);
                    if self.refused.is_none() {
                        keymap::set(section, &command, command.default);
                    }
                }
                if ui
                    .add_enabled(now.is_some(), egui::Button::new(t.get(lang, "clear")))
                    .on_hover_text(t.get(lang, "clear.tip"))
                    .clicked()
                {
                    keymap::set(section, &command, None);
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
                let shown = now.map_or_else(
                    || t.get(lang, "none").to_string(),
                    |k| ui.ctx().format_shortcut(&k),
                );
                let text = egui::RichText::new(shown).monospace();
                ui.label(if now.is_some() { text } else { text.weak() });
            });
        });
    }
}

/// Why `command` cannot have its own shortcut back: another command has it now.
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

impl SettingsPage for KeysPage {
    fn id(&self) -> &str {
        "bind"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn set_commands(&mut self, commands: &[CommandInfo], reserved: &[KeyboardShortcut]) {
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

    fn shortcuts(&self, section: &Section) -> Option<Overrides> {
        Some(keymap::load(section))
    }

    fn wants_keys(&self) -> bool {
        self.recording.is_some()
            && self
                .drawn
                .as_ref()
                .is_some_and(|(ctx, p)| !left(Some(*p), ctx.cumulative_pass_nr()))
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
        ui.horizontal_wrapped(|ui| {
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
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for i in 0..self.commands.len() {
                    let c = &self.commands[i];
                    let first = i == 0 || self.commands[i - 1].group != c.group;
                    if first {
                        if i > 0 {
                            ui.add_space(8.0);
                        }
                        ui.label(egui::RichText::new(&c.group_title).strong());
                    } else if self.commands[i - 1].section != c.section {
                        ui.add_space(4.0);
                    }
                    self.row(ui, section, i, lang);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Modifiers;
    use zlog_ui::MenuSection;

    fn info(key: &str, group: MenuGroup, section: MenuSection) -> CommandInfo {
        CommandInfo {
            key: key.into(),
            label: key.into(),
            group,
            group_title: String::new(),
            section,
            weight: 0,
            default: None,
        }
    }

    /// Listed by menu and section; a choice for a command no longer linked in is
    /// handed back but matches nothing.
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
        let o = p.shortcuts(&s).unwrap();
        let k = o["gone/window"].unwrap();
        assert_eq!(k, KeyboardShortcut::new(Modifiers::COMMAND, Key::G));
        assert!(!p.wants_keys());
    }

    /// A recording stops once the tab is not drawn on the next pass.
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
        // Never drawn: no keys wanted.
        assert!(!p.wants_keys());
        let ctx = egui::Context::default();
        p.drawn = Some((ctx.clone(), ctx.cumulative_pass_nr()));
        assert!(p.wants_keys());
    }

    /// Its own shortcut back is refused while another command has it.
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
}
