//! Every window other than the main one is a dialog: a window of its own on the
//! desktop, as zLog's forms are, so it can sit on another screen or desktop.
//! All of them are opened through [`dialog`], so the policy lives in one place.

use std::path::PathBuf;

use zlog_core::Format;

use egui::{Context, Ui, Vec2, ViewportBuilder, ViewportCommand, ViewportId};

/// The viewport of the dialog `id`, to send it commands (focus, say).
pub fn viewport_id(id: &str) -> ViewportId {
    ViewportId::from_hash_of(id)
}

fn on_top_id() -> egui::Id {
    egui::Id::unique("dialogs_on_top")
}

/// Keep every dialog above other windows (Window menu), from the next frame on.
pub fn set_keep_on_top(ctx: &Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(on_top_id(), on));
}

/// Bring the dialog `id` to the front and give it the keyboard.
pub fn focus(ctx: &Context, id: &str) {
    ctx.send_viewport_cmd_to(viewport_id(id), ViewportCommand::Focus);
}

/// The smallest a dialog may be made; its content scrolls below this.
const MIN_SIZE: [f32; 2] = [200.0, 120.0];

/// The smallest a dialog opens at, whatever size it asks for, so no form opens
/// cramped (its content then grows with the window).
pub const OPEN_SIZE: [f32; 2] = [480.0, 320.0];

/// The width of every modal question (the start time, a deletion, a missing
/// file): its text wraps and its fields fill it.
pub const MODAL_WIDTH: f32 = 460.0;

/// Frames a dialog is drawn before it is grown to fit its content: the first frame
/// lays out with no measured sizes yet, so the second one is measured.
const FIT_FRAME: u8 = 2;

/// Show a dialog while `*open` is true; closing it from the title bar clears `*open`.
///
/// The content scrolls in both directions inside the window, so nothing is ever out
/// of reach however small the window is made.
pub fn dialog<R>(
    ctx: &Context,
    id: &str,
    title: &str,
    size: [f32; 2],
    open: &mut bool,
    mut add: impl FnMut(&mut Ui) -> R,
) -> Option<R> {
    frame(
        ctx,
        id,
        title,
        size,
        open,
        None::<fn(&mut Ui)>,
        None::<fn(&mut Ui)>,
        |ui, _| add(ui),
    )
    .map(|(r, _)| r)
}

/// A dialog with a row of buttons (OK, Cancel) pinned to the bottom of the window,
/// always visible, and the form above it scrolling. `buttons` is drawn first.
pub fn dialog_with_buttons<R, B>(
    ctx: &Context,
    id: &str,
    title: &str,
    size: [f32; 2],
    open: &mut bool,
    mut body: impl FnMut(&mut Ui) -> R,
    buttons: impl FnMut(&mut Ui) -> B,
) -> Option<(R, B)> {
    frame(
        ctx,
        id,
        title,
        size,
        open,
        None::<fn(&mut Ui)>,
        Some(buttons),
        |ui, _| body(ui),
    )
    .map(|(r, b)| (r, b.expect("buttons")))
}

/// A dialog of several pages: the list of pages on the left (`pages`, which returns
/// the page chosen), the chosen page scrolling on the right (`body`), and the
/// buttons pinned to the bottom.
#[allow(clippy::too_many_arguments)]
pub fn dialog_with_pages<P, R, B>(
    ctx: &Context,
    id: &str,
    title: &str,
    size: [f32; 2],
    open: &mut bool,
    pages: impl FnMut(&mut Ui) -> P,
    mut body: impl FnMut(&mut Ui, P) -> R,
    buttons: impl FnMut(&mut Ui) -> B,
) -> Option<(R, B)> {
    frame(
        ctx,
        id,
        title,
        size,
        open,
        Some(pages),
        Some(buttons),
        |ui, p| body(ui, p.expect("pages")),
    )
    .map(|(r, b)| (r, b.expect("buttons")))
}

