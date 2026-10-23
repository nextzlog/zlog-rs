//! The application's own commands (File, Edit, Device) as [`zlog_ui::Commands`]
//! providers, and the menu bar, toolbar and the toolbar tab of Options built from
//! one list of every provider's commands: the application's first, then the
//! panels'. Menus are sorted by [`MenuSection`], weight and label, not by the order
//! the providers are listed in, with separators between sections.

use egui::{Key, KeyboardShortcut, Modifiers};
use zlog_ui::keys::{CommandInfo, Overrides};
use zlog_ui::{icon, Command, Commands, MenuGroup, MenuSection};

use crate::i18n::tr_in;
use crate::settings::{Settings, ToolbarStyle};

/// Where a command comes from, to run it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// The application's provider of this group (File, Edit, Device).
    Core(MenuGroup),
    /// The panel with this index.
    Panel(usize),
}

/// What the application's commands depend on now.
#[derive(Debug, Clone, Default)]
pub struct CoreState {
    /// The log changed since it was saved ([`crate::session::Session::unsaved`]).
    pub unsaved: bool,
    /// The contest is checked against a period (`menuCorrectStartTime` enabled).
    pub period: bool,
    /// Formats of the extensions: name, can import, can export.
    pub formats: Vec<(String, bool, bool)>,
    pub rig_connected: bool,
    pub show_rig: bool,
}

/// One of the application's providers: its group and the state it shows. Running
/// a command records it; the application carries it out.
pub struct CoreCommands {
    pub group: MenuGroup,
    pub state: CoreState,
    pub chosen: Option<String>,
}

/// The groups of the application's providers, in order.
pub const CORE_GROUPS: [MenuGroup; 3] = [MenuGroup::File, MenuGroup::Edit, MenuGroup::Device];

impl Commands for CoreCommands {
    fn id(&self) -> &str {
        match self.group {
            MenuGroup::File => "file",
            MenuGroup::Edit => "edit",
            _ => "device",
        }
    }

    fn title(&self, language: &str) -> String {
        tr_in(language, crate::menus::group_key(self.group)).into()
    }

    fn group(&self) -> MenuGroup {
        self.group
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        match self.group {
            MenuGroup::File => file_commands(&self.state, language),
            MenuGroup::Edit => edit_commands(&self.state, language),
            _ => device_commands(&self.state, language),
        }
    }

    fn run(&mut self, id: &str) {
        self.chosen = Some(id.to_string());
    }
}

/// Ctrl+S (Cmd+S on macOS).
pub const SAVE_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

/// A command labelled by `label` with the tooltip `cmd.<id>.tip`.
fn cmd(lang: &str, id: &str, label: &str) -> Command {
    Command::new(
        id,
        tr_in(lang, label),
        tr_in(lang, &format!("cmd.{id}.tip")),
    )
}

/// File: New/Open, Save, Save As, Import, Export, Correct start time, Exit
/// (zLog's File menu; Open and Save also on the toolbar by default, Import and
/// Export hidden until chosen in the toolbar tab of Options).
pub fn file_commands(s: &CoreState, lang: &str) -> Vec<Command> {
    let mut open = cmd(lang, "file.open", "toolbar.open")
        .icon(icon::FOLDER_OPEN)
        .on_toolbar(true)
        .toolbar_key("file.open")
        .section(MenuSection::Log, 0);
    open.menu_label = Some(tr_in(lang, "menu.file.open").into());
    let mut save = cmd(lang, "file.save", "menu.file.save")
        .icon(icon::FLOPPY_DISK)
        .shortcut(SAVE_KEY)
        .on_toolbar(true)
        .toolbar_key("file.save")
        .section(MenuSection::Log, 1);
    if !s.unsaved {
        save = save.disabled_because(tr_in(lang, "cmd.file.save.none"));
    }
    let format = |dir: &str, i: usize, name: &str| {
        Command::new(
            format!("file.{dir}.{i}"),
            name,
            trf_in(lang, "cmd.file.format.tip", name),
        )
    };
    let port = |dir: &str, glyph, weight, can: fn(&(String, bool, bool)) -> bool| {
        let children: Vec<Command> = s
            .formats
            .iter()
            .enumerate()
            .filter(|(_, f)| can(f))
            .map(|(i, f)| format(dir, i, &f.0))
            .collect();
        let mut c = cmd(lang, &format!("file.{dir}"), &format!("toolbar.{dir}"))
            .icon(glyph)
            .on_toolbar(false)
            .toolbar_key(format!("file.{dir}"))
            .section(MenuSection::Exchange, weight)
            .children(children);
        c.menu_label = Some(tr_in(lang, &format!("menu.file.{dir}")).into());
        if c.children.is_empty() {
            c = c.disabled_because(tr_in(lang, &format!("cmd.file.{dir}.none")));
        }
        c
    };
    let import = port("import", icon::DOWNLOAD_SIMPLE, 0, |f| f.1);
    let export = port("export", icon::UPLOAD_SIMPLE, 1, |f| f.2);
    let mut start =
        cmd(lang, "file.start_time", "menu.file.start_time").section(MenuSection::Period, 0);
    if !s.period {
        start = start.disabled_because(tr_in(lang, "cmd.file.start_time.none"));
    }
    vec![
        open,
        save,
        cmd(lang, "file.save_as", "menu.file.save_as").section(MenuSection::Log, 2),
        cmd(lang, "file.save_as_zlo", "menu.file.save_as_zlo").section(MenuSection::Log, 3),
        import,
        export,
        start,
        cmd(lang, "file.exit", "menu.file.exit").section(MenuSection::Quit, 0),
    ]
}

