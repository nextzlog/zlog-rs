use egui::{Color32, Ui};

pub fn error(ui: &Ui) -> Color32 {
    ui.visuals().error_fg_color
}

pub fn warning(ui: &Ui) -> Color32 {
    ui.visuals().warn_fg_color
}

pub fn good(ui: &Ui) -> Color32 {
    pick(
        ui,
        Color32::from_rgb(0, 120, 0),
        Color32::from_rgb(110, 200, 110),
    )
}

pub fn weak(ui: &Ui) -> Color32 {
    ui.visuals().weak_text_color()
}

pub fn cq(ui: &Ui) -> Color32 {
    pick(ui, Color32::BLUE, Color32::from_rgb(120, 160, 255))
}

pub fn sp(ui: &Ui) -> Color32 {
    pick(
        ui,
        Color32::from_rgb(255, 0, 255),
        Color32::from_rgb(255, 120, 255),
    )
}

fn pick(ui: &Ui, light: Color32, dark: Color32) -> Color32 {
    if ui.visuals().dark_mode {
        dark
    } else {
        light
    }
}