/// OK and Cancel in `language`, as zLog's dialogs have them; Enter and Esc press
/// them when `keys`. `Some(true)` for OK, `Some(false)` for Cancel.
pub fn ok_cancel(ui: &mut Ui, language: &str, ok_enabled: bool, keys: bool) -> Option<bool> {
    let mut pressed = None;
    let text = |key| crate::texts().get(language, key);
    ui.horizontal(|ui| {
        let ok = ui.add_enabled(ok_enabled, egui::Button::new(text("button.ok")));
        let enter = keys && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ok.clicked() || (ok_enabled && enter) {
            pressed = Some(true);
        }
        let cancel = ui.button(text("button.cancel"));
        if cancel.clicked() || (keys && ui.input(|i| i.key_pressed(egui::Key::Escape))) {
            pressed = Some(false);
        }
    });
    pressed
}

/// A bottom row asking to confirm an action: `question` in bold, then the button
/// doing it (`yes`) and the one going back (`no`). `Some(true)` for `yes`,
/// `Some(false)` for `no`.
pub fn confirm_row(ui: &mut Ui, question: &str, yes: &str, no: &str) -> Option<bool> {
    let mut pressed = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(question).strong());
        if ui.button(yes).clicked() {
            pressed = Some(true);
        }
        if ui.button(no).clicked() {
            pressed = Some(false);
        }
    });
    pressed
}

/// A modal question over the main window: `body` scrolls when the window is too low
/// to show it, and the `buttons` below it stay in view.
pub fn modal_with_buttons<R, B>(
    ctx: &Context,
    id: &str,
    body: impl FnOnce(&mut Ui) -> R,
    buttons: impl FnOnce(&mut Ui) -> B,
) -> (R, B) {
    egui::Modal::new(egui::Id::unique(("modal", id)))
        .show(ctx, |ui| {
            ui.set_width(
                MODAL_WIDTH
                    .min(ctx.content_rect().width() - 40.0)
                    .max(200.0),
            );
            let room = modal_body_height(ctx.content_rect().height());
            let r = egui::ScrollArea::both()
                .id_salt(("modal_body", id))
                .max_height(room)
                .show(ui, body)
                .inner;
            ui.separator();
            (r, buttons(ui))
        })
        .inner
}

/// Height left to a modal's body in a window `height` high: the rest is the modal's
/// frame, the separator and the row of buttons.
fn modal_body_height(height: f32) -> f32 {
    (height - 120.0).max(40.0)
}

/// Draw the parts of a window: buttons at the bottom, the pages on the left, and the
/// body scrolling in what is left. Used by the dialogs and, through
/// [`window_parts`], by a start screen drawn in the main window.
#[allow(clippy::too_many_arguments)]
fn frame<P, B, R>(
    ctx: &Context,
    id: &str,
    title: &str,
    size: [f32; 2],
    open: &mut bool,
    mut pages: Option<impl FnMut(&mut Ui) -> P>,
    mut buttons: Option<impl FnMut(&mut Ui) -> B>,
    mut body: impl FnMut(&mut Ui, Option<P>) -> R,
) -> Option<(R, Option<B>)> {
    let fit_id = egui::Id::unique(("dialog_fit", id));
    if !*open {
        // Fitted again the next time it opens.
        ctx.data_mut(|d| d.remove::<u8>(fit_id));
        return None;
    }
    let builder = ViewportBuilder::default()
        .with_title(title)
        .with_inner_size([size[0].max(OPEN_SIZE[0]), size[1].max(OPEN_SIZE[1])])
        .with_min_inner_size(MIN_SIZE)
        .with_resizable(true)
        .with_window_level(window_level(ctx));
    // egui may draw the window more than once in a frame (a pass discarded for
    // layout), so the parts are called again on every pass.
    let (result, closed) = ctx.show_viewport_immediate(viewport_id(id), builder, |ui, class| {
        let closed = ui.ctx().input(|i| i.viewport().close_requested());
        let (result, needed) = window_parts(
            ui,
            id,
            pages.as_mut().map(|f| move |ui: &mut Ui| f(ui)),
            buttons.as_mut().map(|f| move |ui: &mut Ui| f(ui)),
            |ui, p| body(ui, p),
        );
        // An embedded dialog (no window of its own, as in tests) is an egui window
        // in the main one: growing "its" viewport would grow the main window.
        if class != egui::ViewportClass::EmbeddedWindow {
            fit(ui.ctx(), fit_id, needed);
        }
        (result, closed)
    });
    if closed {
        *open = false;
    }
    Some(result)
}