/// Edit: the entry row's own keys, the last section of the menu (after Undo and
/// Redo, and the QSO commands of the extensions with the Band, Mode and Power
/// submenus).
pub fn edit_commands(_s: &CoreState, lang: &str) -> Vec<Command> {
    let shift = |k| KeyboardShortcut::new(Modifiers::SHIFT, k);
    let mut list = Vec::new();
    let entry = [
        ("edit.next_band", shift(Key::B)),
        ("edit.next_mode", shift(Key::M)),
        ("edit.next_power", shift(Key::P)),
        ("edit.cq_sp", shift(Key::O)),
        (
            "edit.clear",
            KeyboardShortcut::new(Modifiers::COMMAND, Key::K),
        ),
    ];
    for (i, (id, k)) in entry.into_iter().enumerate() {
        let label = format!("menu.{id}");
        let c = cmd(lang, id, &label).key_hint(k);
        list.push(c.section(MenuSection::Entry, i as i32));
    }
    list
}

/// Device: connecting the rig and its window, above the panels of the group.
pub fn device_commands(s: &CoreState, lang: &str) -> Vec<Command> {
    let mut connect = cmd(lang, "device.connect", "menu.device.connect").icon(icon::PLUGS);
    if s.rig_connected {
        connect = connect.disabled_because(tr_in(lang, "cmd.device.connect.busy"));
    }
    let mut disconnect =
        cmd(lang, "device.disconnect", "menu.device.disconnect").icon(icon::PLUGS_CONNECTED);
    if !s.rig_connected {
        disconnect = disconnect.disabled_because(tr_in(lang, "cmd.device.disconnect.none"));
    }
    let mut window = cmd(lang, "device.rig_window", "menu.rig.window").selected(s.show_rig);
    window.checkable = true;
    // Shift+X, handled by the entry row (`actionToggleRig`).
    let toggle = cmd(lang, "device.toggle_rig", "menu.device.toggle_rig")
        .key_hint(KeyboardShortcut::new(Modifiers::SHIFT, Key::X));
    [connect, disconnect, window, toggle]
        .into_iter()
        .enumerate()
        .map(|(i, c)| c.section(MenuSection::Rig, i as i32))
        .collect()
}

fn trf_in(lang: &str, key: &str, arg: &str) -> String {
    zlog_core::text::substitute(tr_in(lang, key), &[&arg])
}

/// One provider's commands in the common list.
pub struct Listed {
    pub source: Source,
    /// The provider's [`Commands::id`].
    pub id: String,
    pub title: String,
    pub group: MenuGroup,
    pub commands: Vec<Command>,
}

/// Every provider's commands in `lang`: the application's, then the panels'.
pub fn list<'a>(
    core: &[CoreCommands],
    panels: impl IntoIterator<Item = (&'a dyn zlog_ui::Panel, bool)>,
    lang: &str,
) -> Vec<Listed> {
    let mut all: Vec<Listed> = core
        .iter()
        .map(|c| Listed {
            source: Source::Core(c.group),
            id: c.id().to_string(),
            title: c.title(lang),
            group: c.group,
            commands: c.commands(lang),
        })
        .collect();
    for (i, (p, open)) in panels.into_iter().enumerate() {
        all.push(Listed {
            source: Source::Panel(i),
            id: p.id().to_string(),
            title: p.title(lang),
            group: p.menu(),
            commands: zlog_ui::panel_commands(p, open, lang),
        });
    }
    all
}

