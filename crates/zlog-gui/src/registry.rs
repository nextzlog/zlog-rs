//! The extensions linked into this build.
//!
//! Each extension crate is a Cargo feature of this crate. To add one: add the crate
//! as an optional dependency and a feature in `Cargo.toml`, then one line here.
//! Log extensions ([`registry`]) receive events in the order listed;
//! window extensions ([`panels`]) appear in the Windows menu in the order listed.

use zlog_core::Registry;
use zlog_ui::Panel;

pub fn registry() -> Registry {
    let registry = Registry::new();
    #[cfg(feature = "jarl")]
    let registry = registry.with(zlog_jarl::Jarl);
    #[cfg(feature = "japan")]
    let registry = registry.with(zlog_japan::Japan);
    #[cfg(feature = "world")]
    let registry = registry.with(zlog_world::World);
    #[cfg(feature = "general")]
    let registry = registry.with(zlog_general::General);
    #[cfg(feature = "qxsl")]
    let registry = registry.with(zlog_qxsl::Qxsl);
    #[cfg(feature = "prefix")]
    let registry = registry.with(zlog_prefix::PrefixExtension);
    registry
}

// One push per feature reads better than a vec! that cannot hold #[cfg] items.
#[allow(unused_mut, clippy::vec_init_then_push)]
pub fn panels() -> Vec<Box<dyn Panel>> {
    let mut panels: Vec<Box<dyn Panel>> = Vec::new();
    #[cfg(feature = "score")]
    panels.push(Box::new(zlog_score::Score));
    #[cfg(feature = "score")]
    panels.push(Box::new(zlog_score::Multipliers::default()));
    #[cfg(feature = "maplot")]
    panels.push(Box::new(zlog_maplot::Maplot::default()));
    #[cfg(feature = "cw")]
    panels.push(Box::new(zlog_cw::CwPanel::default()));
    #[cfg(feature = "soumu")]
    panels.push(Box::new(zlog_soumu::Soumu::default()));
    #[cfg(feature = "es")]
    panels.push(Box::new(zlog_es::EsInfo::default()));
    #[cfg(feature = "comports")]
    panels.push(Box::new(zlog_comports::ComPorts::default()));
    #[cfg(feature = "latest")]
    panels.push(Box::new(zlog_latest::Latest::new(
        env!("CARGO_PKG_REPOSITORY"),
        env!("CARGO_PKG_VERSION"),
    )));
    panels
}

/// Pages extensions add to the Options dialog, in the order listed.
#[allow(unused_mut, clippy::vec_init_then_push)]
pub fn settings_pages() -> Vec<Box<dyn zlog_ui::SettingsPage>> {
    let mut pages: Vec<Box<dyn zlog_ui::SettingsPage>> = Vec::new();
    pages
}