/// The level of every dialog: above other windows when asked to keep them on top.
fn window_level(ctx: &Context) -> egui::WindowLevel {
    let on = ctx
        .data(|d| d.get_temp::<bool>(on_top_id()))
        .unwrap_or(false);
    if on {
        egui::WindowLevel::AlwaysOnTop
    } else {
        egui::WindowLevel::Normal
    }
}

/// Lay out the parts of a window in `ui` (the whole window): buttons pinned to the
/// bottom, the pages on the left, the body in a scroll area filling the rest.
/// Returns what the parts returned and the window size that shows all of the body.
pub fn window_parts<P, B, R>(
    ui: &mut Ui,
    id: &str,
    pages: Option<impl FnOnce(&mut Ui) -> P>,
    buttons: Option<impl FnOnce(&mut Ui) -> B>,
    body: impl FnOnce(&mut Ui, Option<P>) -> R,
) -> ((R, Option<B>), Vec2) {
    let window = ui.max_rect().size();
    let b = buttons.map(|f| {
        egui::Panel::bottom(egui::Id::unique(("dialog_buttons", id)))
            .resizable(false)
            .show(ui, |ui| {
                ui.add_space(2.0);
                let b = f(ui);
                ui.add_space(2.0);
                b
            })
            .inner
    });
    let p = pages.map(|f| {
        egui::Panel::left(egui::Id::unique(("dialog_pages", id)))
            .resizable(false)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("pages")
                    .show(ui, f)
                    .inner
            })
            .inner
    });
    let out = egui::CentralPanel::default()
        .show(ui, |ui| {
            egui::ScrollArea::both()
                .id_salt(("dialog_body", id))
                .auto_shrink(false)
                .show(ui, |ui| body(ui, p))
        })
        .inner;
    // The window less the scroll area's view, plus the whole content.
    let needed = window - out.inner_rect.size() + out.content_size;
    ((out.inner, b), needed)
}

/// Grow the current window once, after it is first laid out, so that its content
/// fits without scrolling (no larger than the monitor); it is never shrunk.
fn fit(ctx: &Context, fit_id: egui::Id, needed: Vec2) {
    let frame = ctx.data_mut(|d| {
        let n = d.get_temp_mut_or_default::<u8>(fit_id);
        *n = n.saturating_add(1);
        *n
    });
    if frame != FIT_FRAME {
        return;
    }
    let (inner, monitor) = ctx.input(|i| {
        let v = i.viewport();
        (v.inner_rect.map(|r| r.size()), v.monitor_size)
    });
    let Some(inner) = inner else {
        return;
    };
    if let Some(size) = grown_size(inner, needed, monitor) {
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
    }
}

/// The size to grow a window of `inner` to so that `needed` fits, kept within 90%
/// of the monitor; `None` when it already fits.
fn grown_size(inner: Vec2, needed: Vec2, monitor: Option<Vec2>) -> Option<Vec2> {
    let mut size = inner.max(needed.ceil());
    if let Some(m) = monitor {
        size = size.min(m * 0.9).max(inner);
    }
    (size.x > inner.x + 0.5 || size.y > inner.y + 0.5).then_some(size)
}

/// Extensions of a zLog log; ZLOX first, the format zLog writes by default.
const LOG_TYPES: &[(&str, &str)] = &[("ZLOX", "zlox"), ("ZLO", "zlo")];

/// A file-type filter label and its extensions in both cases, as `Name (*.a, *.b)`.
///
/// macOS compares extensions case-sensitively, and zLog writes `.ZLO` as well as
/// `.zlo`, so both cases are offered.
fn filter(name: &str, exts: &[&str]) -> (String, Vec<String>) {
    let label = exts
        .iter()
        .map(|e| format!("*.{e}"))
        .collect::<Vec<_>>()
        .join(", ");
    let both = exts
        .iter()
        .flat_map(|e| [e.to_ascii_lowercase(), e.to_ascii_uppercase()])
        .collect();
    (format!("{name} ({label})"), both)
}

