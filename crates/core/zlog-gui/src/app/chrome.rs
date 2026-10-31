use zlog_core::{Band, Mode, Power};
use zlog_ui::chrome::{Menu, MenuItem, Pick, StatusLine};
use zlog_ui::{Command, MenuGroup, MenuSection};

use super::{qso_items, PanelSlot};
use crate::commands::{self, Listed, Source};
use crate::i18n::{self, tr, trf};
use crate::main_window::{ExtensionItems, GridAction};
use crate::options::Options;
use crate::session::Session;
use zlog_ui::EntryStyle;

pub(crate) const HOST: usize = usize::MAX;

pub(crate) const POWERS: [Power; 4] = [Power::P001, Power::P002, Power::P005, Power::P010];

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HostItem {
    Options,
    Wizard,
    Language(String),
    Front,
    OnTop,
    CloseAll,
    Focus(usize),
    About,
    Manual,
    Qso(GridAction),
    File(usize, String),
}

impl HostItem {
    pub fn of(id: &str, session: &Session) -> Option<HostItem> {
        let (kind, rest) = id.split_once(':').unwrap_or((id, ""));
        let n = || rest.parse::<usize>().ok();
        Some(match kind {
            "options" => HostItem::Options,
            "wizard" => HostItem::Wizard,
            "language" => HostItem::Language(rest.into()),
            "window.front" => HostItem::Front,
            "window.on_top" => HostItem::OnTop,
            "window.close_all" => HostItem::CloseAll,
            "window.focus" => HostItem::Focus(n()?),
            "help.about" => HostItem::About,
            "help.manual" => HostItem::Manual,
            "qso.band" => HostItem::Qso(GridAction::Band(*bands(session).get(n()?)?)),
            "qso.mode" => HostItem::Qso(GridAction::Mode(*Mode::ALL.get(n()?)?)),
            "qso.power" => HostItem::Qso(GridAction::Power(*POWERS.get(n()?)?)),
            "qso.ext" => {
                let (panel, item) = rest.split_once(':')?;
                HostItem::Qso(GridAction::Extension(panel.parse().ok()?, item.into()))
            }
            "file" => {
                let (i, item) = rest.split_once(':')?;
                HostItem::File(i.parse().ok()?, item.into())
            }
            _ => return None,
        })
    }
}

pub fn bands(s: &Session) -> Vec<Band> {
    let (lo, hi) = s.rules().band_range();
    let warc = s.rules().uses_warc();
    Band::range(lo, hi)
        .filter(|b| warc || !b.is_warc())
        .collect()
}

fn host(id: impl Into<String>, label: impl Into<String>) -> MenuItem {
    MenuItem::Command(HOST, Command::new(id, label, ""))
}

pub(crate) fn index_of(all: &[Listed], source: Source) -> usize {
    all.iter().position(|l| l.source == source).unwrap_or(HOST)
}

pub fn listed(all: &[Listed], pick: &Pick) -> Option<(Source, String)> {
    all.get(pick.source).map(|l| (l.source, pick.id.clone()))
}

fn items_of(
    all: &[Listed],
    sections: &commands::Sections,
    mut extra: impl FnMut(MenuSection, &mut Vec<MenuItem>),
) -> Vec<MenuItem> {
    let mut items = Vec::new();
    for (n, (section, list)) in sections.iter().enumerate() {
        if n > 0 {
            items.push(MenuItem::Separator);
        }
        for (source, c) in list {
            items.push(MenuItem::Command(index_of(all, *source), c.clone()));
        }
        extra(*section, &mut items);
    }
    items
}