/// The providers of `group`, in list order.
pub fn of_group(all: &[Listed], group: MenuGroup) -> impl Iterator<Item = &Listed> {
    all.iter().filter(move |l| l.group == group)
}

/// Where a group's buttons stand on the toolbar: File first, then the extension
/// menus in menu-bar order.
fn toolbar_position(group: MenuGroup) -> usize {
    match group {
        MenuGroup::File => 0,
        g => 1 + crate::menus::position(g),
    }
}

/// The name the toolbar settings remember a button under.
pub fn toolbar_key<'a>(l: &'a Listed, c: &'a Command) -> &'a str {
    c.toolbar_key.as_deref().unwrap_or(&l.id)
}

/// A row of the toolbar tab: the buttons remembered under one key.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarRow {
    pub key: String,
    pub label: String,
    /// Shown until the operator chooses otherwise.
    pub default: bool,
    pub group: MenuGroup,
}

/// The rows of the toolbar tab, one per key, by group in toolbar order and within
/// a group in `order` (the operator's, [`Settings::toolbar_order`]), the keys it
/// does not name after it in menu order (section, weight, label).
pub fn toolbar_rows(all: &[Listed], order: &[String]) -> Vec<ToolbarRow> {
    let mut rows: Vec<(usize, (MenuSection, i32, String), ToolbarRow)> = Vec::new();
    for l in all {
        for c in l.commands.iter().filter(|c| c.toolbar) {
            let key = toolbar_key(l, c);
            if rows.iter().any(|r| r.2.key == key) {
                continue;
            }
            let label = if c.toolbar_key.is_some() {
                c.menu_text().to_string()
            } else {
                l.title.clone()
            };
            rows.push((
                toolbar_position(l.group),
                zlog_ui::section::sort_key(c.section, c.weight, &label),
                ToolbarRow {
                    key: key.into(),
                    label,
                    default: c.toolbar_default,
                    group: l.group,
                },
            ));
        }
    }
    rows.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let mut rows: Vec<ToolbarRow> = rows.into_iter().map(|r| r.2).collect();
    in_order(&mut rows, order);
    rows
}

/// `rows`, in menu order, put in `order` within each group; the keys `order` does
/// not name keep their place after those it does.
pub fn in_order(rows: &mut [ToolbarRow], order: &[String]) {
    rows.sort_by_key(|r| {
        let chosen = order.iter().position(|k| *k == r.key);
        (toolbar_position(r.group), chosen.unwrap_or(usize::MAX))
    });
}

/// The toolbar's buttons shown now, by group in toolbar order and within a group
/// in the order of [`toolbar_rows`]: (source, command).
pub fn toolbar_buttons<'a>(
    all: &'a [Listed],
    settings: &mut Settings,
) -> Vec<Vec<(Source, &'a Command)>> {
    let rows = toolbar_rows(all, &settings.toolbar_order());
    let mut groups: Vec<(MenuGroup, Vec<(Source, &Command)>)> = Vec::new();
    for row in rows {
        if !settings.toolbar_shown(&row.key, row.default) {
            continue;
        }
        let key = row.key.as_str();
        let buttons = all.iter().flat_map(|l| {
            l.commands
                .iter()
                .filter(move |c| c.toolbar && toolbar_key(l, c) == key)
                .map(move |c| (l.source, c))
        });
        match groups.last_mut() {
            Some((g, list)) if *g == row.group => list.extend(buttons),
            _ => groups.push((row.group, buttons.collect())),
        }
    }
    groups.retain(|(_, g)| !g.is_empty());
    groups.into_iter().map(|(_, g)| g).collect()
}

/// A menu's items by section, in menu order: only sections with items.
pub type Sections = Vec<(MenuSection, Vec<(Source, Command)>)>;

/// `items` sorted by section, weight and label, split into sections.
pub fn sectioned(items: impl IntoIterator<Item = (Source, Command)>) -> Sections {
    zlog_ui::section::split(
        items
            .into_iter()
            .map(|(s, c)| (c.section, c.weight, c.menu_text().to_string(), (s, c)))
            .collect(),
    )
}