fn with_filters(mut dialog: rfd::FileDialog, filters: &[(String, Vec<String>)]) -> rfd::FileDialog {
    for (label, exts) in filters {
        dialog = dialog.add_filter(label, exts);
    }
    dialog
}

/// Filters for a log: both types together, then each alone.
fn log_filters(together: bool) -> Vec<(String, Vec<String>)> {
    let mut all = Vec::new();
    if together {
        let exts: Vec<&str> = LOG_TYPES.iter().map(|(_, e)| *e).collect();
        all.push(filter("zLog", &exts));
    }
    all.extend(LOG_TYPES.iter().map(|(n, e)| filter(n, &[e])));
    all
}

#[cfg(feature = "test-hooks")]
thread_local! {
    /// What the file dialogs answer on this thread ([`set_test_pick`]).
    static TEST_PICK: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Make the file dialogs of this thread answer `path` (`None`: cancelled) from now
/// on. With the `test-hooks` feature, which tests that click through the windows
/// turn on as a dev-dependency, no file dialog ever opens: they are cancelled
/// until this is called.
#[cfg(feature = "test-hooks")]
pub fn set_test_pick(path: Option<PathBuf>) {
    TEST_PICK.with(|p| *p.borrow_mut() = path);
}

/// The answer of a file dialog under the `test-hooks` feature ([`set_test_pick`]);
/// `None` without it, where the dialog opens.
fn test_pick() -> Option<Option<PathBuf>> {
    #[cfg(feature = "test-hooks")]
    return Some(TEST_PICK.with(|p| p.borrow().clone()));
    #[cfg(not(feature = "test-hooks"))]
    None
}

/// Pick an existing log to open.
pub fn open_log_file() -> Option<PathBuf> {
    if let Some(p) = test_pick() {
        return p;
    }
    with_filters(rfd::FileDialog::new(), &log_filters(true)).pick_file()
}

/// Extensions of a log in `format`, its own first.
fn log_exts(format: Format) -> [&'static str; 2] {
    match format {
        Format::Zlox => ["zlox", "zlo"],
        Format::Zlo => ["zlo", "zlox"],
    }
}

/// The name a save dialog for `format` suggests: `name` with its log extension, if
/// any, changed to that of `format`.
fn suggested_name(name: &str, format: Format) -> String {
    let [own, other] = log_exts(format);
    let path = PathBuf::from(name);
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case(other) || e.eq_ignore_ascii_case(own) => {
            path.with_extension(own).display().to_string()
        }
        _ => name.to_string(),
    }
}

/// Ask where to keep a log in `format`.
///
/// The format is chosen before the dialog (a menu item for each), because the
/// dialog does not say which file type was picked; a name typed without a log
/// extension gets that of `format`, while a typed `.zlo` or `.zlox` wins.
pub fn choose_log_file(name: &str, format: Format) -> Option<SaveTo> {
    let exts = log_exts(format);
    if let Some(p) = test_pick() {
        return p.map(|p| SaveTo::new(p, &exts));
    }
    let (label, _) = LOG_TYPES
        .iter()
        .find(|(_, e)| *e == exts[0])
        .expect("log type");
    let dialog = rfd::FileDialog::new().set_file_name(suggested_name(name, format));
    with_filters(dialog, &[filter(label, &exts[..1])])
        .save_file()
        .map(|p| SaveTo::new(p, &exts))
}

/// Pick a folder, starting in `start` when it is one.
pub fn pick_folder(start: &str) -> Option<PathBuf> {
    if let Some(p) = test_pick() {
        return p;
    }
    let mut dialog = rfd::FileDialog::new();
    if !start.is_empty() && std::path::Path::new(start).is_dir() {
        dialog = dialog.set_directory(start);
    }
    dialog.pick_folder()
}

/// Pick a file to import in a format with these extensions.
pub fn import_file(name: &str, exts: &[&str]) -> Option<PathBuf> {
    if let Some(p) = test_pick() {
        return p;
    }
    with_filters(rfd::FileDialog::new(), &[filter(name, exts)]).pick_file()
}

