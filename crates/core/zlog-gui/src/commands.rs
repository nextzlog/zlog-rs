use egui::{Key, KeyboardShortcut, Modifiers};
use zlog_ui::keys::{CommandInfo, CommandOverrides};
use zlog_ui::{icon, Command, Commands, MenuGroup, MenuSection};

use crate::i18n::tr_in;
use crate::settings::Settings;
use zlog_ui::toolbar::ToolbarRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Core(MenuGroup),
    Panel(usize),
    Host,
}

#[derive(Debug, Clone, Default)]
pub struct CoreState {
    pub unsaved: bool,
    pub period: bool,
    pub period_possible: bool,
    pub formats: Vec<(String, bool, bool)>,
}

pub struct CoreCommands {
    pub group: MenuGroup,
    pub state: CoreState,
    pub chosen: Option<String>,
}

pub const CORE_GROUPS: [MenuGroup; 3] = [MenuGroup::File, MenuGroup::Edit, MenuGroup::Settings];

pub(crate) const OPTIONS: &str = "settings.options";

pub(crate) const REOPEN: &str = "settings.reopen";

impl Commands for CoreCommands {
    fn id(&self) -> &str {
        match self.group {
            MenuGroup::Edit => "edit",
            MenuGroup::Settings => "settings",
            _ => "file",
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
            MenuGroup::Edit => edit_commands(&self.state, language),
            MenuGroup::Settings => settings_commands(language),
            _ => file_commands(&self.state, language),
        }
    }

    fn run(&mut self, id: &str) {
        self.chosen = Some(id.to_string());
    }
}

pub const STATION: &str = "file.station";

pub(crate) const SAVE_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

fn cmd(lang: &str, id: &str, label: &str) -> Command {
    Command::new(
        id,
        tr_in(lang, label),
        tr_in(lang, &format!("cmd.{id}.tip")),
    )
}

