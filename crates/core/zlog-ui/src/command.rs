use crate::{MenuGroup, MenuSection, Panel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub id: String,
    pub label: String,
    pub menu_label: Option<String>,
    pub description: String,
    pub icon: Option<&'static str>,
    pub shortcut: Option<egui::KeyboardShortcut>,
    pub key_hint: Option<egui::KeyboardShortcut>,
    pub row_key: Option<egui::KeyboardShortcut>,
    pub state: Option<String>,
    pub disabled_reason: Option<String>,
    pub enabled: bool,
    pub selected: bool,
    pub checkable: bool,
    pub radio: bool,
    pub menu: bool,
    pub toolbar: bool,
    pub context: bool,
    pub toolbar_default: bool,
    pub toolbar_key: Option<String>,
    pub section: MenuSection,
    pub weight: i32,
    pub children: Vec<Command>,
    pub role: Option<&'static str>,
    pub opens_window: bool,
    pub dialog: bool,
}

impl Command {
    pub const WINDOW: &'static str = "window";

    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Command {
            id: id.into(),
            label: label.into(),
            menu_label: None,
            description: description.into(),
            icon: None,
            shortcut: None,
            key_hint: None,
            row_key: None,
            state: None,
            disabled_reason: None,
            enabled: true,
            selected: false,
            checkable: false,
            radio: false,
            menu: true,
            toolbar: false,
            context: false,
            toolbar_default: true,
            toolbar_key: None,
            section: MenuSection::Other,
            weight: 0,
            children: Vec::new(),
            role: None,
            opens_window: false,
            dialog: false,
        }
    }

    pub fn window(label: impl Into<String>, tooltip: impl Into<String>) -> Self {
        let mut c = Command::new(Command::WINDOW, label, tooltip);
        c.toolbar = true;
        c.checkable = true;
        c
    }

    pub fn action(
        id: impl Into<String>,
        label: impl Into<String>,
        tooltip: impl Into<String>,
    ) -> Self {
        let mut c = Command::new(id, label, tooltip);
        c.menu = false;
        c.toolbar = true;
        c
    }

    pub fn is_window(&self) -> bool {
        self.id == Command::WINDOW
    }

    pub fn role(mut self, role: &'static str) -> Self {
        self.role = Some(role);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn icon(mut self, glyph: &'static str) -> Self {
        self.icon = Some(glyph);
        self
    }

    pub fn shortcut(mut self, shortcut: egui::KeyboardShortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn key_hint(mut self, key: egui::KeyboardShortcut) -> Self {
        self.key_hint = Some(key);
        self
    }

    pub fn row_key(mut self, key: egui::KeyboardShortcut) -> Self {
        self.row_key = Some(key);
        self
    }

    pub fn section(mut self, section: MenuSection, weight: i32) -> Self {
        self.section = section;
        self.weight = weight;
        self
    }

    pub fn menu_key(&self) -> Option<&egui::KeyboardShortcut> {
        self.shortcut
            .as_ref()
            .or(self.key_hint.as_ref())
            .or(self.row_key.as_ref())
    }

    pub fn state(mut self, state: impl Into<String>) -> Self {
        self.state = Some(state.into());
        self
    }

    pub fn disabled_because(mut self, reason: impl Into<String>) -> Self {
        self.enabled = false;
        self.disabled_reason = Some(reason.into());
        self
    }

    pub fn on_toolbar(mut self, shown: bool) -> Self {
        self.toolbar = true;
        self.toolbar_default = shown;
        self
    }

    pub fn opens_window(mut self) -> Self {
        self.opens_window = true;
        self
    }

    pub fn in_context(mut self) -> Self {
        self.context = true;
        self
    }

    pub fn toolbar_key(mut self, key: impl Into<String>) -> Self {
        self.toolbar_key = Some(key.into());
        self
    }

    pub fn menu_label(mut self, label: impl Into<String>) -> Self {
        self.menu_label = Some(label.into());
        self
    }

    pub fn children(mut self, children: Vec<Command>) -> Self {
        self.children = children;
        self
    }

    pub fn dialog(mut self) -> Self {
        self.dialog = true;
        self
    }

    pub fn menu_name(&self) -> &str {
        self.menu_label.as_deref().unwrap_or(&self.label)
    }

    pub fn menu_text(&self) -> std::borrow::Cow<'_, str> {
        with_ellipsis(self.menu_name(), self.dialog)
    }

    pub fn rename(&mut self, name: &str) {
        let name = name.trim();
        if !name.is_empty() {
            self.menu_label = Some(name.to_string());
        }
    }

    pub fn describe(&mut self, text: &str) {
        let text = text.trim();
        if !text.is_empty() {
            self.description = text.to_string();
        }
    }

    pub fn rename_short(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.menu_label.get_or_insert_with(|| self.label.clone());
        self.label = name.to_string();
    }

    pub fn window_title(&self) -> String {
        window_title(self.menu_name())
    }

    pub fn full_tooltip(&self, shortcut: Option<&str>) -> String {
        let mut text = self.description.clone();
        if let Some(k) = shortcut.filter(|k| !k.is_empty()) {
            text.push_str(&format!(" ({k})"));
        }
        let reason = self.disabled_reason.as_ref().filter(|_| !self.enabled);
        for line in self.state.iter().chain(reason) {
            text.push('\n');
            text.push_str(line);
        }
        text
    }
}

