//! The extensions linked into this build.
//!
//! Each extension crate is a Cargo feature of this crate. To add one: add the crate
//! as an optional dependency and a feature in `Cargo.toml`, then one line here.
//! Log extensions ([`registry`]) receive events in the order listed;
//! window extensions ([`panels`]) appear in their group menus and on the toolbar in the order listed.

use zlog_core::Registry;
use zlog_ui::Panel;

pub fn registry() -> Registry {
    let registry = Registry::new();
    // Before zlog-cfg: CFG files with a `dll` line are opened with the rule plugin.
    #[cfg(feature = "zylo")]
    let registry = registry.with(zlog_zylo::Zylo);
    #[cfg(feature = "cfg")]
    let registry = registry.with(zlog_cfg::Cfg);
    #[cfg(feature = "rule-ja")]
    let registry = registry.with(zlog_rule_ja::Japan);
    #[cfg(feature = "rule-dx")]
    let registry = registry.with(zlog_rule_dx::World);
    #[cfg(feature = "base")]
    let registry = registry.with(zlog_rule::Base);
    #[cfg(feature = "qxsl")]
    let registry = registry.with(zlog_qxsl::Qxsl);
    #[cfg(feature = "link")]
    let registry = registry.with(zlog_link::ZLink);
    #[cfg(feature = "dc")]
    let registry = registry.with(zlog_dc::Discord);
    #[cfg(feature = "undo")]
    let registry = registry.with(zlog_undo::Undo::default());
    registry
}

// One push per feature reads better than a vec! that cannot hold #[cfg] items.
#[allow(unused_mut, clippy::vec_init_then_push)]
pub fn panels() -> Vec<Box<dyn Panel>> {
    let mut panels: Vec<Box<dyn Panel>> = Vec::new();
    #[cfg(feature = "edit")]
    panels.push(Box::new(zlog_edit::Editor::default()));
    #[cfg(feature = "undo")]
    panels.push(Box::new(zlog_undo::UndoMenu::default()));
    #[cfg(feature = "score")]
    panels.push(Box::new(zlog_score::Score));
    #[cfg(feature = "score")]
    panels.push(Box::new(zlog_score::Multipliers::default()));
    #[cfg(feature = "map")]
    panels.push(Box::new(zlog_map::Maplot::default()));
    #[cfg(feature = "check")]
    panels.extend(zlog_check::panels());
    #[cfg(feature = "cw")]
    panels.push(Box::new(zlog_cw::CwPanel::default()));
    // Before the keyer: it sees every PTT request, then leaves it to the keyer.
    #[cfg(feature = "so2r")]
    panels.push(Box::new(zlog_so2r::So2rPanel::default()));
    #[cfg(feature = "so2r")]
    panels.push(Box::new(zlog_so2r::QueuePanel::default()));
    #[cfg(feature = "keyer")]
    panels.push(Box::new(zlog_keyer::KeyerPanel::default()));
    #[cfg(feature = "voice")]
    panels.push(Box::new(zlog_voice::VoicePanel::default()));
    #[cfg(feature = "qsy")]
    panels.push(Box::new(zlog_qsy::QsyInfo));
    #[cfg(feature = "rtty")]
    panels.push(Box::new(zlog_rtty::RttyPanel::default()));
    #[cfg(feature = "ops")]
    panels.push(Box::new(zlog_ops::Crew::default()));
    #[cfg(feature = "link")]
    panels.push(Box::new(zlog_link::ZLinkPanel::default()));
    #[cfg(feature = "link")]
    panels.push(Box::new(zlog_link::FreqPanel::default()));
    #[cfg(feature = "cluster")]
    panels.push(Box::new(zlog_cluster::Cluster::default()));
    #[cfg(feature = "scope")]
    panels.push(Box::new(zlog_scope::BandScope::default()));
    #[cfg(feature = "soumu")]
    panels.push(Box::new(zlog_soumu::Soumu::default()));
    #[cfg(feature = "unr")]
    panels.push(Box::new(zlog_unr::CorrectNr::default()));
    #[cfg(feature = "search")]
    panels.push(Box::new(zlog_search::Search::default()));
    #[cfg(feature = "rate")]
    panels.push(Box::new(zlog_rate::Rate::default()));
    #[cfg(feature = "summary")]
    panels.push(Box::new(zlog_summary::Summary::default()));
    #[cfg(feature = "jarl")]
    panels.push(Box::new(zlog_jarl::MemberCheck::default()));
    #[cfg(feature = "es")]
    panels.push(Box::new(zlog_es::EsInfo::default()));
    #[cfg(feature = "ports")]
    panels.push(Box::new(zlog_ports::ComPorts::default()));
    #[cfg(feature = "zylo")]
    panels.push(Box::new(zlog_zylo::ZyloPanel::default()));
    #[cfg(feature = "qth")]
    panels.push(Box::new(zlog_qth::CountryStatus::default()));
    #[cfg(feature = "dc")]
    panels.push(Box::new(zlog_dc::DiscordStatus::default()));
    #[cfg(feature = "update")]
    panels.push(Box::new(zlog_update::Updater::new(env!(
        "CARGO_PKG_REPOSITORY"
    ))));
    panels
}

/// The screen shown until a log is open (the zLog Menu), listing
/// [`contest_entries`]; `None` without the `open` feature, where the main window
/// only says that no start screen is linked in.
pub fn start_screen() -> Option<Box<dyn zlog_ui::StartScreen>> {
    #[cfg(feature = "open")]
    return Some(Box::new(zlog_open::MenuForm::new(contest_entries())));
    #[cfg(not(feature = "open"))]
    None
}

