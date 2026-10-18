//! zLog-style logging window for macOS and Windows.
//!
//! - [`app`]: switches between the zLog Menu and the main window.
//! - [`session`]: the open contest; turns operator actions into book commands.
//! - [`entry`]: the entry row as data.
//! - [`main_window`], [`windows`], [`menu_form`]: drawing only.
//! - [`dialog`]: every other window is a dialog of its own on the desktop.
//! - [`registry`]: the extensions linked into this build.
//! - [`rig`]: rig control (or the simulator) kept in step with the entry row.
//! - [`settings`]: settings kept between runs, one section per part and extension.
//! - [`i18n`]: user-interface text, one file per language in `lang/`.

mod app;
mod dialog;
mod entry;
mod i18n;
mod main_window;
mod menu_form;
mod options;
mod registry;
mod rig;
mod session;
mod settings;
mod windows;

use std::sync::Arc;

fn main() -> eframe::Result {
    i18n::set_from_environment();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 520.0])
            .with_title("zLog"),
        ..Default::default()
    };
    eframe::run_native(
        "zLog",
        options,
        Box::new(|cc| {
            install_japanese_font(&cc.egui_ctx);
            // zLog uses the light Windows look.
            cc.egui_ctx.set_theme(egui::Theme::Light);
            Ok(Box::new(app::ZLogApp::new()))
        }),
    )
}

/// egui has no Japanese glyphs; borrow a system font as a fallback for every family.
fn install_japanese_font(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        // The real file name of the macOS Hiragino font, so it stays in Japanese.
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "C:\\Windows\\Fonts\\YuGothR.ttc",
        "C:\\Windows\\Fonts\\meiryo.ttc",
        "C:\\Windows\\Fonts\\msgothic.ttc",
        "/usr/share/fonts/opentype/ipafont-gothic/ipag.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ];
    let Some(bytes) = CANDIDATES.iter().find_map(|p| std::fs::read(p).ok()) else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "japanese".into(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("japanese".into());
    }
    ctx.set_fonts(fonts);
}