pub fn with_ellipsis(name: &str, dialog: bool) -> std::borrow::Cow<'_, str> {
    match dialog {
        true => format!("{name}…").into(),
        false => name.into(),
    }
}

pub fn window_title(text: &str) -> String {
    let mut t = text.trim();
    t = t.trim_end_matches("...").trim_end_matches('…').trim_end();
    if let Some(rest) = t.strip_suffix("を表示") {
        t = rest.trim_end();
    }
    match t.strip_prefix("Show ") {
        Some(rest) if !rest.is_empty() => {
            let mut c = rest.chars();
            let first = c.next().map(|f| f.to_uppercase().collect::<String>());
            format!("{}{}", first.unwrap_or_default(), c.as_str())
        }
        _ => t.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleCommand {
    pub role: &'static str,
    pub label: String,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
}

pub fn role_commands<'a>(commands: impl IntoIterator<Item = &'a Command>) -> Vec<RoleCommand> {
    fn add(c: &Command, out: &mut Vec<RoleCommand>) {
        if let Some(role) = c.role {
            out.push(RoleCommand {
                role,
                label: c.menu_text().to_string(),
                enabled: c.enabled,
                disabled_reason: c.disabled_reason.clone().filter(|_| !c.enabled),
            });
        }
        for child in &c.children {
            add(child, out);
        }
    }
    let mut out = Vec::new();
    for c in commands {
        add(c, &mut out);
    }
    out
}

pub trait Commands {
    fn id(&self) -> &str;

    fn title(&self, language: &str) -> String;

    fn group(&self) -> MenuGroup;

    fn commands(&self, language: &str) -> Vec<Command>;

    fn run(&mut self, id: &str);
}

pub struct PanelCommands<'a> {
    pub panel: &'a mut dyn Panel,
    pub open: &'a mut bool,
}

pub fn panel_commands(panel: &dyn Panel, open: bool, language: &str) -> Vec<Command> {
    let mut list = panel.commands(language);
    let title = panel.title(language);
    if panel.window_kind().has_window() && !list.iter().any(Command::is_window) {
        let mut w = Command::window(title.clone(), title.clone());
        w.toolbar = false;
        list.insert(0, w);
    }
    let help = panel.menu().is_app_menu();
    let dialog = help || panel.window_kind() == crate::WindowKind::Dialog;
    for c in &mut list {
        c.toolbar &= !help || c.toolbar_key.is_some();
        if c.toolbar_key.is_none() {
            c.toolbar_default = panel.toolbar_default_shown();
        } else {
            c.toolbar_default &= panel.toolbar_default_shown();
        }
        if c.section == MenuSection::Other {
            (c.section, c.weight) = panel.section();
        }
        if c.is_window() {
            c.selected = open;
            c.menu_label.get_or_insert_with(|| title.clone());
            c.checkable = !dialog;
            if panel.window_kind() == crate::WindowKind::Dialog {
                c.toolbar = false;
                c.dialog = true;
            }
        }
    }
    list
}