/// The menu items of `group`'s providers, by section.
pub fn menu_sections(all: &[Listed], group: MenuGroup) -> Sections {
    sectioned(of_group(all, group).flat_map(|l| {
        l.commands
            .iter()
            .filter(|c| c.menu)
            .map(|c| (l.source, c.clone()))
    }))
}

/// The items of `sections` with a separator between two sections; `extra` draws
/// more at the end of a section (the Edit menu's submenus of a QSO) and may choose
/// a command too. Returns the command chosen.
pub fn draw_sections(
    ui: &mut egui::Ui,
    sections: &Sections,
    mut extra: impl FnMut(&mut egui::Ui, MenuSection) -> Option<(Source, String)>,
) -> Option<(Source, String)> {
    let mut chosen = None;
    for (n, (section, items)) in sections.iter().enumerate() {
        if n > 0 {
            ui.separator();
        }
        for (source, c) in items {
            if let Some(id) = menu_items(ui, std::slice::from_ref(c)) {
                chosen = Some((*source, id));
            }
        }
        if let Some(c) = extra(ui, *section) {
            chosen = Some(c);
        }
    }
    chosen
}

/// Keys the entry row and the log grid keep for themselves, whatever the
/// commands' keys ([`Command::key_hint`], [`Command::row_key`]) add: moving and
/// logging, and the F keys of the keyer and voice messages.
const RESERVED: [Key; 22] = [
    Key::Enter,
    Key::Space,
    Key::Escape,
    Key::Tab,
    Key::Backspace,
    Key::Delete,
    Key::ArrowUp,
    Key::ArrowDown,
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
];

/// Every menu command of `all` (and of its submenus) as a tab assigning
/// shortcuts lists it, with the keys no shortcut may take.
pub fn command_infos(all: &[Listed], lang: &str) -> (Vec<CommandInfo>, Vec<KeyboardShortcut>) {
    let mut infos = Vec::new();
    let mut reserved: Vec<KeyboardShortcut> = RESERVED
        .iter()
        .map(|k| KeyboardShortcut::new(Modifiers::NONE, *k))
        .collect();
    for l in all {
        let group_title = tr_in(lang, crate::menus::group_key(l.group)).to_string();
        let mut add = |c: &Command, label: String| {
            reserved.extend(c.key_hint.iter().chain(&c.row_key).copied());
            if c.menu && c.children.is_empty() {
                infos.push(CommandInfo {
                    key: zlog_ui::keys::command_key(&l.id, &c.id),
                    label,
                    group: l.group,
                    group_title: group_title.clone(),
                    section: c.section,
                    weight: c.weight,
                    default: c.shortcut,
                });
            }
        };
        for c in &l.commands {
            add(c, c.menu_text().to_string());
            for child in &c.children {
                add(child, format!("{} > {}", c.menu_text(), child.menu_text()));
            }
        }
    }
    (infos, reserved)
}

/// Put the shortcuts chosen into the commands of `all` (and their submenus).
pub fn apply_shortcuts(all: &mut [Listed], overrides: &Overrides) {
    if overrides.is_empty() {
        return;
    }
    for l in all {
        let set = |c: &mut Command| {
            if let Some(k) = overrides.get(&zlog_ui::keys::command_key(&l.id, &c.id)) {
                c.shortcut = *k;
            }
        };
        for c in &mut l.commands {
            set(c);
            c.children.iter_mut().for_each(set);
        }
    }
}

/// The command whose row key ([`Command::row_key`]) was pressed this frame
/// (consumed), enabled or not.
///
/// With `modified_only`, only row keys with Ctrl, Cmd or Alt count: they type
/// nothing, so they may also run from an entry row with nothing typed in it.
pub fn row_key_pressed<'a>(
    ctx: &egui::Context,
    all: &'a [Listed],
    modified_only: bool,
) -> Option<(Source, &'a Command)> {
    for l in all {
        for c in &l.commands {
            let modified = |k: &KeyboardShortcut| {
                let m = k.modifiers;
                m.ctrl || m.command || m.mac_cmd || m.alt
            };
            if let Some(k) = c.row_key.as_ref().filter(|k| !modified_only || modified(k)) {
                if ctx.input_mut(|i| i.consume_shortcut(k)) {
                    return Some((l.source, c));
                }
            }
        }
    }
    None
}