fn by_panel(all: &[Listed], sections: &commands::Sections) -> Vec<MenuItem> {
    let flat: Vec<&(Source, Command)> = sections.iter().flat_map(|(_, l)| l).collect();
    let count = |s: Source| flat.iter().filter(|(t, _)| *t == s).count();
    let mut items = Vec::new();
    let mut seen: Vec<Source> = Vec::new();
    let mut last = None;
    for (n, (_, list)) in sections.iter().enumerate() {
        for (source, c) in list {
            if seen.contains(source) {
                continue;
            }
            if n > 0 && last != Some(n) && !items.is_empty() {
                items.push(MenuItem::Separator);
            }
            last = Some(n);
            let i = index_of(all, *source);
            if count(*source) < 2 {
                items.push(MenuItem::Command(i, c.clone()));
                continue;
            }
            seen.push(*source);
            let children = flat
                .iter()
                .filter(|(t, _)| t == source)
                .map(|(_, c)| {
                    let mut c = c.clone();
                    if c.is_window() {
                        c.menu_label = Some(tr("menu.device.show").into());
                    }
                    c
                })
                .collect();
            let l = &all[i];
            let parent = Command::new(format!("panel.{}", l.id), &l.title, "").children(children);
            items.push(MenuItem::Command(i, parent));
        }
    }
    items
}

pub(crate) struct MenuState<'a> {
    pub session: &'a Session,
    pub selected: Option<usize>,
    pub panels: &'a [PanelSlot],
    pub files: Vec<Vec<Command>>,
    pub dialogs: &'a [(String, String)],
    pub(crate) on_top: bool,
}

pub fn menus(all: &[Listed], st: &MenuState<'_>) -> Vec<Menu> {
    let lang = i18n::current();
    let mut menus = vec![Menu {
        title: tr("menu.file").into(),
        items: items_of(
            all,
            &commands::menu_sections(all, MenuGroup::File),
            |_, _| {},
        ),
    }];
    let edit = super::edit_sections(all);
    menus.push(Menu {
        title: tr("menu.edit").into(),
        items: items_of(all, &edit, |section, items| {
            if section == MenuSection::Qso {
                qso_submenus(st, items);
            }
        }),
    });
    let mut groups: Vec<MenuGroup> = MenuGroup::ALL
        .into_iter()
        .filter(|g| commands::of_group(all, *g).next().is_some())
        .collect();
    groups.sort_by_key(|g| crate::menus::position(*g));
    for group in groups {
        let mut items: Vec<MenuItem> = st
            .panels
            .iter()
            .filter(|s| s.panel.menu() == group)
            .filter_map(|s| zlog_ui::menu_note_of(s.panel.as_ref(), lang))
            .map(MenuItem::Note)
            .collect();
        let sections = commands::menu_sections(all, group);
        if group == MenuGroup::Device {
            items.extend(by_panel(all, &sections));
        } else {
            items.extend(items_of(all, &sections, |_, _| {}));
        }
        menus.push(Menu {
            title: tr(crate::menus::group_key(group)).into(),
            items,
        });
    }
    menus.push(settings_menu(all, st));
    menus.push(window_menu(all, st));
    let mut help = vec![
        host("help.about", tr("menu.help.about")),
        host("help.manual", tr("menu.help.manual")),
    ];
    let sections = commands::menu_sections(all, MenuGroup::Help);
    if !sections.is_empty() {
        help.push(MenuItem::Separator);
    }
    help.extend(items_of(all, &sections, |_, _| {}));
    menus.push(Menu {
        title: tr("menu.help").into(),
        items: help,
    });
    menus
}

fn language_menu() -> MenuItem {
    let children = i18n::LANGUAGES
        .iter()
        .map(|l| {
            let mut c = Command::new(format!("language:{}", l.code), l.name, "");
            c.radio = true;
            c.selected = i18n::current() == l.code;
            c
        })
        .collect();
    MenuItem::Command(
        HOST,
        Command::new("language", tr("menu.language"), "").children(children),
    )
}

pub(crate) fn start_menus() -> Vec<Menu> {
    let MenuItem::Command(_, c) = language_menu() else {
        return Vec::new();
    };
    vec![Menu {
        title: c.label,
        items: c
            .children
            .into_iter()
            .map(|c| MenuItem::Command(HOST, c))
            .collect(),
    }]
}

