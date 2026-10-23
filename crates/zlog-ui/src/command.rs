//! Commands: what the operator can invoke from the menu bar, the toolbar or the
//! right-click menu of a QSO, described once.
//!
//! The application's own actions (open, save, import, connecting the rig) and the
//! extensions' windows and actions are all [`Command`]s handed out by a
//! [`Commands`] provider; the menu bar, the toolbar and Settings > Toolbar are
//! built from the same list, so a command has one label, one tooltip, one icon
//! and one shortcut wherever it appears.

use crate::{MenuGroup, MenuSection, Panel};

/// One thing the operator can invoke, in the language asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// Passed back to [`Commands::run`]; unique within its provider. The window
    /// toggle of a panel is [`Command::WINDOW`].
    pub id: String,
    /// Short text: of the menu item, and of the toolbar button.
    pub label: String,
    /// Text of the menu item when it differs from the (short) toolbar label: a
    /// panel's window button says "Score", its menu item the window's title.
    pub menu_label: Option<String>,
    /// Longer text shown when the pointer rests on the toolbar button: the full
    /// name and what it does ("Partial check — callsigns in the log containing
    /// what you type"). The application adds the shortcut.
    pub tooltip: String,
    /// A glyph of [`crate::icon`] drawn on the toolbar button.
    pub icon: Option<&'static str>,
    /// A key the application runs the command with, whatever has the keyboard
    /// (Ctrl+S); also shown in the menu and the tooltip.
    pub shortcut: Option<egui::KeyboardShortcut>,
    /// A key shown next to the menu item only, because the widget with the
    /// keyboard handles it (Enter in the log grid, Shift+B in the entry row).
    pub key_hint: Option<egui::KeyboardShortcut>,
    /// A key the log grid runs the command with while a row is selected (Ctrl+Z),
    /// so a text field keeps it otherwise; also shown in the menu. Pressed while
    /// the command is disabled, the status line says why. A double click on a row
    /// runs the command whose row key is Enter.
    pub row_key: Option<egui::KeyboardShortcut>,
    /// The present state, added to the tooltip ("Connected").
    pub state: Option<String>,
    /// Why the command cannot be run, added to the tooltip while disabled
    /// ("Connect a keyer first").
    pub disabled_reason: Option<String>,
    /// Whether it can be run now; shown greyed out otherwise.
    pub enabled: bool,
    /// Whether it is shown pressed or ticked (a window open, a CQ repeat running).
    pub selected: bool,
    /// Drawn as a check box in menus (a window toggle) rather than a button.
    pub checkable: bool,
    /// Listed in the menu of its provider's group.
    pub menu: bool,
    /// May be put on the toolbar.
    pub toolbar: bool,
    /// Listed in the right-click menu of a QSO in the log.
    pub context: bool,
    /// Whether its toolbar button is shown until the operator chooses otherwise
    /// in Settings > Toolbar.
    pub toolbar_default: bool,
    /// The name under which Settings > Toolbar remembers whether the button is
    /// shown; `None` for its provider's [`Commands::id`] (all buttons of a panel
    /// are shown or hidden together).
    pub toolbar_key: Option<String>,
    /// The part of its menu it is listed in; the application sorts a menu by
    /// section, [`Command::weight`] and label, with separators between sections.
    pub section: MenuSection,
    /// Where it stands within its section, lighter first; ties by label.
    pub weight: i32,
    /// Commands of a submenu (Import > ADIF, Cabrillo); on the toolbar, a button
    /// opening them as a menu. Their ids are passed to [`Commands::run`].
    pub children: Vec<Command>,
}

impl Command {
    /// The identifier of a panel's window toggle.
    pub const WINDOW: &'static str = "window";