/// The command whose row key ([`Command::row_key`]) is `key`, enabled or not.
pub fn with_row_key<'a>(
    all: &'a [Listed],
    key: &egui::KeyboardShortcut,
) -> Option<(Source, &'a Command)> {
    all.iter()
        .flat_map(|l| l.commands.iter().map(move |c| (l.source, c)))
        .find(|(_, c)| c.row_key.as_ref() == Some(key))
}

/// The command whose shortcut was pressed this frame (consumed), if enabled.
pub fn pressed(ctx: &egui::Context, all: &[Listed]) -> Option<(Source, String)> {
    for (source, c, k) in shortcut_order(all) {
        if ctx.input_mut(|i| i.consume_shortcut(&k)) {
            return Some((source, c.id.clone()));
        }
    }
    None
}

/// The enabled commands with a shortcut, those of submenus too (of enabled
/// parents), those asking for more modifiers first: egui ignores extra Shift and
/// Alt, so Ctrl+Shift+S must be tried before Ctrl+S.
pub fn shortcut_order(all: &[Listed]) -> Vec<(Source, &Command, KeyboardShortcut)> {
    fn walk<'a>(
        source: Source,
        commands: &'a [Command],
        out: &mut Vec<(Source, &'a Command, KeyboardShortcut)>,
    ) {
        for c in commands.iter().filter(|c| c.enabled) {
            if let Some(k) = c.shortcut {
                out.push((source, c, k));
            }
            walk(source, &c.children, out);
        }
    }
    let mut out = Vec::new();
    for l in all {
        walk(l.source, &l.commands, &mut out);
    }
    out.sort_by_key(|(_, _, k)| std::cmp::Reverse(zlog_ui::keys::specificity(k)));
    out
}

/// The menu items of `commands` (those listed in menus); returns the one chosen.
pub fn menu_items(ui: &mut egui::Ui, commands: &[Command]) -> Option<String> {
    let mut chosen = None;
    for c in commands.iter().filter(|c| c.menu) {
        if !c.children.is_empty() {
            ui.add_enabled_ui(c.enabled, |ui| {
                ui.menu_button(c.menu_text(), |ui| {
                    if let Some(id) = menu_items(ui, &c.children) {
                        chosen = Some(id);
                    }
                })
            })
            .response
            .on_disabled_hover_text(c.full_tooltip(None));
            continue;
        }
        let r = if c.checkable {
            let mut on = c.selected;
            ui.add_enabled(c.enabled, egui::Checkbox::new(&mut on, c.menu_text()))
        } else {
            let mut b = egui::Button::new(c.menu_text());
            if let Some(k) = c.menu_key() {
                b = b.shortcut_text(ui.ctx().format_shortcut(k));
            }
            ui.add_enabled(c.enabled, b)
        };
        let r = match &c.disabled_reason {
            Some(why) => r.on_disabled_hover_text(why),
            None => r,
        };
        if r.clicked() {
            chosen = Some(c.id.clone());
            if !c.checkable {
                ui.close();
            }
        }
    }
    chosen
}

/// A toolbar button for `c` in `style`; a submenu's button opens its commands.
/// Returns the command (or submenu command) chosen.
pub fn toolbar_button(ui: &mut egui::Ui, c: &Command, style: ToolbarStyle) -> Option<String> {
    let shortcut = c.shortcut.as_ref().map(|k| ui.ctx().format_shortcut(k));
    let tip = c.full_tooltip(shortcut.as_deref());
    let button = button(c, style);
    if !c.children.is_empty() {
        let mut chosen = None;
        let r = ui
            .add_enabled_ui(c.enabled, |ui| {
                egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
                    chosen = menu_items(ui, &c.children);
                })
            })
            .inner
            .0;
        named(r, c).on_hover_text(&tip).on_disabled_hover_text(&tip);
        return chosen;
    }
    let r = named(ui.add_enabled(c.enabled, button), c);
    r.on_hover_text(&tip)
        .on_disabled_hover_text(&tip)
        .clicked()
        .then(|| c.id.clone())
}

/// The button named by its label for accessibility (and the GUI tests), not by
/// the icon's private-use glyph.
fn named(r: egui::Response, c: &Command) -> egui::Response {
    r.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, c.enabled, c.selected, &c.label)
    });
    r
}