/// Pick any number of files of any type (Import of contest files, which checks
/// what it takes).
pub fn import_files() -> Vec<PathBuf> {
    if let Some(p) = test_pick() {
        return p.into_iter().collect();
    }
    rfd::FileDialog::new().pick_files().unwrap_or_default()
}

/// Ask where to export in a format with these extensions; the first is added to a
/// name typed without any of them.
pub fn export_file(name: &str, exts: &[&str]) -> Option<SaveTo> {
    if let Some(p) = test_pick() {
        return p.map(|p| SaveTo::new(p, exts));
    }
    with_filters(rfd::FileDialog::new(), &[filter(name, exts)])
        .save_file()
        .map(|p| SaveTo::new(p, exts))
}

/// Where a save dialog says to write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveTo {
    pub path: PathBuf,
    /// The file exists but the system dialog did not ask about replacing it: the
    /// extension was added after the dialog, to a name typed without one.
    pub ask: bool,
}

impl SaveTo {
    fn new(picked: PathBuf, exts: &[&str]) -> Self {
        let path = with_extension(picked.clone(), exts);
        let ask = path != picked && path.exists();
        SaveTo { path, ask }
    }
}

/// Ask whether to replace the file `question` names: `Some(true)` to replace it,
/// `Some(false)` to leave it, `None` while unanswered.
pub fn ask_overwrite(ctx: &Context, question: &str, language: &str) -> Option<bool> {
    modal_with_buttons(
        ctx,
        "overwrite",
        |ui| {
            ui.label(question);
        },
        // No Enter: a key left over from the system dialog must not replace it.
        |ui| ok_cancel(ui, language, true, false),
    )
    .1
}

