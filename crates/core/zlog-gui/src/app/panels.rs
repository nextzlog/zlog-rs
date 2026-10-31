use super::*;

pub struct PanelSlot {
    pub panel: Box<dyn zlog_ui::Panel>,
    pub open: bool,
    pub was_open: bool,
    pub title: String,
}

#[derive(Default)]
pub struct Places {
    pub(super) rects: std::collections::HashMap<String, egui::Rect>,
    pub(super) restored: std::collections::HashSet<String>,
}

impl PanelSlot {
    pub(super) fn window_title(&self, language: &str) -> String {
        if self.title.is_empty() {
            self.panel.title(language)
        } else {
            self.title.clone()
        }
    }
}

pub(super) fn tool_windows<'a>(panels: &'a [PanelSlot], language: &str) -> Vec<(&'a str, String)> {
    panels
        .iter()
        .filter(|s| s.open && s.panel.window_kind() == zlog_ui::WindowKind::Tool)
        .map(|s| (s.panel.id(), s.window_title(language)))
        .collect()
}

pub(super) fn hosted(
    host: Option<&dyn zlog_ui::chrome::WindowHost>,
    panel: &dyn zlog_ui::Panel,
) -> bool {
    panel.window_kind() == zlog_ui::WindowKind::Tool && host.is_some_and(|h| h.hosts(panel.id()))
}

pub fn stop_transmitting(panels: &mut [PanelSlot], view: &zlog_ui::View<'_>) {
    for slot in panels.iter_mut() {
        slot.panel.on_key(zlog_ui::EntryKey::Escape, view);
    }
    let actions: Vec<zlog_ui::EntryAction> = panels
        .iter_mut()
        .flat_map(|s| s.panel.take_actions())
        .collect();
    for action in device_first(actions) {
        if let zlog_ui::EntryAction::Transmit(on) = action {
            panels.iter_mut().any(|s| s.panel.transmit(on));
        }
    }
}

pub fn device_first(actions: Vec<zlog_ui::EntryAction>) -> Vec<zlog_ui::EntryAction> {
    let (mut first, rest): (Vec<_>, Vec<_>) = actions
        .into_iter()
        .partition(|a| matches!(a, zlog_ui::EntryAction::SelectRig(_)));
    first.extend(rest);
    first
}

pub fn panel_sections(panels: &[PanelSlot], settings: &mut Settings) -> bool {
    let mut changed = false;
    for slot in panels {
        let section = settings.section(&panel_section(slot.panel.id()));
        let mut fresh = section.clone();
        slot.panel.save(&mut fresh);
        if fresh != *section {
            *section = fresh;
            changed = true;
        }
    }
    changed
}

pub(super) fn section_infos(
    options: &Options,
    panels: &[PanelSlot],
    menu: &Option<Box<dyn zlog_ui::StartScreen>>,
    lang: &str,
) -> Vec<zlog_ui::SectionInfo> {
    let mut infos = options.section_infos(lang);
    for slot in panels {
        let name = panel_section(slot.panel.id());
        let kind = slot.panel.section_kind();
        match infos.iter_mut().find(|i| i.name == name) {
            Some(i) => i.kind = i.kind.max(kind),
            None => infos.push(zlog_ui::SectionInfo {
                name,
                title: slot.window_title(lang),
                kind,
            }),
        }
    }
    if let Some(m) = menu {
        for s in m.sections() {
            if !infos.iter().any(|i| i.name == *s) {
                infos.push(zlog_ui::SectionInfo {
                    name: s.to_string(),
                    title: format!("{} ({s})", tr("menu.zlog_menu")),
                    kind: zlog_ui::SectionKind::Personal,
                });
            }
        }
    }
    infos
}

pub(super) fn context_commands(all: &[commands::Listed]) -> main_window::ContextCommands {
    all.iter()
        .map(|l| {
            let list: Vec<_> = l.commands.iter().filter(|c| c.context).cloned().collect();
            (l.source, list)
        })
        .filter(|(_, list)| !list.is_empty())
        .collect()
}

