//! zLog-style logging window for macOS and Windows.
//!
//! - [`app`]: switches between the zLog Menu and the main window.
//! - [`session`]: the open contest; turns operator actions into book commands.
//! - [`entry`]: the entry row as data.
//! - [`main_window`], [`windows`]: drawing only; the zLog Menu is an extension
//!   ([`zlog_ui::StartScreen`], zlog-open).
//! - [`dialog`] (from zlog-ui): every other window is a dialog of its own on the desktop.
//! - [`registry`]: the extensions linked into this build.
//! - [`rig`]: rig control (or the simulator) kept in step with the entry row.
//! - [`settings`]: settings kept between runs, one section per part and extension.
//! - [`i18n`]: user-interface text, one file per language in `lang/`.
//! - [`fonts`]: a Japanese font of the system, found at start-up.

mod app;
mod commands;
mod entry;
mod fonts;
mod i18n;
#[cfg(feature = "ini")]
mod ini;
mod main_window;
mod menus;
mod options;
mod registry;
mod rig;
mod rigs;
mod row;
mod session;
mod settings;
mod toolbar;
mod windows;

use zlog_ui::dialog;

// The click-through tests drive the zLog Menu and the user-defined contests.
#[cfg(all(test, feature = "contests"))]
mod gui_tests;

fn main() -> eframe::Result {
    // A ZyLO plugin helper process: no window, nothing else of the application.
    #[cfg(feature = "zylo")]
    if std::env::args().nth(1).as_deref() == Some(zlog_zylo::SUBCOMMAND) {
        zlog_zylo::helper_main();
    }
    i18n::set_from_environment();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([480.0, 320.0])
            .with_title("zLog RS"),
        ..Default::default()
    };
    eframe::run_native(
        "zLog",
        options,
        Box::new(|cc| {
            fonts::install(&cc.egui_ctx);
            // zLog uses the light Windows look.
            cc.egui_ctx.set_theme(egui::Theme::Light);
            Ok(Box::new(app::ZLogApp::new()))
        }),
    )
}