    /// A command listed in its group's menu only (not on the toolbar).
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        tooltip: impl Into<String>,
    ) -> Self {
        Command {
            id: id.into(),
            label: label.into(),
            menu_label: None,
            tooltip: tooltip.into(),
            icon: None,
            shortcut: None,
            key_hint: None,
            row_key: None,
            state: None,
            disabled_reason: None,
            enabled: true,
            selected: false,
            checkable: false,
            menu: true,
            toolbar: false,
            context: false,
            toolbar_default: true,
            toolbar_key: None,
            section: MenuSection::Other,
            weight: 0,
            children: Vec::new(),
        }
    }

    /// A panel's button opening and closing its window, in its menu and on the
    /// toolbar (ticked and pressed while the window is open).
    pub fn window(label: impl Into<String>, tooltip: impl Into<String>) -> Self {
        let mut c = Command::new(Command::WINDOW, label, tooltip);
        c.toolbar = true;
        c.checkable = true;
        c
    }

    /// A toolbar button running [`Panel::run_command`] with `id`, not in a menu.
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

    /// Whether it is a panel's window toggle.
    pub fn is_window(&self) -> bool {
        self.id == Command::WINDOW
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Draw `glyph` (a constant of [`crate::icon`]) on the toolbar button.
    pub fn icon(mut self, glyph: &'static str) -> Self {
        self.icon = Some(glyph);
        self
    }

    /// Run the command with `shortcut`.
    pub fn shortcut(mut self, shortcut: egui::KeyboardShortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    /// Show `key` next to the menu item (handled elsewhere).
    pub fn key_hint(mut self, key: egui::KeyboardShortcut) -> Self {
        self.key_hint = Some(key);
        self
    }

    /// Run it with `key` on the log grid while a row is selected.
    pub fn row_key(mut self, key: egui::KeyboardShortcut) -> Self {
        self.row_key = Some(key);
        self
    }

    /// List it in `section` of its menu, at `weight` within it.
    pub fn section(mut self, section: MenuSection, weight: i32) -> Self {
        self.section = section;
        self.weight = weight;
        self
    }

    /// The key shown next to its menu item.
    pub fn menu_key(&self) -> Option<&egui::KeyboardShortcut> {
        self.shortcut
            .as_ref()
            .or(self.key_hint.as_ref())
            .or(self.row_key.as_ref())
    }

    /// Show `state` in the tooltip.
    pub fn state(mut self, state: impl Into<String>) -> Self {
        self.state = Some(state.into());
        self
    }

    /// Disable the command for `reason`, shown in the tooltip.
    pub fn disabled_because(mut self, reason: impl Into<String>) -> Self {
        self.enabled = false;
        self.disabled_reason = Some(reason.into());
        self
    }

    /// Put it on the toolbar, shown by default when `shown`.
    pub fn on_toolbar(mut self, shown: bool) -> Self {
        self.toolbar = true;
        self.toolbar_default = shown;
        self
    }

    /// List it in the right-click menu of a QSO.
    pub fn in_context(mut self) -> Self {
        self.context = true;
        self
    }

    /// Remember its toolbar button under `key` of its own.
    pub fn toolbar_key(mut self, key: impl Into<String>) -> Self {
        self.toolbar_key = Some(key.into());
        self
    }

    /// List it in menus as `label` rather than its (short) toolbar label.
    pub fn menu_label(mut self, label: impl Into<String>) -> Self {
        self.menu_label = Some(label.into());
        self
    }

    /// Give it a submenu.
    pub fn children(mut self, children: Vec<Command>) -> Self {
        self.children = children;
        self
    }

    /// Text of its menu item.
    pub fn menu_text(&self) -> &str {
        self.menu_label.as_deref().unwrap_or(&self.label)
    }

    /// The whole tooltip: the description with `shortcut` (formatted by the
    /// application) in parentheses, the state, and why it is disabled.
    pub fn full_tooltip(&self, shortcut: Option<&str>) -> String {
        let mut text = self.tooltip.clone();
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

/// A source of commands: the application's File, Edit and Device actions, or a
/// panel (through [`PanelCommands`]).
pub trait Commands {
    /// Stable identifier; also the name Settings > Toolbar remembers the
    /// provider's buttons under (see [`Command::toolbar_key`]).
    fn id(&self) -> &str;

    /// Name of the provider in Settings > Toolbar.
    fn title(&self, language: &str) -> String;

    /// The menu its commands are listed in, and the group of its toolbar buttons.
    fn group(&self) -> MenuGroup;

    /// Its commands now, in `language`.
    fn commands(&self, language: &str) -> Vec<Command>;

    /// The command (or submenu command) `id` was chosen while enabled.
    fn run(&mut self, id: &str);
}

/// A panel as a [`Commands`] provider: its window toggle (made from its title
/// when the panel has no button of its own) and its actions.
pub struct PanelCommands<'a> {
    pub panel: &'a mut dyn Panel,
    /// Whether the panel's window is open.
    pub open: &'a mut bool,
}

/// The commands of `panel`, its window toggle first and set to `open`.
pub fn panel_commands(panel: &dyn Panel, open: bool, language: &str) -> Vec<Command> {
    let mut list = panel.commands(language);
    let title = panel.title(language);
    if panel.has_window() && !list.iter().any(Command::is_window) {
        let mut w = Command::window(title.clone(), title.clone());
        w.toolbar = false;
        list.insert(0, w);
    }
    let help = panel.menu().is_app_menu();
    for c in &mut list {
        // Help and Settings items have no toolbar buttons.
        c.toolbar &= !help;
        c.toolbar_default = panel.toolbar_default_shown();
        // The panel's section for the commands that name none.
        if c.section == MenuSection::Other {
            (c.section, c.weight) = panel.section();
        }
        if c.is_window() {
            c.selected = open;
            // The window's title, unless the panel names its menu item.
            c.menu_label.get_or_insert_with(|| title.clone());
            // The Help menu opens a window rather than ticking it.
            c.checkable = !help;
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
            *self.open = self.panel.menu().is_app_menu() || !*self.open;
        } else {
            self.panel.run_command(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_adds_shortcut_state_and_the_reason_only_while_disabled() {
        let b = Command::action("cq", "CQ", "CQ — repeat F1").state("Stopped");
        assert_eq!(b.full_tooltip(None), "CQ — repeat F1\nStopped");
        assert_eq!(b.full_tooltip(Some("F1")), "CQ — repeat F1 (F1)\nStopped");
        let b = b.disabled_because("Connect a keyer first");
        assert!(!b.enabled);
        assert_eq!(
            b.full_tooltip(None),
            "CQ — repeat F1\nStopped\nConnect a keyer first"
        );
        let b = b.enabled(true);
        assert_eq!(b.full_tooltip(None), "CQ — repeat F1\nStopped");
        assert_eq!(
            Command::window("A", "B").icon(crate::icon::TROPHY).icon,
            Some(crate::icon::TROPHY)
        );
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

    struct Windowless;

    impl Panel for Windowless {
        fn id(&self) -> &str {
            "w"
        }
        fn title(&self, _language: &str) -> String {
            "Status only".into()
        }
        fn has_window(&self) -> bool {
            false
        }
        fn ui(&mut self, _ui: &mut egui::Ui, _view: &crate::View<'_>) {}
    }

    #[test]
    fn a_panel_without_a_window_gets_no_window_toggle() {
        assert!(panel_commands(&Windowless, false, "en").is_empty());
    }
}