pub fn panel_view<'a>(
    focus: Option<zlog_ui::EntryField>,
    editing: Option<zlog_core::QsoId>,
    selected: Option<usize>,
    session: &'a Session,
    spots: &'a [zlog_ui::Spot],
    rig: RigNow,
) -> zlog_ui::View<'a> {
    zlog_ui::View {
        log: session.log(),
        contest: session.rules(),
        sheet: session.sheet(),
        callsign: &session.entry.callsign,
        station: &session.station,
        number: &session.entry.nr_rcvd,
        memo: &session.entry.memo,
        rst_sent: &session.entry.rst_sent,
        power: session.entry.power,
        band: session.entry.band,
        mode: session.entry.mode,
        language: i18n::current(),
        spots,
        hz: rig.hz,
        rig: rig.number,
        cq: session.entry.cq,
        operator: &session.station.operator,
        period_hours: session.period_hours(),
        pc_name: &session.pc_name,
        log_path: session.path.to_str().unwrap_or(""),
        focus,
        editing: editing.and_then(|id| session.find(id).map(|i| &session.log().qsos[i])),
        selected: selected.and_then(|i| session.log().qsos.get(i)),
        period: session,
        tab: session.tab.clone(),
        changes: session.changes(),
    }
}

pub fn pick_callsign(entry: &mut crate::entry::Entry, callsign: String) {
    entry.callsign = callsign;
    entry.nr_rcvd.clear();
}

pub fn qso_items(panels: &[PanelSlot], q: &zlog_core::Qso) -> main_window::ExtensionItems {
    let lang = i18n::current();
    let items = panels.iter().enumerate().flat_map(|(i, s)| {
        let g = s.panel.menu();
        s.panel
            .qso_menu(lang, q)
            .into_iter()
            .map(move |it| (g, i, it))
    });
    crate::menus::grouped(items)
        .into_iter()
        .map(|(_, list)| list)
        .collect()
}

pub(super) fn open_dialogs(
    options: &Options,
    panels: &[PanelSlot],
    show_about: bool,
) -> Vec<(String, String)> {
    let lang = i18n::current();
    let mut all = Vec::new();
    if options.open {
        all.push(("options".into(), tr("options.title").into()));
    }
    for slot in panels {
        all.extend(slot.panel.dialog_windows(lang));
    }
    for slot in panels.iter().filter(|s| s.open) {
        all.push((slot.panel.id().to_string(), slot.window_title(lang)));
    }
    if show_about {
        all.push(("about".into(), tr("menu.help.about").into()));
    }
    all
}

pub(super) const WINDOWS: &str = "windows";

fn rect_text(r: egui::Rect) -> String {
    format!(
        "{:.0},{:.0},{:.0},{:.0}",
        r.min.x,
        r.min.y,
        r.width(),
        r.height()
    )
}

fn parse_rect(text: &str) -> Option<egui::Rect> {
    let v: Vec<f32> = text
        .split(',')
        .map(|n| n.trim().parse().ok())
        .collect::<Option<_>>()?;
    match v[..] {
        [x, y, w, h] if v.iter().all(|n| n.is_finite()) && w >= 40.0 && h >= 40.0 => Some(
            egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h)),
        ),
        _ => None,
    }
}

pub(super) fn windows_section(
    panels: &[PanelSlot],
    host: Option<&dyn zlog_ui::chrome::WindowHost>,
    places: &Places,
    settings: &mut Settings,
) -> bool {
    let section = settings.section(WINDOWS);
    let mut fresh = section.clone();
    let (mut docked, mut floating) = (Vec::new(), Vec::new());
    for slot in panels {
        let id = slot.panel.id();
        if let Some(&r) = places.rects.get(id) {
            fresh.insert(format!("{id}.rect"), rect_text(r));
        }
        if !slot.open || slot.panel.window_kind() != zlog_ui::WindowKind::Tool {
            continue;
        }
        match hosted(host, slot.panel.as_ref()) {
            true => docked.push(id),
            false => floating.push(id),
        }
    }
    fresh.insert("docked".into(), docked.join(","));
    fresh.insert("floating".into(), floating.join(","));
    let changed = fresh != *section;
    *section = fresh;
    changed
}

