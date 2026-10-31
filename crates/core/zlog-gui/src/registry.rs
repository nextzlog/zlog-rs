use std::sync::OnceLock;

use zlog_core::Registry;
use zlog_ui::{Builder, Panel};

pub(crate) type Register = fn(&mut Builder);

static INSTALLED: OnceLock<Register> = OnceLock::new();

pub fn install(register: Register) {
    let _ = INSTALLED.set(register);
}

fn fallback() -> Register {
    #[cfg(test)]
    return zlog_std::register;
    #[cfg(not(test))]
    |_| {}
}

pub fn builder() -> &'static Builder {
    static BUILDER: OnceLock<Builder> = OnceLock::new();
    BUILDER.get_or_init(|| {
        let register = INSTALLED.get().copied().unwrap_or_else(fallback);
        let mut b = Builder::new();
        register(&mut b);
        b
    })
}

pub fn registry() -> Registry {
    builder().registry()
}

pub fn has_periods() -> bool {
    static HAS: OnceLock<bool> = OnceLock::new();
    *HAS.get_or_init(|| registry().has_periods())
}

pub fn panels() -> Vec<Box<dyn Panel>> {
    builder().build_panels(registry)
}

pub fn migrations() -> Vec<zlog_ui::migration::Migration> {
    static ALL: OnceLock<Vec<zlog_ui::migration::Migration>> = OnceLock::new();
    ALL.get_or_init(|| panels().iter().flat_map(|p| p.migrations()).collect())
        .clone()
}

pub(crate) fn start_screen() -> Option<Box<dyn zlog_ui::StartScreen>> {
    builder().build_start_screen(registry)
}

#[cfg(any(test, feature = "testing"))]
pub fn drivers() -> zlog_ui::device::Drivers {
    builder().drivers()
}

pub(crate) fn entry_styles() -> Vec<Box<dyn zlog_ui::EntryStyle>> {
    builder().build_entry_styles()
}

pub fn entry_style(id: &str) -> Option<Box<dyn zlog_ui::EntryStyle>> {
    let mut styles = entry_styles();
    let i = styles.iter().position(|s| s.id() == id).unwrap_or(0);
    (i < styles.len()).then(|| styles.swap_remove(i))
}

pub(crate) fn apply_style(ctx: &egui::Context, settings: &mut crate::settings::Settings) {
    if let Some((section, apply)) = builder().look() {
        apply(ctx, settings.section(section));
    }
    ctx.all_styles_mut(|s| s.spacing.scroll = egui::style::ScrollStyle::solid());
}

pub(crate) fn menu_bar() -> Option<Box<dyn zlog_ui::chrome::MenuBar>> {
    builder().build_menu_bar()
}

pub(crate) fn tool_bar() -> Option<Box<dyn zlog_ui::chrome::ToolBar>> {
    builder().build_tool_bar()
}

pub(crate) fn status_bar() -> Option<Box<dyn zlog_ui::chrome::StatusBar>> {
    builder().build_status_bar()
}

pub(crate) fn log_table() -> Option<Box<dyn zlog_ui::chrome::LogTable>> {
    builder().build_log_table()
}

pub(crate) fn window_host() -> Option<Box<dyn zlog_ui::chrome::WindowHost>> {
    builder().build_window_host()
}

pub fn settings_files() -> Vec<Box<dyn zlog_ui::SettingsFile>> {
    builder().build_settings_files()
}

pub(crate) fn settings_pages(
    toolbar: zlog_ui::toolbar::Rows,
) -> Vec<Box<dyn zlog_ui::SettingsPage>> {
    let mut pages: Vec<Box<dyn zlog_ui::SettingsPage>> = vec![
        Box::new(crate::options::StationPage),
        Box::new(crate::options::PreferencesPage::new(entry_styles())),
    ];
    pages.extend(builder().build_pages(registry, toolbar));
    pages
}

pub(crate) fn subcommand(name: &str) -> Option<fn()> {
    builder().subcommand_of(name)
}