impl Commands for PanelCommands<'_> {
    fn id(&self) -> &str {
        self.panel.id()
    }

    fn title(&self, language: &str) -> String {
        self.panel.title(language)
    }

    fn group(&self) -> MenuGroup {
        self.panel.menu()
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        panel_commands(self.panel, *self.open, language)
    }

    fn run(&mut self, id: &str) {
        if id == Command::WINDOW {
            let dialog = self.panel.window_kind() == crate::WindowKind::Dialog;
            *self.open = dialog || self.panel.menu().is_app_menu() || !*self.open;
        } else {
            self.panel.run_command(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_is_none_until_tagged() {
        assert_eq!(Command::new("x", "X", "").role, None);
        let c = Command::action("cq", "CQ", "").role("cq");
        assert_eq!(c.role, Some("cq"));
    }

    #[test]
    fn role_commands_take_tagged_ones_and_children() {
        let sub = Command::new("x", "X", "").role("sub");
        let list = [
            Command::new("a", "A", ""),
            Command::new("b", "B", "")
                .role("score")
                .disabled_because("No log"),
            Command::new("c", "C", "").children(vec![sub]),
        ];
        let found = role_commands(&list);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].role, "score");
        assert!(!found[0].enabled);
        assert_eq!(found[0].disabled_reason.as_deref(), Some("No log"));
        assert_eq!(found[1].label, "X");
    }

    #[test]
    fn tooltip_adds_shortcut_state_and_the_reason_only_while_disabled() {
        let b = Command::action("cq", "CQ", "Repeat F1").state("Stopped");
        assert_eq!(b.full_tooltip(None), "Repeat F1\nStopped");
        assert_eq!(b.full_tooltip(Some("F1")), "Repeat F1 (F1)\nStopped");
        let b = b.disabled_because("Connect a keyer first");
        assert!(!b.enabled);
        assert_eq!(
            b.full_tooltip(None),
            "Repeat F1\nStopped\nConnect a keyer first"
        );
        let b = b.enabled(true);
        assert_eq!(b.full_tooltip(None), "Repeat F1\nStopped");
        assert_eq!(
            Command::window("A", "B").icon(crate::icon::TROPHY).icon,
            Some(crate::icon::TROPHY)
        );
    }

    #[test]
    fn renaming_keeps_the_description() {
        let mut c = Command::window("Partial", "Calls containing it")
            .menu_label("コンテスト内の部分一致を表示");
        assert_eq!(c.window_title(), "コンテスト内の部分一致");
        c.rename("  ");
        assert_eq!(c.label, "Partial");
        c.rename("Pc");
        assert_eq!((c.label.as_str(), c.menu_name()), ("Partial", "Pc"));
        assert_eq!(
            c.description, "Calls containing it",
            "the name is not in it"
        );
        c.describe("Mine");
        assert_eq!(c.full_tooltip(None), "Mine");
        assert_eq!(c.window_title(), "Pc");
        c.rename_short("P");
        assert_eq!((c.label.as_str(), c.menu_name()), ("P", "Pc"));
        let mut s = Command::new("save", "Save", "Write the log");
        s.rename_short("Sv");
        assert_eq!((s.label.as_str(), s.menu_name()), ("Sv", "Save"));
        assert_eq!(window_title("Show partial matches"), "Partial matches");
        assert_eq!(window_title("Options..."), "Options");
        let c = Command::new("o", "Options", "");
        assert_eq!(
            (c.menu_text(), c.clone().dialog().menu_text()),
            ("Options".into(), "Options…".into())
        );
        assert_eq!(window_title("Show"), "Show");
    }

    #[test]
    fn places_by_constructor() {
        let w = Command::window("A", "B");
        assert!(w.is_window() && w.menu && w.toolbar && w.checkable);
        let a = Command::action("cq", "CQ", "CQ");
        assert!(!a.menu && a.toolbar && !a.is_window());
        let n = Command::new("save", "Save", "Save").on_toolbar(false);
        assert!(n.menu && n.toolbar && !n.toolbar_default);
        assert_eq!(n.menu_text(), "Save");
    }

    struct P(MenuGroup);

    impl Panel for P {
        fn id(&self) -> &str {
            "p"
        }
        fn title(&self, _language: &str) -> String {
            "Panel".into()
        }
        fn menu(&self) -> MenuGroup {
            self.0
        }
        fn ui(&mut self, _ui: &mut egui::Ui, _view: &crate::View<'_>) {}
    }

    #[test]
    fn a_panel_without_buttons_gets_a_menu_only_window_toggle() {
        let mut p = P(MenuGroup::Tools);
        let mut open = false;
        let mut c = PanelCommands {
            panel: &mut p,
            open: &mut open,
        };
        let list = c.commands("en");
        assert_eq!(list.len(), 1);
        assert!(list[0].is_window() && list[0].menu && !list[0].toolbar);
        assert_eq!(list[0].menu_text(), "Panel");
        c.run(Command::WINDOW);
        c.run(Command::WINDOW);
        assert!(!open);
        let mut h = P(MenuGroup::Help);
        let mut c = PanelCommands {
            panel: &mut h,
            open: &mut open,
        };
        assert!(!c.commands("en")[0].checkable);
        c.run(Command::WINDOW);
        c.run(Command::WINDOW);
        assert!(open, "Help items open");
        let mut s = P(MenuGroup::Settings);
        let mut open = false;
        let mut c = PanelCommands {
            panel: &mut s,
            open: &mut open,
        };
        let list = c.commands("en");
        assert!(!list[0].checkable && !list[0].toolbar);
        c.run(Command::WINDOW);
        c.run(Command::WINDOW);
        assert!(open, "Settings items open");
        assert!(!MenuGroup::ALL.contains(&MenuGroup::Settings));
    }

    #[test]
    fn keyed_menu_commands_keep_their_key() {
        struct C(MenuGroup);
        impl Panel for C {
            fn id(&self) -> &str {
                "c"
            }
            fn title(&self, _language: &str) -> String {
                "C".into()
            }
            fn menu(&self) -> MenuGroup {
                self.0
            }
            fn toolbar_default_shown(&self) -> bool {
                true
            }
            fn commands(&self, _language: &str) -> Vec<Command> {
                vec![
                    Command::window("C", "C window"),
                    Command::new("x", "X", "X action").toolbar_key("c.x"),
                ]
            }
            fn ui(&mut self, _ui: &mut egui::Ui, _view: &crate::View<'_>) {}
        }
        let list = panel_commands(&C(MenuGroup::Tools), false, "en");
        assert!(list[0].toolbar && list[0].toolbar_default);
        assert!(!list[1].toolbar && list[1].menu);
        assert_eq!(list[1].toolbar_key.as_deref(), Some("c.x"));
        let list = panel_commands(&C(MenuGroup::Settings), false, "en");
        assert!(!list[0].toolbar && !list[1].toolbar);
        assert_eq!(list[1].toolbar_key.as_deref(), Some("c.x"));
    }

    struct D;

    impl Panel for D {
        fn id(&self) -> &str {
            "d"
        }
        fn title(&self, _language: &str) -> String {
            "Task".into()
        }
        fn window_kind(&self) -> crate::WindowKind {
            crate::WindowKind::Dialog
        }
        fn ui(&mut self, _ui: &mut egui::Ui, _view: &crate::View<'_>) {}
    }

    #[test]
    fn a_dialog_item_opens_and_is_not_checkable() {
        let mut p = D;
        let mut open = false;
        let mut c = PanelCommands {
            panel: &mut p,
            open: &mut open,
        };
        let list = c.commands("en");
        assert!(!list[0].checkable);
        assert_eq!(list[0].menu_text(), "Task…");
        assert_eq!(list[0].menu_name(), "Task");
        assert_eq!(list[0].window_title(), "Task");
        c.run(Command::WINDOW);
        c.run(Command::WINDOW);
        assert!(open, "running it twice keeps it open");
        let mut t = P(MenuGroup::Tools);
        assert!(panel_commands(&t, false, "en")[0].checkable);
        let mut open = false;
        let mut c = PanelCommands {
            panel: &mut t,
            open: &mut open,
        };
        c.run(Command::WINDOW);
        assert!(open);
    }

    struct Windowless;

    impl Panel for Windowless {
        fn id(&self) -> &str {
            "w"
        }
        fn title(&self, _language: &str) -> String {
            "Status only".into()
        }
        fn window_kind(&self) -> crate::WindowKind {
            crate::WindowKind::None
        }
        fn ui(&mut self, _ui: &mut egui::Ui, _view: &crate::View<'_>) {}
    }

    #[test]
    fn a_panel_without_a_window_gets_no_window_toggle() {
        assert!(panel_commands(&Windowless, false, "en").is_empty());
    }
}