fn settings_menu(all: &[Listed], st: &MenuState<'_>) -> Menu {
    let mut items = vec![
        MenuItem::Command(
            HOST,
            Command::new("options", tr("menu.settings.options"), "").dialog(),
        ),
        MenuItem::Command(
            HOST,
            Command::new(
                "wizard",
                tr("menu.settings.wizard"),
                tr("menu.settings.wizard.tip"),
            )
            .dialog(),
        ),
        MenuItem::Separator,
        language_menu(),
    ];
    if !st.files.is_empty() {
        items.push(MenuItem::Separator);
    }
    for (i, list) in st.files.iter().enumerate() {
        for c in list {
            let mut c = c.clone();
            c.id = format!("file:{i}:{}", c.id);
            items.push(MenuItem::Command(HOST, c));
        }
    }
    let sections = commands::menu_sections(all, MenuGroup::Settings);
    if !sections.is_empty() {
        items.push(MenuItem::Separator);
    }
    items.extend(items_of(all, &sections, |_, _| {}));
    Menu {
        title: tr("menu.settings").into(),
        items,
    }
}

fn window_menu(all: &[Listed], st: &MenuState<'_>) -> Menu {
    let any = !st.dialogs.is_empty();
    let front = Command::new("window.front", tr("menu.window.front"), "").enabled(any);
    let mut on_top = Command::new("window.on_top", tr("menu.window.on_top"), "");
    on_top.checkable = true;
    on_top.selected = st.on_top;
    let close = Command::new("window.close_all", tr("menu.window.close_all"), "").enabled(any);
    let mut items = vec![
        MenuItem::Command(HOST, front),
        MenuItem::Command(HOST, on_top),
        MenuItem::Command(HOST, close),
    ];
    items.extend(items_of(
        all,
        &commands::menu_sections(all, MenuGroup::Window),
        |_, _| {},
    ));
    if any {
        items.push(MenuItem::Separator);
    }
    for (n, (_, title)) in st.dialogs.iter().enumerate() {
        items.push(host(format!("window.focus:{n}"), title.clone()));
    }
    Menu {
        title: tr("menu.window").into(),
        items,
    }
}

fn qso_submenus(st: &MenuState<'_>, items: &mut Vec<MenuItem>) {
    let selected = st.selected.and_then(|i| st.session.log().qsos.get(i));
    let extra: ExtensionItems = selected
        .map(|q| qso_items(st.panels, q))
        .unwrap_or_default();
    let on = selected.is_some();
    let submenu = |id: &str, key: &str, labels: Vec<String>| {
        let children = labels
            .into_iter()
            .enumerate()
            .map(|(i, l)| Command::new(format!("{id}:{i}"), l, ""))
            .collect();
        let c = Command::new(id, tr(key), "").children(children).enabled(on);
        MenuItem::Command(HOST, c)
    };
    let bands = bands(st.session).iter().map(|b| b.label().into()).collect();
    items.push(submenu("qso.band", "grid.band", bands));
    let modes = Mode::ALL.iter().map(|m| m.label().into()).collect();
    items.push(submenu("qso.mode", "grid.mode", modes));
    let powers = POWERS
        .iter()
        .map(|p| p.code().map(String::from).unwrap_or_default())
        .collect();
    items.push(submenu("qso.power", "grid.power", powers));
    for group in extra {
        items.push(MenuItem::Separator);
        for (panel, it) in group {
            let mut c = Command::new(format!("qso.ext:{panel}:{}", it.id), it.label, "")
                .enabled(it.enabled && on);
            c.dialog = it.dialog;
            items.push(MenuItem::Command(HOST, c));
        }
    }
    if !on {
        items.push(MenuItem::Note(tr("menu.edit.select_row").into()));
    }
}

pub fn toolbar<'a>(
    all: &'a [Listed],
    settings: &mut crate::settings::Settings,
) -> Vec<Vec<(usize, &'a Command)>> {
    commands::toolbar_buttons(all, settings)
        .into_iter()
        .map(|g| g.into_iter().map(|(s, c)| (index_of(all, s), c)).collect())
        .collect()
}