pub fn file_commands(s: &CoreState, lang: &str) -> Vec<Command> {
    let mut open = cmd(lang, "file.open", "toolbar.open")
        .icon(icon::FOLDER_OPEN)
        .on_toolbar(true)
        .toolbar_key("file.open")
        .section(MenuSection::Log, 0)
        .role("open")
        .dialog();
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
        .dialog()
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
        (!c.children.is_empty()).then_some(c)
    };
    let import = port("import", icon::DOWNLOAD_SIMPLE, 0, |f| f.1);
    let export = port("export", icon::UPLOAD_SIMPLE, 1, |f| f.2);
    let start = s.period_possible.then(|| {
        let start = cmd(lang, "file.start_time", "menu.file.start_time")
            .section(MenuSection::Period, 0)
            .dialog();
        match s.period {
            true => start,
            false => start.disabled_because(tr_in(lang, "cmd.file.start_time.none")),
        }
    });
    let mut station = cmd(lang, STATION, "options.station").role("station");
    station.menu = false;
    [
        Some(open),
        Some(station),
        Some(save),
        Some(
            cmd(lang, "file.save_as", "menu.file.save_as")
                .section(MenuSection::Log, 2)
                .dialog(),
        ),
        Some(
            cmd(lang, "file.save_as_zlo", "menu.file.save_as_zlo")
                .section(MenuSection::Log, 3)
                .dialog(),
        ),
        import,
        export,
        start,
        Some(cmd(lang, "file.exit", "menu.file.exit").section(MenuSection::Quit, 0)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

pub(crate) fn edit_commands(_s: &CoreState, lang: &str) -> Vec<Command> {
    let shift = |k| KeyboardShortcut::new(Modifiers::SHIFT, k);
    let mut list = Vec::new();
    let entry = [
        ("edit.next_band", shift(Key::B)),
        ("edit.next_mode", shift(Key::M)),
        ("edit.next_power", shift(Key::P)),
        ("edit.cq_sp", shift(Key::O)),
        ("edit.clear", KeyboardShortcut::new(Modifiers::CTRL, Key::K)),
    ];
    let glyphs = [
        icon::SQUARES_FOUR,
        icon::TEXTBOX,
        icon::PLUS_MINUS,
        icon::MEGAPHONE,
        icon::BROOM,
    ];
    for (i, ((id, k), glyph)) in entry.into_iter().zip(glyphs).enumerate() {
        let label = format!("menu.{id}");
        let c = cmd(lang, id, &label).key_hint(k).icon(glyph);
        list.push(c.section(MenuSection::Entry, i as i32));
    }
    list
}

pub(crate) fn settings_commands(lang: &str) -> Vec<Command> {
    let mut options = cmd(lang, OPTIONS, "toolbar.options")
        .icon(icon::GEAR)
        .dialog();
    options.menu = false;
    let mut reopen = cmd(lang, REOPEN, "menu.settings.reopen")
        .icon(icon::CLOCK_COUNTER_CLOCKWISE)
        .on_toolbar(false);
    reopen.checkable = true;
    vec![options, reopen]
}

pub(crate) fn opens_window(all: &[Listed], source: Source, id: &str) -> bool {
    all.iter()
        .filter(|l| l.source == source)
        .flat_map(|l| &l.commands)
        .any(|c| c.id == id && c.opens_window)
}

fn trf_in(lang: &str, key: &str, arg: &str) -> String {
    zlog_core::text::substitute(tr_in(lang, key), &[&arg])
}

#[derive(Clone)]
pub struct Listed {
    pub source: Source,
    pub id: String,
    pub title: String,
    pub group: MenuGroup,
    pub commands: Vec<Command>,
}

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

pub(crate) fn of_group(all: &[Listed], group: MenuGroup) -> impl Iterator<Item = &Listed> {
    all.iter().filter(move |l| l.group == group)
}

pub fn toolbar_key(l: &Listed, c: &Command) -> Option<String> {
    match (&c.toolbar_key, c.toolbar) {
        (Some(k), _) if c.toolbar || c.menu => Some(k.clone()),
        (None, true) => Some(l.id.clone()),
        (None, false) if c.menu => Some(zlog_ui::keys::command_key(&l.id, &c.id)),
        _ => None,
    }
}

pub fn toolbar_rows(all: &[Listed], order: &[String]) -> Vec<ToolbarRow> {
    let mut rows: Vec<(usize, (MenuSection, i32, String), ToolbarRow)> = Vec::new();
    for l in all {
        for c in &l.commands {
            let Some(key) = toolbar_key(l, c) else {
                continue;
            };
            if rows.iter().any(|r| r.2.key == key) {
                continue;
            }
            let window = l.commands.iter().find(|c| c.is_window());
            let label = match (&c.toolbar_key, window) {
                (None, _) if !c.toolbar && c.is_window() => c.window_title(),
                (None, _) if !c.toolbar => c.menu_text().to_string(),
                (Some(_), _) => c.menu_text().to_string(),
                (None, Some(w)) => w.window_title(),
                (None, None) => l.title.clone(),
            };
            rows.push((
                zlog_ui::toolbar::position(l.group),
                zlog_ui::section::sort_key(c.section, c.weight, &label),
                ToolbarRow {
                    key,
                    label,
                    default: c.toolbar && c.toolbar_default,
                    group: l.group,
                    group_title: crate::i18n::tr(crate::menus::group_key(l.group)).into(),
                },
            ));
        }
    }
    rows.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    let mut rows: Vec<ToolbarRow> = rows.into_iter().map(|r| r.2).collect();
    zlog_ui::toolbar::in_order(&mut rows, order);
    rows
}

pub(crate) fn toolbar_buttons<'a>(
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
                .filter(move |c| toolbar_key(l, c).as_deref() == Some(key))
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

pub(crate) type Sections = Vec<(MenuSection, Vec<(Source, Command)>)>;

pub(crate) fn sectioned(items: impl IntoIterator<Item = (Source, Command)>) -> Sections {
    zlog_ui::section::split(
        items
            .into_iter()
            .map(|(s, c)| (c.section, c.weight, c.menu_text().to_string(), (s, c)))
            .collect(),
    )
}

pub fn menu_sections(all: &[Listed], group: MenuGroup) -> Sections {
    sectioned(of_group(all, group).flat_map(|l| {
        l.commands
            .iter()
            .filter(|c| c.menu)
            .map(|c| (l.source, c.clone()))
    }))
}

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

pub fn command_infos(
    all: &[Listed],
    entry_keys: &[KeyboardShortcut],
    lang: &str,
) -> (Vec<CommandInfo>, Vec<KeyboardShortcut>) {
    let mut infos = Vec::new();
    let mut reserved: Vec<KeyboardShortcut> = RESERVED
        .iter()
        .map(|k| KeyboardShortcut::new(Modifiers::NONE, *k))
        .chain(entry_keys.iter().copied())
        .collect();
    for l in all {
        let group_title = tr_in(lang, crate::menus::group_key(l.group)).to_string();
        let mut add = |c: &Command, label: String| {
            reserved.extend(c.key_hint.iter().chain(&c.row_key).copied());
            if (c.menu || c.toolbar) && c.children.is_empty() {
                infos.push(CommandInfo {
                    key: zlog_ui::keys::command_key(&l.id, &c.id),
                    label,
                    group: l.group,
                    group_title: group_title.clone(),
                    section: c.section,
                    weight: c.weight,
                    default: c.shortcut,
                    short_label: c.toolbar.then(|| c.label.clone()),
                    description: c.description.clone(),
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

pub fn apply_shortcuts(all: &mut [Listed], overrides: &CommandOverrides) {
    if overrides.is_empty() {
        return;
    }
    for l in all {
        let set = |c: &mut Command| {
            let key = zlog_ui::keys::command_key(&l.id, &c.id);
            if let Some(k) = overrides.shortcuts.get(&key) {
                c.shortcut = *k;
            }
            if let Some(name) = overrides.names.get(&key) {
                c.rename(name);
            }
            if let Some(name) = overrides.short_names.get(&key) {
                c.rename_short(name);
            }
            if let Some(text) = overrides.descriptions.get(&key) {
                c.describe(text);
            }
        };
        for c in &mut l.commands {
            set(c);
            c.children.iter_mut().for_each(set);
        }
    }
}

pub(crate) fn row_key_pressed<'a>(
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
                if ctx.input_mut(|i| zlog_ui::keys::consume_exact(i, k.modifiers, k.logical_key)) {
                    return Some((l.source, c));
                }
            }
        }
    }
    None
}

pub(crate) fn with_row_key<'a>(
    all: &'a [Listed],
    key: &egui::KeyboardShortcut,
) -> Option<(Source, &'a Command)> {
    all.iter()
        .flat_map(|l| l.commands.iter().map(move |c| (l.source, c)))
        .find(|(_, c)| c.row_key.as_ref() == Some(key))
}

pub fn pressed(ctx: &egui::Context, all: &[Listed]) -> Option<(Source, String)> {
    for (source, c, k) in shortcut_order(all) {
        if ctx.input_mut(|i| zlog_ui::keys::consume_shortcut(i, k.modifiers, k.logical_key)) {
            return Some((source, c.id.clone()));
        }
    }
    None
}

pub(crate) fn shortcut_order(all: &[Listed]) -> Vec<(Source, &Command, KeyboardShortcut)> {
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
            period_possible: true,
            formats: vec![
                ("ADIF".into(), true, true),
                ("Cabrillo".into(), false, true),
            ],
        }
    }

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
                    if c.toolbar || matches!(l.source, Source::Core(_)) {
                        assert!(!c.description.is_empty(), "{name}");
                        assert!(!c.description.contains(" — "), "{name}");
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
            "Write the log to its file (Ctrl+S)"
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
        assert!(!none
            .iter()
            .any(|c| c.id == "file.import" || c.id == "file.export"));
    }

    #[test]
    fn start_time_is_listed_only_when_some_contest_has_a_period() {
        let start = |s: &CoreState| {
            file_commands(s, "en")
                .into_iter()
                .find(|c| c.id == "file.start_time")
        };
        assert!(start(&state()).unwrap().enabled);
        let other = CoreState {
            period: false,
            ..state()
        };
        assert!(start(&other).unwrap().disabled_reason.is_some());
        let never = CoreState {
            period_possible: false,
            ..state()
        };
        assert!(start(&never).is_none());
    }

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
            .filter(|r| !r.key.contains('/'))
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
            let list = zlog_ui::panel_commands(p.as_ref(), false, "en");
            let mut has = list.iter().filter(|c| c.toolbar);
            for c in has.clone() {
                assert!(keys.contains(c.toolbar_key.as_deref().unwrap_or(p.id())));
            }
            let own = has.any(|c| c.toolbar_key.is_none());
            assert_eq!(
                keys.contains(p.id()),
                own || list
                    .iter()
                    .any(|c| c.toolbar_key.as_deref() == Some(p.id())),
                "{}",
                p.id()
            );
        }
        let bar = toolbar_buttons(&all, &mut settings);
        let first: Vec<&str> = bar[0].iter().map(|(_, c)| c.id.as_str()).collect();
        assert_eq!(first, ["file.open", "file.save"]);
        let mut seen = std::collections::HashSet::new();
        for (s, c) in bar.iter().flatten() {
            assert!(seen.insert((*s, c.id.clone())), "{}", c.id);
            let l = all.iter().find(|l| l.source == *s).unwrap();
            assert!(l.commands.iter().any(|x| std::ptr::eq(x, *c)));
        }
        settings.set_toolbar_shown("file.import", true);
        settings.set_toolbar_shown("file.save", false);
        let bar = toolbar_buttons(&all, &mut settings);
        let first: Vec<&str> = bar[0].iter().map(|(_, c)| c.id.as_str()).collect();
        assert_eq!(first, ["file.open", "file.import"]);
        settings.set_toolbar_order(&["file.import".into(), "gone".into(), "file.open".into()]);
        let bar = toolbar_buttons(&all, &mut settings);
        let first: Vec<&str> = bar[0].iter().map(|(_, c)| c.id.as_str()).collect();
        assert_eq!(first, ["file.import", "file.open"]);
        let rows = toolbar_rows(&all, &settings.toolbar_order());
        let keys: Vec<&str> = rows
            .iter()
            .map(|r| r.key.as_str())
            .filter(|k| !k.contains('/'))
            .take(4)
            .collect();
        assert_eq!(
            keys,
            ["file.import", "file.open", "file.save", "file.export"]
        );
        crate::settings::TEST_PATH.with(|p| *p.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(dir);
    }

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

    #[test]
    fn shortcuts_chosen_are_applied() {
        let panels = crate::registry::panels();
        let mut all = list(
            &core(&state()),
            panels.iter().map(|p| (p.as_ref(), false)),
            "en",
        );
        let style = crate::registry::entry_style("zlog").unwrap();
        let (infos, reserved) = command_infos(&all, &style.reserved_keys(), "en");
        let (_, without) = command_infos(&all, &[], "en");
        let alt_n = KeyboardShortcut::new(Modifiers::ALT, Key::N);
        assert!(!without.iter().any(|r| zlog_ui::keys::overlap(r, &alt_n)));
        {
            use zlog_bind::keymap::{conflict, Conflict};
            let save = infos.iter().find(|c| c.key == "file/file.save").unwrap();
            let section = zlog_ui::Section::new();
            assert!(style.reserved_keys().contains(&alt_n));
            let refused = conflict(&section, &infos, &reserved, save, &alt_n);
            assert!(matches!(refused, Some(Conflict::Reserved)));
            assert!(conflict(&section, &infos, &without, save, &alt_n).is_none());
        }
        let save = infos.iter().find(|c| c.key == "file/file.save").unwrap();
        assert_eq!(save.default, Some(SAVE_KEY));
        assert!(infos.iter().any(|c| c.key == "file/file.import.0"));
        let shift_b = KeyboardShortcut::new(Modifiers::SHIFT, Key::B);
        let enter = KeyboardShortcut::new(Modifiers::NONE, Key::Enter);
        assert!(reserved.contains(&shift_b) && reserved.contains(&enter));
        for k in [
            KeyboardShortcut::new(Modifiers::ALT | Modifiers::SHIFT, Key::A),
            KeyboardShortcut::new(Modifiers::ALT, Key::M),
            KeyboardShortcut::new(Modifiers::COMMAND, Key::K),
        ] {
            assert!(reserved.iter().any(|r| zlog_ui::keys::overlap(r, &k)));
        }
        let f6 = KeyboardShortcut::new(Modifiers::COMMAND, Key::F6);
        let mut o = CommandOverrides::default();
        o.shortcuts.insert("file/file.save".into(), None);
        o.shortcuts.insert("file/file.exit".into(), Some(f6));
        o.shortcuts
            .insert("file/file.import.0".into(), Some(SAVE_KEY));
        o.shortcuts.insert("gone/window".into(), Some(f6));
        apply_shortcuts(&mut all, &o);
        let file = &all[0].commands;
        let get = |id: &str| file.iter().find(|c| c.id == id).unwrap();
        assert_eq!(get("file.save").shortcut, None);
        assert_eq!(get("file.exit").shortcut, Some(f6));
        assert_eq!(get("file.import").children[0].shortcut, Some(SAVE_KEY));
        assert_eq!(get("file.open").shortcut, None);
    }

    #[test]
    fn names_chosen_rename_the_commands_everywhere() {
        let panels = crate::registry::panels();
        let fresh = list(
            &core(&state()),
            panels.iter().map(|p| (p.as_ref(), false)),
            "en",
        );
        let mut all = fresh.clone();
        let mut o = CommandOverrides::default();
        o.names.insert("file/file.save".into(), "Write".into());
        o.names.insert("gone/window".into(), "Gone".into());
        o.short_names.insert("file/file.save".into(), "Wr".into());
        o.descriptions
            .insert("file/file.save".into(), "Mine".into());
        apply_shortcuts(&mut all, &o);
        let save = all[0]
            .commands
            .iter()
            .find(|c| c.id == "file.save")
            .unwrap();
        assert_eq!((save.label.as_str(), save.menu_name()), ("Wr", "Write"));
        assert!(
            save.full_tooltip(None).starts_with("Mine"),
            "{}",
            save.description
        );
        let menu = menu_sections(&all, MenuGroup::File);
        assert!(menu
            .iter()
            .flat_map(|(_, items)| items)
            .any(|(_, c)| c.menu_text() == "Write"));
        let dir = std::env::temp_dir().join(format!("zlog-gui-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::settings::TEST_PATH.with(|p| *p.borrow_mut() = Some(dir.join("s.toml")));
        let mut settings = Settings::load();
        let bar = toolbar_buttons(&all, &mut settings);
        assert!(bar.iter().flatten().any(|(_, c)| c.label == "Wr"));
        crate::settings::TEST_PATH.with(|p| *p.borrow_mut() = None);
        let rows = toolbar_rows(&all, &[]);
        assert!(rows
            .iter()
            .any(|r| r.key == "file.save" && r.label == "Write"));
        let mut same = fresh.clone();
        apply_shortcuts(&mut same, &CommandOverrides::default());
        let texts = |l: &[Listed]| -> Vec<String> {
            l.iter()
                .flat_map(|l| l.commands.iter().map(|c| c.description.clone()))
                .collect()
        };
        assert_eq!(texts(&same), texts(&fresh));
    }

    #[test]
    fn shortcuts_of_submenus_run_and_the_most_specific_wins() {
        let mut all = list(&core(&state()), std::iter::empty(), "en");
        let f6 = KeyboardShortcut::new(Modifiers::COMMAND, Key::F6);
        let save_as =
            KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, SAVE_KEY.logical_key);
        let mut o = CommandOverrides::default();
        o.shortcuts.insert("file/file.import.0".into(), Some(f6));
        o.shortcuts.insert("file/file.exit".into(), Some(save_as));
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
    fn a_shift_letter_shortcut_types_nothing() {
        let shift_v = KeyboardShortcut::new(Modifiers::SHIFT, Key::V);
        let all = vec![Listed {
            id: "so2r".into(),
            title: "SO2R".into(),
            source: Source::Panel(0),
            group: MenuGroup::Device,
            commands: vec![Command::action("toggle_tx", "TX", "").shortcut(shift_v)],
        }];
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: Key::V,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::SHIFT,
        });
        input.events.push(egui::Event::Text("V".into()));
        ctx.begin_pass(input);
        assert_eq!(
            pressed(&ctx, &all).map(|(_, id)| id).as_deref(),
            Some("toggle_tx")
        );
        assert!(ctx.input(|i| i.events.is_empty()));
        ctx.end_pass().textures_delta.clear();
    }

    #[test]
    fn a_shift_letter_shortcut_keeps_text_typed_before_it() {
        let shift_v = KeyboardShortcut::new(Modifiers::SHIFT, Key::V);
        let all = vec![Listed {
            id: "so2r".into(),
            title: "SO2R".into(),
            source: Source::Panel(0),
            group: MenuGroup::Device,
            commands: vec![Command::action("toggle_tx", "TX", "").shortcut(shift_v)],
        }];
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Text("A".into()));
        input.events.push(egui::Event::Key {
            key: Key::V,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::SHIFT,
        });
        input.events.push(egui::Event::Text("V".into()));
        ctx.begin_pass(input);
        assert!(pressed(&ctx, &all).is_some());
        assert_eq!(
            ctx.input(|i| i.events.clone()),
            [egui::Event::Text("A".into())]
        );
        ctx.end_pass().textures_delta.clear();
    }

    #[test]
    fn core_providers_record_what_was_run() {
        let mut c = core(&state());
        c[0].run("file.save");
        assert_eq!(c[0].chosen.as_deref(), Some("file.save"));
        assert_eq!(c[0].group(), MenuGroup::File);
        assert_eq!(c[1].id(), "edit");
    }
}