/// A toolbar button drawn in `style`; a command without an icon keeps its label.
fn button(c: &Command, style: ToolbarStyle) -> egui::Button<'static> {
    let icon = |g: &str| egui::RichText::new(g).family(icon::family());
    let label = egui::RichText::new(c.label.clone());
    match (c.icon, style) {
        (Some(g), ToolbarStyle::Icons) => egui::Button::selectable(c.selected, icon(g)),
        (Some(g), ToolbarStyle::IconsAndLabels) => {
            egui::Button::selectable(c.selected, (icon(g), label))
        }
        _ => egui::Button::selectable(c.selected, label),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core(state: &CoreState) -> Vec<CoreCommands> {
        CORE_GROUPS
            .iter()
            .map(|g| CoreCommands {
                group: *g,
                state: state.clone(),
                chosen: None,
            })
            .collect()
    }

    fn state() -> CoreState {
        CoreState {
            unsaved: true,
            period: true,
            formats: vec![
                ("ADIF".into(), true, true),
                ("Cabrillo".into(), false, true),
            ],
            ..Default::default()
        }
    }

    /// Every command of the application and of every panel, in both languages,
    /// has a label and a "Name — description" tooltip; toolbar commands have an
    /// icon of the font; ids are unique within their provider.
    #[test]
    fn every_command_has_texts_and_toolbar_commands_have_icons() {
        let panels = crate::registry::panels();
        for lang in ["en", "ja"] {
            let all = list(
                &core(&state()),
                panels.iter().map(|p| (p.as_ref(), false)),
                lang,
            );
            for l in &all {
                let mut ids = std::collections::HashSet::new();
                let mut every: Vec<&Command> = l.commands.iter().collect();
                every.extend(l.commands.iter().flat_map(|c| &c.children));
                for c in every {
                    let name = format!("{lang} {} {}", l.id, c.id);
                    assert!(ids.insert(c.id.clone()), "{name}: twice");
                    assert!(!c.label.is_empty() && c.label != "?", "{name}");
                    assert!(c.menu_text() != "?", "{name}");
                    // A panel's window toggle made from its title is in the menu only.
                    if c.toolbar || matches!(l.source, Source::Core(_)) {
                        let (title, what) = c.tooltip.split_once(" — ").expect(&name);
                        assert!(!title.is_empty() && !what.is_empty(), "{name}");
                    }
                    if c.toolbar {
                        let g = c.icon.expect(&name);
                        assert!(icon::ALL.iter().any(|(_, x)| *x == g), "{name}");
                    }
                    if !c.enabled && l.source != Source::Core(MenuGroup::Edit) {
                        assert!(c.disabled_reason.is_some(), "{name}");
                    }
                }
            }
        }
    }

    #[test]
    fn save_is_disabled_with_a_reason_when_everything_is_saved() {
        let saved = CoreState {
            unsaved: false,
            ..state()
        };
        let save = |s: &CoreState| {
            file_commands(s, "en")
                .into_iter()
                .find(|c| c.id == "file.save")
                .unwrap()
        };
        let c = save(&saved);
        assert!(!c.enabled);
        assert_eq!(c.disabled_reason.as_deref(), Some("Every change is saved"));
        assert_eq!(c.shortcut, Some(SAVE_KEY));
        let c = save(&state());
        assert!(c.enabled);
        assert_eq!(
            c.full_tooltip(Some("Ctrl+S")),
            "Save — write the log to its file (Ctrl+S)"
        );
    }

    #[test]
    fn import_and_export_list_the_formats_that_can() {
        let f = file_commands(&state(), "en");
        let kids = |id: &str| -> Vec<String> {
            let c = f.iter().find(|c| c.id == id).unwrap();
            c.children.iter().map(|c| c.label.clone()).collect()
        };
        assert_eq!(kids("file.import"), ["ADIF"]);
        assert_eq!(kids("file.export"), ["ADIF", "Cabrillo"]);
        let none = file_commands(&CoreState::default(), "en");
        let i = none.iter().find(|c| c.id == "file.import").unwrap();
        assert!(!i.enabled && i.disabled_reason.is_some());
    }

    /// The File group is first on the toolbar with Open and Save shown and
    /// Import and Export hidden; the menu and the toolbar come from one list, so
    /// each button is there once.
    #[test]
    fn toolbar_defaults_and_one_list_for_menu_and_toolbar() {
        let dir = std::env::temp_dir().join(format!("zlog-gui-cmds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::settings::TEST_PATH.with(|p| *p.borrow_mut() = Some(dir.join("s.toml")));
        let mut settings = Settings::load();
        let panels = crate::registry::panels();
        let all = list(
            &core(&state()),
            panels.iter().map(|p| (p.as_ref(), false)),
            "en",
        );
        let rows = toolbar_rows(&all, &[]);
        let file: Vec<(&str, bool)> = rows
            .iter()
            .take(4)
            .map(|r| (r.key.as_str(), r.default))
            .collect();
        assert_eq!(
            file,
            [
                ("file.open", true),
                ("file.save", true),
                ("file.import", false),
                ("file.export", false)
            ]
        );
        let keys: std::collections::HashSet<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys.len(), rows.len(), "one row per key");
        for p in &panels {
            let has = p.commands("en").iter().any(|c| c.toolbar);
            assert_eq!(keys.contains(p.id()), has, "{}", p.id());
        }
        let bar = toolbar_buttons(&all, &mut settings);
        let first: Vec<&str> = bar[0].iter().map(|(_, c)| c.id.as_str()).collect();
        assert_eq!(first, ["file.open", "file.save"]);
        let mut seen = std::collections::HashSet::new();
        for (s, c) in bar.iter().flatten() {
            assert!(seen.insert((*s, c.id.clone())), "{}", c.id);
            // Every toolbar button is the same command as in the menus.
            let l = all.iter().find(|l| l.source == *s).unwrap();
            assert!(l.commands.iter().any(|x| std::ptr::eq(x, *c)));
        }
        settings.set_toolbar_shown("file.import", true);
        settings.set_toolbar_shown("file.save", false);
        let bar = toolbar_buttons(&all, &mut settings);
        let first: Vec<&str> = bar[0].iter().map(|(_, c)| c.id.as_str()).collect();
        assert_eq!(first, ["file.open", "file.import"]);
        // The operator's order within the group; keys it does not name follow.
        settings.set_toolbar_order(&["file.import".into(), "gone".into(), "file.open".into()]);
        let bar = toolbar_buttons(&all, &mut settings);
        let first: Vec<&str> = bar[0].iter().map(|(_, c)| c.id.as_str()).collect();
        assert_eq!(first, ["file.import", "file.open"]);
        let rows = toolbar_rows(&all, &settings.toolbar_order());
        let keys: Vec<&str> = rows.iter().take(4).map(|r| r.key.as_str()).collect();
        assert_eq!(
            keys,
            ["file.import", "file.open", "file.save", "file.export"]
        );
        crate::settings::TEST_PATH.with(|p| *p.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The sections of every menu with all panels, without each one, and with
    /// none: in menu order, none empty, so the separators drawn between them are
    /// never leading, trailing or doubled; the order never depends on which
    /// panels are linked in.
    #[test]
    fn menus_keep_their_order_and_separators_with_any_panels() {
        let panels = crate::registry::panels();
        let core = core(&state());
        let menu = |skip: Option<usize>, none: bool, group| {
            let linked = panels
                .iter()
                .enumerate()
                .filter(|(i, _)| !none && Some(*i) != skip)
                .map(|(_, p)| (p.as_ref(), false));
            let all = list(&core, linked, "en");
            let sections = menu_sections(&all, group);
            for w in sections.windows(2) {
                assert!(w[0].0 < w[1].0, "{group:?}: {:?} then {:?}", w[0].0, w[1].0);
            }
            assert!(sections.iter().all(|(_, items)| !items.is_empty()));
            sections
                .into_iter()
                .flat_map(|(_, items)| items)
                .map(|(_, c)| c.menu_text().to_string())
                .collect::<Vec<_>>()
        };
        let mut groups = MenuGroup::ALL.to_vec();
        groups.extend([
            MenuGroup::File,
            MenuGroup::Edit,
            MenuGroup::Settings,
            MenuGroup::Help,
        ]);
        for group in groups {
            let full = menu(None, false, group);
            for skip in 0..panels.len() {
                let part = menu(Some(skip), false, group);
                let kept: Vec<&String> = full.iter().filter(|l| part.contains(l)).collect();
                assert_eq!(kept, part.iter().collect::<Vec<_>>(), "{group:?}");
            }
            menu(None, true, group);
        }
    }

    /// Undo, Redo and Skip lead the Edit menu, the entry row's keys end it; the
    /// Device menu starts with the rig.
    #[test]
    fn sections_put_items_where_they_belong() {
        let panels = crate::registry::panels();
        let all = list(
            &core(&state()),
            panels.iter().map(|p| (p.as_ref(), false)),
            "en",
        );
        let edit = menu_sections(&all, MenuGroup::Edit);
        if panels.iter().any(|p| p.id() == "undo") {
            assert_eq!(edit[0].0, MenuSection::Undo);
        }
        assert_eq!(edit.last().unwrap().0, MenuSection::Entry);
        let device = menu_sections(&all, MenuGroup::Device);
        assert_eq!(device[0].0, MenuSection::Rig);
        assert_eq!(device[0].1[0].1.id, "device.connect");
        let file = menu_sections(&all, MenuGroup::File);
        let file: Vec<MenuSection> = file.iter().map(|(s, _)| *s).collect();
        assert_eq!(
            file,
            [
                MenuSection::Log,
                MenuSection::Exchange,
                MenuSection::Period,
                MenuSection::Quit
            ]
        );
    }

    /// The shortcuts chosen replace the commands' own; those of a provider not
    /// linked in are ignored; keys of the entry row are kept back.
    #[test]
    fn shortcuts_chosen_are_applied() {
        let panels = crate::registry::panels();
        let mut all = list(
            &core(&state()),
            panels.iter().map(|p| (p.as_ref(), false)),
            "en",
        );
        let (infos, reserved) = command_infos(&all, "en");
        let save = infos.iter().find(|c| c.key == "file/file.save").unwrap();
        assert_eq!(save.default, Some(SAVE_KEY));
        assert!(infos.iter().any(|c| c.key == "file/file.import.0"));
        let shift_b = KeyboardShortcut::new(Modifiers::SHIFT, Key::B);
        let enter = KeyboardShortcut::new(Modifiers::NONE, Key::Enter);
        assert!(reserved.contains(&shift_b) && reserved.contains(&enter));
        let f6 = KeyboardShortcut::new(Modifiers::COMMAND, Key::F6);
        let mut o = Overrides::new();
        o.insert("file/file.save".into(), None);
        o.insert("file/file.exit".into(), Some(f6));
        o.insert("file/file.import.0".into(), Some(SAVE_KEY));
        o.insert("gone/window".into(), Some(f6));
        apply_shortcuts(&mut all, &o);
        let file = &all[0].commands;
        let get = |id: &str| file.iter().find(|c| c.id == id).unwrap();
        assert_eq!(get("file.save").shortcut, None);
        assert_eq!(get("file.exit").shortcut, Some(f6));
        assert_eq!(get("file.import").children[0].shortcut, Some(SAVE_KEY));
        assert_eq!(get("file.open").shortcut, None);
    }

    /// A shortcut given to a submenu item runs it, and Ctrl+Shift+S runs the
    /// command that has it, not the one with Ctrl+S.
    #[test]
    fn shortcuts_of_submenus_run_and_the_most_specific_wins() {
        let mut all = list(&core(&state()), std::iter::empty(), "en");
        let f6 = KeyboardShortcut::new(Modifiers::COMMAND, Key::F6);
        let save_as =
            KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, SAVE_KEY.logical_key);
        let mut o = Overrides::new();
        o.insert("file/file.import.0".into(), Some(f6));
        o.insert("file/file.exit".into(), Some(save_as));
        apply_shortcuts(&mut all, &o);
        let press = |k: KeyboardShortcut| {
            let ctx = egui::Context::default();
            let mut input = egui::RawInput::default();
            let mut modifiers = k.modifiers;
            modifiers.ctrl |= modifiers.command;
            input.events.push(egui::Event::Key {
                key: k.logical_key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            });
            ctx.begin_pass(input);
            let chosen = pressed(&ctx, &all).map(|(_, id)| id);
            ctx.end_pass().textures_delta.clear();
            chosen
        };
        assert_eq!(press(f6).as_deref(), Some("file.import.0"));
        assert_eq!(press(save_as).as_deref(), Some("file.exit"));
        assert_eq!(press(SAVE_KEY).as_deref(), Some("file.save"));
    }

    #[test]
    fn core_providers_record_what_was_run() {
        let mut c = core(&state());
        c[0].run("file.save");
        assert_eq!(c[0].chosen.as_deref(), Some("file.save"));
        assert_eq!(c[0].group(), MenuGroup::File);
        assert_eq!(c[1].id(), "edit");
        let d = device_commands(
            &CoreState {
                rig_connected: true,
                ..state()
            },
            "en",
        );
        assert!(!d[0].enabled && d[1].enabled);
    }
}
