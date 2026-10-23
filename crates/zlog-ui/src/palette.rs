//! Colours of meaning (an error, a warning, a success, CQ and S&P) that read well
//! in the light and in the dark theme; text drawn in a colour of its own should
//! take it from here rather than fix one.

use egui::{Color32, Ui};

/// Text saying something failed.
pub fn error(ui: &Ui) -> Color32 {
    ui.visuals().error_fg_color
}

/// Text asking for attention (a missing file, a dupe at hand).
pub fn warning(ui: &Ui) -> Color32 {
    ui.visuals().warn_fg_color
}

/// Text saying something went well, or is there.
pub fn good(ui: &Ui) -> Color32 {
    pick(
        ui,
        Color32::from_rgb(0, 120, 0),
        Color32::from_rgb(110, 200, 110),
    )
}

/// Text of less weight (done, not known).
pub fn weak(ui: &Ui) -> Color32 {
    ui.visuals().weak_text_color()
}

/// CQ (run) mode, blue as zLog shows it.
pub fn cq(ui: &Ui) -> Color32 {
    pick(ui, Color32::BLUE, Color32::from_rgb(120, 160, 255))
}

/// S&P mode, fuchsia as zLog shows it.
pub fn sp(ui: &Ui) -> Color32 {
    pick(
        ui,
        Color32::from_rgb(255, 0, 255),
        Color32::from_rgb(255, 120, 255),
    )
}

/// `light` in the light theme, `dark` in the dark one.
fn pick(ui: &Ui, light: Color32, dark: Color32) -> Color32 {
    if ui.visuals().dark_mode {
        dark
    } else {
        light
    }
}
