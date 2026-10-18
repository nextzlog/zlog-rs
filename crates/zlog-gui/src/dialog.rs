//! Every window other than the main one is a dialog: a window of its own on the
//! desktop, as zLog's forms are, so it can sit on another screen or desktop.
//! All of them are opened through [`dialog`], so the policy lives in one place.

use egui::{Context, Ui, ViewportBuilder, ViewportId};

/// Show a dialog while `*open` is true; closing it from the title bar clears `*open`.
pub fn dialog<R>(
    ctx: &Context,
    id: &str,
    title: &str,
    size: [f32; 2],
    open: &mut bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    if !*open {
        return None;
    }
    let builder = ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size);
    let mut add = Some(add);
    let (result, closed) =
        ctx.show_viewport_immediate(ViewportId::from_hash_of(id), builder, |ui, _class| {
            let closed = ui.ctx().input(|i| i.viewport().close_requested());
            let result = egui::CentralPanel::default()
                .show(ui, |ui| add.take().map(|f| f(ui)))
                .inner;
            (result, closed)
        });
    if closed {
        *open = false;
    }
    result
}