pub(super) fn restore_windows(
    panels: &mut [PanelSlot],
    host: Option<&dyn zlog_ui::chrome::WindowHost>,
    places: &mut Places,
    settings: &mut Settings,
) {
    let floating = settings.reopen_windows();
    let section = settings.section(WINDOWS);
    let list = |key: &str| -> Vec<String> {
        section
            .get(key)
            .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_default()
    };
    let (docked, open) = (list("docked"), list("floating"));
    for slot in panels {
        let id = slot.panel.id().to_string();
        let rect = section
            .get(&format!("{id}.rect"))
            .and_then(|t| parse_rect(t));
        if let Some(r) = rect {
            places.rects.insert(id.clone(), r);
        }
        if slot.panel.window_kind() != zlog_ui::WindowKind::Tool {
            continue;
        }
        let docks = hosted(host, slot.panel.as_ref());
        if (docks && docked.contains(&id)) || (floating && !docks && open.contains(&id)) {
            slot.open = true;
            places.restored.insert(id.clone());
        }
    }
}

pub(super) fn load_panel(panel: &mut dyn zlog_ui::Panel, settings: &mut Settings) {
    let own = settings.section(&panel_section(panel.id())).clone();
    panel.load_with(&own, settings.sections());
}

pub(super) fn panel_affected(panel: &dyn zlog_ui::Panel, changed: &[String]) -> bool {
    let own = panel_section(panel.id());
    changed
        .iter()
        .any(|c| *c == own || panel.reads().contains(c))
}

pub(super) fn panel_section(id: &str) -> String {
    format!("panel.{id}")
}

pub(super) fn rig_selected(n: u8) -> Status {
    Status::info(i18n::trf("status.rig_active", &[&n]))
}

#[derive(Debug, Clone, Copy)]
pub struct RigNow {
    pub(super) hz: Option<u64>,
    pub(super) number: u8,
}

impl Default for RigNow {
    fn default() -> Self {
        RigNow {
            hz: None,
            number: 1,
        }
    }
}

pub(super) fn rig_now(panels: &[PanelSlot], fallback: u8) -> RigNow {
    match rig_of(panels) {
        Some(r) => RigNow {
            hz: r.hz(),
            number: r.number(),
        },
        None => RigNow {
            hz: None,
            number: fallback,
        },
    }
}

pub(super) fn rig_of(panels: &[PanelSlot]) -> Option<&dyn zlog_ui::rig::RigControl> {
    panels
        .iter()
        .find_map(|s| zlog_ui::rig_of(s.panel.as_ref()))
}

pub(super) fn rig_of_mut(panels: &mut [PanelSlot]) -> Option<&mut dyn zlog_ui::rig::RigControl> {
    panels
        .iter_mut()
        .find_map(|s| zlog_ui::rig_of_mut(s.panel.as_mut()))
}

pub(super) fn load_rig_sections(
    panels: &mut [PanelSlot],
    settings: &mut Settings,
    changed: &[String],
) {
    if let Some(rig) = rig_of_mut(panels) {
        let names = rig.sections();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        rig.load_sections(&settings.sections_named(&names), changed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Reader {
        own: Option<String>,
        other: Option<String>,
    }

    impl zlog_ui::Panel for Reader {
        fn id(&self) -> &str {
            "reader"
        }

        fn title(&self, _: &str) -> String {
            String::new()
        }

        fn ui(&mut self, _: &mut egui::Ui, _: &zlog_ui::View) {}

        fn reads(&self) -> Vec<String> {
            vec!["panel.other".into()]
        }

        fn load_with(&mut self, section: &zlog_ui::Section, others: &zlog_ui::Sections) {
            self.own = section.get("k").cloned();
            self.other = others.get("panel.other").and_then(|s| s.get("k")).cloned();
        }
    }

    #[test]
    fn a_panel_reads_other_sections_and_follows_their_changes() {
        let mut settings = Settings::default();
        settings
            .section("panel.reader")
            .insert("k".into(), "mine".into());
        settings
            .section("panel.other")
            .insert("k".into(), "T".into());
        let mut p = Reader::default();
        load_panel(&mut p, &mut settings);
        assert_eq!(
            (p.own.as_deref(), p.other.as_deref()),
            (Some("mine"), Some("T"))
        );
        assert!(panel_affected(&p, &["panel.other".into()]));
        assert!(panel_affected(&p, &["panel.reader".into()]));
        assert!(!panel_affected(&p, &["panel.third".into()]));
    }
}