/// `path`, or `path` with the first of `exts` added when it has none of them.
fn with_extension(path: PathBuf, exts: &[&str]) -> PathBuf {
    let known = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)));
    match exts.first() {
        Some(first) if !known => {
            let mut name = path.into_os_string();
            name.push(format!(".{first}"));
            name.into()
        }
        _ => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lay out a window `size` big with a body far taller and wider than it; returns
    /// the OK button's rect, the screen and the size the body needs.
    fn lay_out(size: Vec2, pages: bool) -> (egui::Rect, egui::Rect, Vec2) {
        let ctx = Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let mut ok = egui::Rect::NOTHING;
        let mut needed = Vec2::ZERO;
        // Panels settle their sizes over the first frames.
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                let (_, n) = window_parts(
                    ui,
                    "test",
                    pages.then_some(|ui: &mut Ui| {
                        for i in 0..30 {
                            ui.label(format!("page {i}"));
                        }
                    }),
                    Some(|ui: &mut Ui| ok = ui.button("OK").rect),
                    |ui, _| {
                        for i in 0..80 {
                            ui.label(format!("line {i} {}", "x".repeat(120)));
                        }
                    },
                );
                needed = n;
            });
            out.textures_delta.clear();
        }
        (ok, screen, needed)
    }

    /// The OK button stays in the window however much the form holds.
    #[test]
    fn buttons_stay_visible_in_a_small_window() {
        for pages in [false, true] {
            for size in [[220.0, 140.0], [520.0, 300.0], [900.0, 700.0]] {
                let (ok, screen, needed) = lay_out(Vec2::from(size), pages);
                assert!(ok.is_positive(), "{size:?}");
                assert!(screen.contains_rect(ok), "{ok:?} outside {screen:?}");
                // The body was measured larger than the window, so it scrolls.
                assert!(needed.y > screen.height(), "{needed:?}");
            }
        }
    }

    /// egui draws a pass again when a widget asks to discard the first one (a grid
    /// measuring its columns does); a dialog drawn twice in one frame must not panic.
    /// Double-clicking a QSO crashed here before.
    #[test]
    fn a_dialog_survives_a_discarded_pass() {
        let ctx = Context::default();
        let mut open = true;
        let mut passes = 0;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            dialog_with_buttons(
                ui.ctx(),
                "discard",
                "Discard",
                [200.0, 120.0],
                &mut open,
                |ui| {
                    passes += 1;
                    if passes == 1 {
                        ui.ctx().request_discard("test");
                    }
                },
                |ui| ok_cancel(ui, "en", true, false),
            );
        });
        out.textures_delta.clear();
        assert!(passes >= 2, "drawn {passes} time(s)");
    }

    #[test]
    fn windows_grow_to_fit_but_never_beyond_the_monitor() {
        let v = Vec2::new;
        assert_eq!(grown_size(v(400.0, 300.0), v(300.0, 200.0), None), None);
        assert_eq!(
            grown_size(v(400.0, 300.0), v(380.0, 520.0), None),
            Some(v(400.0, 520.0))
        );
        assert_eq!(
            grown_size(v(400.0, 300.0), v(3000.0, 3000.0), Some(v(1000.0, 800.0))),
            Some(v(900.0, 720.0))
        );
        assert_eq!(modal_body_height(100.0), 40.0);
    }

    #[test]
    fn log_extension_is_added_only_when_missing() {
        let p = |s: &str| with_extension(PathBuf::from(s), &["zlox", "zlo"]);
        assert_eq!(p("a.zlo"), PathBuf::from("a.zlo"));
        assert_eq!(p("a.ZLOX"), PathBuf::from("a.ZLOX"));
        assert_eq!(p("contest"), PathBuf::from("contest.zlox"));
        assert_eq!(p("allja.2026"), PathBuf::from("allja.2026.zlox"));
        assert_eq!(with_extension(PathBuf::from("x"), &[]), PathBuf::from("x"));
    }

    /// Regression G7: a name typed without extension got one after the system
    /// dialog, so an existing file of that name was replaced without asking.
    #[test]
    fn an_existing_file_reached_by_the_added_extension_is_asked_about() {
        let dir = std::env::temp_dir().join(format!("zlog-saveto-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.zlox"), b"x").unwrap();
        let to = SaveTo::new(dir.join("a"), &["zlox"]);
        assert_eq!(to.path, dir.join("a.zlox"));
        assert!(to.ask);
        // Typed in full: the system dialog asked already.
        assert!(!SaveTo::new(dir.join("a.zlox"), &["zlox"]).ask);
        assert!(!SaveTo::new(dir.join("b"), &["zlox"]).ask);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn filters_name_their_extensions_in_both_cases() {
        let (label, exts) = filter("QXML", &["xml", "qxml"]);
        assert_eq!(label, "QXML (*.xml, *.qxml)");
        assert_eq!(exts, ["xml", "XML", "qxml", "QXML"]);
        let logs = log_filters(true);
        assert_eq!(logs[0].0, "zLog (*.zlox, *.zlo)");
        assert_eq!(logs[1].0, "ZLOX (*.zlox)");
        assert_eq!(logs[2].1, ["zlo", "ZLO"]);
        assert_eq!(log_filters(false).len(), 2);
    }

    /// Regression C4: a name typed in the ZLO dialog without extension became .zlox.
    #[test]
    fn the_chosen_format_gives_the_extension() {
        let p = |s: &str, f| with_extension(PathBuf::from(s), &log_exts(f));
        assert_eq!(p("contest", Format::Zlo), PathBuf::from("contest.zlo"));
        assert_eq!(p("contest", Format::Zlox), PathBuf::from("contest.zlox"));
        assert_eq!(p("a.ZLOX", Format::Zlo), PathBuf::from("a.ZLOX"));
        assert_eq!(p("a.2026", Format::Zlo), PathBuf::from("a.2026.zlo"));
    }

    #[test]
    fn suggested_name_takes_the_formats_extension() {
        assert_eq!(suggested_name("allja.zlox", Format::Zlo), "allja.zlo");
        assert_eq!(suggested_name("allja.ZLO", Format::Zlox), "allja.zlox");
        assert_eq!(suggested_name("allja.zlo", Format::Zlo), "allja.zlo");
        assert_eq!(suggested_name("allja", Format::Zlo), "allja");
        assert_eq!(suggested_name("notes.txt", Format::Zlo), "notes.txt");
        assert_eq!(suggested_name("", Format::Zlox), "");
    }
}