/// The entries of the start screen's contest menu, in the order listed: the
/// groups of the linked contests, then the user-defined contest window.
#[cfg(feature = "open")]
#[allow(unused_mut)]
fn contest_entries() -> Vec<Box<dyn zlog_ui::ContestEntry>> {
    let mut entries = zlog_ui::contest_groups(&registry());
    #[cfg(feature = "contests")]
    entries.push(Box::new(zlog_cfg::window::ContestWindow::new(
        contest_lists(),
    )));
    entries
}

/// Lists of contest files searched in the user-defined contest window, in the
/// order listed.
#[cfg(feature = "contests")]
#[allow(unused_mut, clippy::vec_init_then_push)]
fn contest_lists() -> Vec<Box<dyn zlog_ui::ContestList>> {
    let mut lists: Vec<Box<dyn zlog_ui::ContestList>> = Vec::new();
    #[cfg(feature = "ja6ycu")]
    lists.push(Box::new(zlog_ja6ycu::Ja6ycu::default()));
    lists
}

/// The styles of the entry row, in the order listed; the first is the default.
#[allow(unused_mut, clippy::vec_init_then_push)]
pub fn entry_styles() -> Vec<Box<dyn zlog_ui::EntryStyle>> {
    let mut styles: Vec<Box<dyn zlog_ui::EntryStyle>> = Vec::new();
    #[cfg(feature = "entry")]
    styles.push(Box::new(zlog_entry::ZlogRow::default()));
    styles
}

/// The entry style `id`, or the first when none is linked in by that name; `None`
/// when no style is linked in, where the main window has no entry row.
pub fn entry_style(id: &str) -> Option<Box<dyn zlog_ui::EntryStyle>> {
    let mut styles = entry_styles();
    let i = styles.iter().position(|s| s.id() == id).unwrap_or(0);
    (i < styles.len()).then(|| styles.swap_remove(i))
}

/// Give every window the look kept in the settings (the theme of the Display
/// tab); without the `style` feature, egui's own.
#[allow(unused_variables)]
pub fn apply_style(ctx: &egui::Context, settings: &mut crate::settings::Settings) {
    #[cfg(feature = "style")]
    zlog_style::apply(ctx, settings.section(zlog_style::SECTION));
}

/// The tabs of the Options dialog, in the order listed.
#[allow(unused_mut, clippy::vec_init_then_push)]
pub fn settings_pages(toolbar: crate::toolbar::Rows) -> Vec<Box<dyn zlog_ui::SettingsPage>> {
    // The application's own tabs first, then the extensions'.
    let mut pages: Vec<Box<dyn zlog_ui::SettingsPage>> = vec![
        Box::new(crate::options::StationPage),
        Box::new(crate::options::PreferencesPage::new(entry_styles())),
        Box::new(crate::options::RigPage::new(1)),
        Box::new(crate::options::RigPage::new(2)),
        Box::new(crate::toolbar::ToolbarPage::new(toolbar)),
    ];
    #[cfg(feature = "bind")]
    pages.push(Box::new(zlog_bind::KeysPage::default()));
    #[cfg(feature = "style")]
    pages.push(Box::new(zlog_style::DisplayPage));
    #[cfg(feature = "zylo")]
    pages.push(Box::new(zlog_zylo::ZyloOptions::default()));
    // Edits the section of its status item (`panel.dc`), which takes it after OK.
    #[cfg(feature = "dc")]
    pages.push(Box::new(zlog_dc::DiscordOptions::default()));
    pages
}

#[cfg(test)]
mod tests {
    /// Every toolbar button of every panel has an icon and a tooltip naming and
    /// describing the tool in both languages; a disabled one says why.
    #[test]
    fn toolbar_buttons_have_icons_and_tooltips() {
        for lang in ["en", "ja"] {
            for p in super::panels() {
                for b in p.commands(lang).into_iter().filter(|b| b.toolbar) {
                    let name = format!("{lang} {} {}", p.id(), b.label);
                    assert!(b.icon.is_some(), "{name}: no icon");
                    let (title, what) = b.tooltip.split_once(" — ").expect(&name);
                    assert!(!title.is_empty() && !what.is_empty(), "{name}");
                    if !b.is_window() {
                        assert!(b.state.is_some(), "{name}: no state");
                    }
                    if !b.enabled {
                        let why = b.disabled_reason.as_deref().expect(&name);
                        assert!(b.full_tooltip(None).ends_with(why), "{name}");
                    }
                }
            }
        }
        // The keyer's CQ and Stop without a keyer, and the network Connects
        // without a host, say what to do first.
        let ja: Vec<_> = super::panels()
            .iter()
            .flat_map(|p| p.commands("ja"))
            .filter(|b| !b.enabled)
            .collect();
        assert!(ja.len() >= 2 && ja.iter().all(|b| b.disabled_reason.is_some()));
    }

    /// The updater follows the nightly of nextzlog/zlog-rs; an empty repository in
    /// Cargo.toml once left it asking for `repos//releases`.
    #[test]
    fn the_updater_follows_this_repository() {
        assert_eq!(
            env!("CARGO_PKG_REPOSITORY"),
            "https://github.com/nextzlog/zlog-rs"
        );
        #[cfg(feature = "update")]
        assert_eq!(
            zlog_update::release_api_url(&zlog_update::repository_path(env!(
                "CARGO_PKG_REPOSITORY"
            ))),
            "https://api.github.com/repos/nextzlog/zlog-rs/releases/tags/nightly"
        );
    }
}