pub(crate) fn connections(
    panels: &[super::PanelSlot],
    lang: &str,
) -> Vec<(usize, zlog_ui::connection::Connection)> {
    panels
        .iter()
        .enumerate()
        .flat_map(|(i, s)| {
            zlog_ui::connections_of(s.panel.as_ref(), lang)
                .into_iter()
                .map(move |c| (i, c))
        })
        .collect()
}

pub(crate) fn bar_with_connections(
    ui: &mut egui::Ui,
    connections: &[(usize, zlog_ui::connection::Connection)],
    lang: &str,
    bar: impl FnOnce(&mut egui::Ui),
) -> Option<(usize, zlog_ui::connection::ConnectionPick)> {
    if connections.is_empty() {
        bar(ui);
        return None;
    }
    let collapsed = zlog_ui::connection::collapsed(ui.available_width(), connections.len());
    let width = zlog_ui::connection::width(connections.len(), collapsed);
    ui.horizontal_top(|ui| {
        let rest = (ui.available_width() - width).max(0.0);
        let height = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            egui::vec2(rest, height),
            egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
            bar,
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            zlog_ui::connection::indicator(ui, connections, lang, collapsed)
        })
        .inner
    })
    .inner
}

pub(crate) fn status_line(s: &Session, items: &[zlog_ui::StatusItem]) -> StatusLine {
    StatusLine {
        message: zlog_ui::StatusItem::new(&s.status.text).warning(s.status.warning),
        items: items.to_vec(),
        cq: s.entry.cq,
        cq_text: tr(if s.entry.cq { "status.cq" } else { "status.sp" }).into(),
        count: trf("status.qso_count", &[&s.log().qsos.len()]),
        contest: s.rules().name().into(),
    }
}

pub(crate) fn options_commands(
    options: &mut Options,
    toolbar: &zlog_ui::toolbar::Rows,
    all: &[commands::Listed],
    overrides: &zlog_ui::keys::CommandOverrides,
    entry: Option<&dyn EntryStyle>,
    lang: &str,
) {
    let mut named: Vec<commands::Listed> = all.to_vec();
    commands::apply_shortcuts(&mut named, overrides);
    *toolbar.borrow_mut() = commands::toolbar_rows(&named, &[]);
    let entry_keys = entry.map(|e| e.reserved_keys()).unwrap_or_default();
    let (infos, reserved) = commands::command_infos(all, &entry_keys, lang);
    options.set_commands(&infos, &reserved);
}

pub(crate) fn command_of_role(all: &[commands::Listed], role: &str) -> Option<(Source, String)> {
    pub fn find(list: &[zlog_ui::Command], role: &str) -> Option<String> {
        list.iter().find_map(|c| {
            if c.role == Some(role) && c.enabled {
                Some(c.id.clone())
            } else {
                find(&c.children, role)
            }
        })
    }
    all.iter()
        .find_map(|l| find(&l.commands, role).map(|id| (l.source, id)))
}

pub(crate) fn edit_sections(all: &[commands::Listed]) -> commands::Sections {
    let edit = zlog_ui::MenuGroup::Edit;
    let mut items: Vec<(Source, zlog_ui::Command)> = Vec::new();
    for l in all {
        for c in l.commands.iter().filter(|c| c.menu) {
            if c.context {
                let mut c = c.clone();
                c.section = zlog_ui::MenuSection::Qso;
                items.push((l.source, c));
            } else if l.group == edit {
                items.push((l.source, c.clone()));
            }
        }
    }
    let mut sections = commands::sectioned(items);
    if !sections
        .iter()
        .any(|(s, _)| *s == zlog_ui::MenuSection::Qso)
    {
        sections.push((zlog_ui::MenuSection::Qso, Vec::new()));
        sections.sort_by_key(|(s, _)| *s);
    }
    sections
}
