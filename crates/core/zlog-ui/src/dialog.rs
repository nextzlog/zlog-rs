use std::path::PathBuf;

use zlog_core::Format;

use egui::{Context, Pos2, Rect, Ui, Vec2, ViewportBuilder, ViewportCommand, ViewportId};

pub fn viewport_id(id: &str) -> ViewportId {
    ViewportId::from_hash_of(id)
}

fn on_top_id() -> egui::Id {
    egui::Id::unique("dialogs_on_top")
}

pub fn set_keep_on_top(ctx: &Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(on_top_id(), on));
}

pub fn focus(ctx: &Context, id: &str) {
    ctx.send_viewport_cmd_to(viewport_id(id), ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd_to(viewport_id(id), ViewportCommand::Focus);
}

const MIN_SIZE: [f32; 2] = [200.0, 120.0];

const CASCADE: f32 = 28.0;

fn place_id(id: &str) -> egui::Id {
    egui::Id::unique(("dialog_place", id))
}

fn pending_id(id: &str) -> egui::Id {
    egui::Id::unique(("dialog_place_pending", id))
}

pub fn place(ctx: &Context, id: &str, rect: Rect) {
    let pending = ctx.data(|d| d.get_temp::<bool>(pending_id(id)).is_some());
    let stored = ctx.data(|d| d.get_temp::<Rect>(place_id(id)));
    let rect = match stored {
        Some(_) if !pending => return,
        Some(r) if r != rect => {
            ctx.data_mut(|d| {
                d.insert_temp(place_id(id), rect);
                d.remove::<bool>(pending_id(id));
            });
            return;
        }
        Some(r) => r,
        None => rect,
    };
    let (main, monitor) = root_screen(ctx);
    let clamped = clamp(rect, main, monitor);
    ctx.data_mut(|d| {
        d.insert_temp(place_id(id), clamped.unwrap_or(rect));
        match clamped {
            Some(_) => d.remove::<bool>(pending_id(id)),
            None => {
                d.insert_temp(pending_id(id), true);
            }
        }
    });
}

fn root_screen(ctx: &Context) -> (Option<Rect>, Option<Vec2>) {
    ctx.input_for(ViewportId::ROOT, |i| {
        let v = i.viewport();
        (v.outer_rect, v.monitor_size)
    })
}

pub fn restored(ctx: &Context, rect: Rect) -> Rect {
    let (main, monitor) = root_screen(ctx);
    on_screen(rect, main, monitor)
}

const TITLE_STRIP: Vec2 = Vec2::new(48.0, 32.0);

const NEIGHBOUR: f32 = 1.5;

pub fn on_screen(rect: Rect, main: Option<Rect>, monitor: Option<Vec2>) -> Rect {
    clamp(rect, main, monitor).unwrap_or(rect)
}

fn clamp(rect: Rect, main: Option<Rect>, monitor: Option<Vec2>) -> Option<Rect> {
    let monitor = monitor.filter(|m| m.x > 0.0 && m.y > 0.0)?;
    let origin = visible_origin(main, monitor);
    let fits = rect.is_positive()
        && rect.is_finite()
        && rect.height() <= monitor.y
        && rect.width() <= monitor.x * 2.0;
    let main = main.unwrap_or(Rect::from_min_size(Pos2::ZERO, monitor));
    let (a, b) = (main.max - monitor, main.min + monitor);
    let home = Rect::from_min_max(a.min(b), a.max(b));
    let strip = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + TITLE_STRIP.y));
    let reachable = |beside: Vec2| {
        let s = strip.intersect(home.expand2(beside));
        s.width() >= TITLE_STRIP.x && s.height() >= TITLE_STRIP.y
    };
    if fits
        && (reachable(monitor * Vec2::new(NEIGHBOUR, 1.0))
            || reachable(monitor * Vec2::new(1.0, NEIGHBOUR)))
    {
        return Some(rect);
    }
    let size = if rect.is_finite() {
        rect.size()
    } else {
        monitor
    };
    let room = (monitor - BESIDE_MAIN).max(TITLE_STRIP);
    Some(Rect::from_min_size(origin, size.max(TITLE_STRIP).min(room)))
}

const BESIDE_MAIN: Vec2 = Vec2::new(48.0, 64.0);

fn visible_origin(main: Option<Rect>, monitor: Vec2) -> Pos2 {
    let origin = cascade_origin(main);
    let Some(main) = main else {
        return origin;
    };
    let seen = main.intersect(Rect::from_min_size(Pos2::ZERO, monitor));
    if seen.width() < TITLE_STRIP.x || seen.height() < TITLE_STRIP.y {
        return origin;
    }
    origin.clamp(seen.min, seen.max - TITLE_STRIP)
}

fn cascade_origin(main: Option<Rect>) -> Pos2 {
    main.map_or(Pos2::new(40.0, 40.0), |m| m.min + BESIDE_MAIN)
}

pub fn place_of(outer: Option<Rect>, inner: Option<Rect>) -> Option<Rect> {
    let inner = inner?;
    Some(Rect::from_min_size(
        outer.unwrap_or(inner).min,
        inner.size(),
    ))
}

pub fn geometry(ctx: &Context, id: &str) -> Option<Rect> {
    ctx.input_for(viewport_id(id), |i| {
        let v = i.viewport();
        v.outer_rect?;
        place_of(v.outer_rect, v.inner_rect)
    })
}

fn opening_rect(ctx: &Context, id: &str, size: Vec2) -> Rect {
    let main = ctx.input_for(ViewportId::ROOT, |i| i.viewport().outer_rect);
    ctx.data_mut(|d| {
        if let Some(r) = d.get_temp::<Rect>(place_id(id)) {
            return r;
        }
        let n = d.get_temp_mut_or_default::<u32>(egui::Id::unique("dialog_cascade"));
        let step = (*n % 8) as f32 * CASCADE;
        *n += 1;
        let r = Rect::from_min_size(cascade_origin(main) + Vec2::splat(step), size);
        d.insert_temp(place_id(id), r);
        r
    })
}

pub const OPEN_SIZE: [f32; 2] = [480.0, 320.0];

pub const MODAL_WIDTH: f32 = 460.0;

const FIT_FRAME: u8 = 2;

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

pub fn ok_cancel(ui: &mut Ui, language: &str, ok_enabled: bool, keys: bool) -> Option<bool> {
    let mut pressed = None;
    let text = |key| crate::texts().get(language, key);
    crate::form::buttons(ui, |ui| {
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

pub fn confirm_row(ui: &mut Ui, question: &str, yes: &str, no: &str) -> Option<bool> {
    let mut pressed = None;
    crate::form::buttons(ui, |ui| {
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
            let out = egui::ScrollArea::both()
                .id_salt(("modal_body", id))
                .max_height(room)
                .show(ui, body);
            scroll_bars(ui, &out, egui::Vec2b::TRUE);
            let r = out.inner;
            ui.separator();
            (r, buttons(ui))
        })
        .inner
}

pub fn scroll_bars<R>(ui: &Ui, out: &egui::scroll_area::ScrollAreaOutput<R>, enabled: egui::Vec2b) {
    use egui::accesskit::{Node, Orientation, Role};
    let (inner, content) = (out.inner_rect, out.content_size);
    let bar = ui.spacing().scroll.allocated_width();
    let offset = out.state.clamped_offset();
    let bars = [
        (
            Orientation::Horizontal,
            "Horizontal scroll bar",
            enabled.x && inner.width().ceil() < content.x,
            Rect::from_x_y_ranges(inner.x_range(), inner.bottom()..=inner.bottom() + bar),
            offset.x,
            content.x - inner.width(),
        ),
        (
            Orientation::Vertical,
            "Vertical scroll bar",
            enabled.y && inner.height().ceil() < content.y,
            Rect::from_x_y_ranges(inner.right()..=inner.right() + bar, inner.y_range()),
            offset.y,
            content.y - inner.height(),
        ),
    ];
    for (orientation, label, shown, rect, at, max) in bars {
        if !shown {
            continue;
        }
        let id = out.id.with(label);
        ui.interact(rect, id, egui::Sense::empty());
        ui.ctx().accesskit_node_builder(id, |n: &mut Node| {
            n.set_role(Role::ScrollBar);
            n.set_orientation(orientation);
            n.set_label(label);
            n.set_bounds(egui::accesskit::Rect {
                x0: rect.min.x.into(),
                y0: rect.min.y.into(),
                x1: rect.max.x.into(),
                y1: rect.max.y.into(),
            });
            n.set_numeric_value(at.into());
            n.set_min_numeric_value(0.0);
            n.set_max_numeric_value(max.into());
        });
    }
}

fn modal_body_height(height: f32) -> f32 {
    (height - 120.0).max(40.0)
}

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
        ctx.data_mut(|d| {
            d.remove::<u8>(fit_id);
            d.remove::<Rect>(place_id(id));
        });
        return None;
    }
    let size = Vec2::new(size[0].max(OPEN_SIZE[0]), size[1].max(OPEN_SIZE[1]));
    let at = opening_rect(ctx, id, size);
    let builder = ViewportBuilder::default()
        .with_title(title)
        .with_position(at.min)
        .with_inner_size(at.size())
        .with_min_inner_size(MIN_SIZE)
        .with_resizable(true)
        .with_window_level(window_level(ctx));
    // egui may draw the window more than once in a frame (a pass discarded for
    // layout), so the parts are called again on every pass. Only the first pass
    // sees the input, so its result carries the clicks.
    let mut first = None;
    let mut closed = false;
    ctx.show_viewport_immediate(viewport_id(id), builder, |ui, class| {
        closed |= ui.ctx().input(|i| i.viewport().close_requested());
        let (result, needed) = window_parts(
            ui,
            id,
            pages.as_mut().map(|f| move |ui: &mut Ui| f(ui)),
            buttons.as_mut().map(|f| move |ui: &mut Ui| f(ui)),
            |ui, p| body(ui, p),
        );
        if class != egui::ViewportClass::EmbeddedWindow {
            fit(ui.ctx(), fit_id, needed);
        }
        first.get_or_insert(result);
    });
    if closed {
        *open = false;
    }
    first
}

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
    scroll_bars(ui, &out, egui::Vec2b::TRUE);
    let needed = window - out.inner_rect.size() + out.content_size;
    ((out.inner, b), needed)
}

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

fn grown_size(inner: Vec2, needed: Vec2, monitor: Option<Vec2>) -> Option<Vec2> {
    let mut size = inner.max(needed.ceil());
    if let Some(m) = monitor {
        size = size.min(m * 0.9).max(inner);
    }
    (size.x > inner.x + 0.5 || size.y > inner.y + 0.5).then_some(size)
}

const LOG_TYPES: &[(&str, &str)] = &[("ZLOX", "zlox"), ("ZLO", "zlo")];

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

fn log_filters(together: bool) -> Vec<(String, Vec<String>)> {
    let mut all = Vec::new();
    if together {
        let exts: Vec<&str> = LOG_TYPES.iter().map(|(_, e)| *e).collect();
        all.push(filter("zLog", &exts));
    }
    all.extend(LOG_TYPES.iter().map(|(n, e)| filter(n, &[e])));
    all
}

static NATIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn native() -> rfd::FileDialog {
    NATIVE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    rfd::FileDialog::new()
}

pub fn native_dialogs() -> usize {
    NATIVE.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(any(test, feature = "test-hooks"))]
thread_local! {
    static TEST_PICK: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(any(test, feature = "test-hooks"))]
pub fn set_test_pick(path: Option<PathBuf>) {
    TEST_PICK.with(|p| *p.borrow_mut() = path);
}

fn test_pick() -> Option<Option<PathBuf>> {
    #[cfg(any(test, feature = "test-hooks"))]
    return Some(TEST_PICK.with(|p| p.borrow().clone()));
    #[cfg(not(any(test, feature = "test-hooks")))]
    None
}

pub fn open_log_file() -> Option<PathBuf> {
    if let Some(p) = test_pick() {
        return p;
    }
    with_filters(native(), &log_filters(true)).pick_file()
}

fn log_exts(format: Format) -> [&'static str; 2] {
    match format {
        Format::Zlox => ["zlox", "zlo"],
        Format::Zlo => ["zlo", "zlox"],
    }
}

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

pub fn choose_log_file(name: &str, format: Format) -> Option<SaveTo> {
    let exts = log_exts(format);
    if let Some(p) = test_pick() {
        return p.map(|p| SaveTo::new(p, &exts));
    }
    let (label, _) = LOG_TYPES
        .iter()
        .find(|(_, e)| *e == exts[0])
        .expect("log type");
    let dialog = native().set_file_name(suggested_name(name, format));
    with_filters(dialog, &[filter(label, &exts[..1])])
        .save_file()
        .map(|p| SaveTo::new(p, &exts))
}

pub fn pick_folder(start: &str) -> Option<PathBuf> {
    if let Some(p) = test_pick() {
        return p;
    }
    let mut dialog = native();
    if !start.is_empty() && std::path::Path::new(start).is_dir() {
        dialog = dialog.set_directory(start);
    }
    dialog.pick_folder()
}

pub fn import_file(name: &str, exts: &[&str]) -> Option<PathBuf> {
    if let Some(p) = test_pick() {
        return p;
    }
    with_filters(native(), &[filter(name, exts)]).pick_file()
}

pub fn import_files() -> Vec<PathBuf> {
    if let Some(p) = test_pick() {
        return p.into_iter().collect();
    }
    native().pick_files().unwrap_or_default()
}

pub fn import_files_of(name: &str, exts: &[&str]) -> Vec<PathBuf> {
    if let Some(p) = test_pick() {
        return p.into_iter().collect();
    }
    with_filters(native(), &[filter(name, exts)])
        .pick_files()
        .unwrap_or_default()
}

pub fn save_file_as(file_name: &str) -> Option<PathBuf> {
    if let Some(p) = test_pick() {
        return p;
    }
    native().set_file_name(file_name).save_file()
}

pub fn export_file(name: &str, exts: &[&str]) -> Option<SaveTo> {
    export_file_named(name, exts, "")
}

pub fn export_file_named(name: &str, exts: &[&str], log_path: &str) -> Option<SaveTo> {
    if let Some(p) = test_pick() {
        return p.map(|p| SaveTo::new(p, exts));
    }
    let mut dialog = with_filters(native(), &[filter(name, exts)]);
    if let Some(file) = export_name(log_path, exts.first().copied().unwrap_or("")) {
        dialog = dialog.set_file_name(file);
        let dir = std::path::Path::new(log_path).parent();
        if let Some(dir) = dir.filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
    }
    dialog.save_file().map(|p| SaveTo::new(p, exts))
}

pub fn export_name(log_path: &str, ext: &str) -> Option<String> {
    let stem = std::path::Path::new(log_path).file_stem()?.to_str()?;
    if stem.is_empty() {
        return None;
    }
    let ext = ext.trim_start_matches('.');
    Some(if ext.is_empty() {
        stem.to_string()
    } else {
        format!("{stem}.{ext}")
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveTo {
    pub path: PathBuf,
    pub ask: bool,
}

impl SaveTo {
    fn new(picked: PathBuf, exts: &[&str]) -> Self {
        let path = with_extension(picked.clone(), exts);
        let ask = path != picked && path.exists();
        SaveTo { path, ask }
    }
}

pub fn ask_overwrite(ctx: &Context, question: &str, language: &str) -> Option<bool> {
    modal_with_buttons(
        ctx,
        "overwrite",
        |ui| {
            ui.label(question);
        },
        |ui| ok_cancel(ui, language, true, false),
    )
    .1
}

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

pub fn port_label<'a>(port: &'a str, language: &str) -> &'a str {
    match port {
        zlog_serial::SIMULATOR => crate::texts().get(language, "device.simulator"),
        _ => port,
    }
}

pub fn settings_form(
    ui: &mut Ui,
    fields: &[crate::device::SettingField],
    section: &mut crate::Section,
    lang: &str,
) -> bool {
    settings_form_with(ui, fields, section, lang, |_| {})
}

pub fn settings_form_with(
    ui: &mut Ui,
    fields: &[crate::device::SettingField],
    section: &mut crate::Section,
    lang: &str,
    detail: impl FnOnce(&mut Ui),
) -> bool {
    let (advanced, common): (Vec<_>, Vec<_>) = fields.iter().partition(|f| f.advanced);
    let labels: Vec<&str> = fields.iter().map(|f| f.label.as_str()).collect();
    let rows = Rows {
        labels: &labels,
        all: fields,
        lang,
    };
    let mut changed = rows.show(ui, "settings_form", &common, section);
    if advanced.is_empty() {
        detail(ui);
        return changed;
    }
    let title = crate::texts().get(lang, "settings.advanced");
    egui::CollapsingHeader::new(title)
        .id_salt("settings_form_advanced")
        .show(ui, |ui| {
            changed |= rows.show(ui, "settings_form_advanced", &advanced, section);
            detail(ui);
        });
    changed
}

pub fn pick_file(filters: &[(String, Vec<String>)]) -> Option<PathBuf> {
    if let Some(p) = test_pick() {
        return p;
    }
    let filters: Vec<_> = filters
        .iter()
        .map(|(name, exts)| {
            let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
            filter(name, &exts)
        })
        .collect();
    with_filters(native(), &filters).pick_file()
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

fn file_field(
    ui: &mut Ui,
    v: &mut String,
    filters: &[(String, Vec<String>)],
    width: f32,
    lang: &str,
) {
    let t = |k| crate::texts().get(lang, k);
    ui.horizontal(|ui| {
        ui.set_max_width(width);
        if ui.button(t("settings.open")).clicked() {
            if let Some(p) = pick_file(filters) {
                *v = p.display().to_string();
            }
        }
        if v.is_empty() {
            ui.weak(t("settings.no_file"));
            return;
        }
        if ui
            .small_button("\u{00D7}")
            .on_hover_text(t("settings.clear"))
            .clicked()
        {
            v.clear();
            return;
        }
        ui.add(egui::Label::new(file_name(v)).truncate())
            .on_hover_text(v.as_str());
    });
}

struct Rows<'a> {
    labels: &'a [&'a str],
    all: &'a [crate::device::SettingField],
    lang: &'a str,
}

impl Rows<'_> {
    fn label_of(&self, key: &str, value: &str) -> String {
        use crate::device::SettingKind;
        let choices = self.all.iter().filter(|f| f.key == key);
        let mut labels = choices.filter_map(|f| match &f.kind {
            SettingKind::Choice(c) => c.iter().find(|(c, _)| c == value).map(|(_, l)| l.clone()),
            _ => None,
        });
        labels.next().unwrap_or_else(|| value.to_string())
    }

    fn show(
        &self,
        ui: &mut Ui,
        id_salt: &'static str,
        fields: &[&crate::device::SettingField],
        section: &mut crate::Section,
    ) -> bool {
        use crate::device::{SettingKind, BAUDS};
        let lang = self.lang;
        let mut changed = false;
        crate::form(ui, id_salt, self.labels, |ui, form| {
            let width = form.width();
            for f in fields {
                ui.label(&f.label);
                let old = f.value(section).to_string();
                let mut v = old.clone();
                let salt = (id_salt, f.key, &f.label);
                match &f.kind {
                    SettingKind::Text => {
                        crate::sized_field(ui, egui::TextEdit::singleline(&mut v), width);
                    }
                    SettingKind::File { filters } => file_field(ui, &mut v, filters, width, lang),
                    SettingKind::Port(found) => {
                        ui.horizontal(|ui| {
                            let pick = ui.spacing().interact_size.y + ui.spacing().item_spacing.x;
                            let edit = egui::TextEdit::singleline(&mut v);
                            crate::sized_field(ui, edit, (width - pick).max(0.0));
                            egui::ComboBox::from_id_salt(salt)
                                .selected_text("")
                                .width(0.0)
                                .show_ui(ui, |ui| {
                                    for p in found {
                                        ui.selectable_value(&mut v, p.clone(), port_label(p, lang));
                                    }
                                });
                        });
                    }
                    SettingKind::Baud => {
                        form.fit(ui, |ui| {
                            form.combo(salt).selected_text(v.clone()).show_ui(ui, |ui| {
                                for b in BAUDS {
                                    ui.selectable_value(&mut v, b.to_string(), b.to_string());
                                }
                            })
                        });
                    }
                    SettingKind::Choice(choices) => {
                        let shown = self.label_of(f.key, &v);
                        form.fit(ui, |ui| {
                            form.combo(salt).selected_text(shown).show_ui(ui, |ui| {
                                for (c, l) in choices {
                                    ui.selectable_value(&mut v, c.clone(), l);
                                }
                            })
                        });
                    }
                    SettingKind::Bool => {
                        let mut on = v == "true";
                        ui.checkbox(&mut on, "");
                        v = on.to_string();
                    }
                    SettingKind::Int { min, max } => {
                        let mut n: i64 = v.parse().unwrap_or(*min).clamp(*min, *max);
                        let drag = egui::DragValue::new(&mut n).range(*min..=*max);
                        ui.add(drag);
                        v = n.to_string();
                    }
                }
                if v != old {
                    section.insert(f.key.into(), v);
                    changed = true;
                }
                ui.end_row();
            }
        });
        changed
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn an_export_suggests_the_name_of_the_log() {
        assert_eq!(
            export_name("/logs/JA1ZLO-allja.zlox", "csv").as_deref(),
            Some("JA1ZLO-allja.csv")
        );
        assert_eq!(export_name("a.zlo", ".adi").as_deref(), Some("a.adi"));
        assert_eq!(export_name("a.zlo", "").as_deref(), Some("a"));
        assert_eq!(export_name("", "csv"), None);
    }
    use super::*;

    fn lay_out(size: Vec2, pages: bool) -> (egui::Rect, egui::Rect, Vec2) {
        let ctx = Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let mut ok = egui::Rect::NOTHING;
        let mut needed = Vec2::ZERO;
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

    #[test]
    fn buttons_stay_visible_in_a_small_window() {
        for pages in [false, true] {
            for size in [[220.0, 140.0], [520.0, 300.0], [900.0, 700.0]] {
                let (ok, screen, needed) = lay_out(Vec2::from(size), pages);
                assert!(ok.is_positive(), "{size:?}");
                assert!(screen.contains_rect(ok), "{ok:?} outside {screen:?}");
                assert!(needed.y > screen.height(), "{needed:?}");
            }
        }
    }

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
    fn a_native_dialog_keeps_the_input_of_a_discarded_pass() {
        let ctx = Context::default();
        ctx.set_embed_viewports(false);
        Context::set_immediate_viewport_renderer(|ctx, mut v| {
            let input = egui::RawInput {
                viewport_id: v.ids.this,
                viewports: std::iter::once((v.ids.this, Default::default())).collect(),
                events: vec![egui::Event::Text("x".into())],
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| (v.viewport_ui_cb)(ui));
            out.textures_delta.clear();
        });
        let mut open = true;
        let mut passes = 0;
        let mut seen = None;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            seen = dialog_with_buttons(
                ui.ctx(),
                "native",
                "Native",
                [200.0, 120.0],
                &mut open,
                |ui| {
                    passes += 1;
                    if passes == 1 {
                        ui.ctx().request_discard("test");
                    }
                },
                |ui| ui.input(|i| !i.events.is_empty()),
            )
            .map(|(_, b)| b);
        });
        out.textures_delta.clear();
        assert_eq!(passes, 2);
        assert_eq!(seen, Some(true));
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

    #[test]
    fn an_existing_file_reached_by_the_added_extension_is_asked_about() {
        let dir = std::env::temp_dir().join(format!("zlog-saveto-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.zlox"), b"x").unwrap();
        let to = SaveTo::new(dir.join("a"), &["zlox"]);
        assert_eq!(to.path, dir.join("a.zlox"));
        assert!(to.ask);
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

    #[test]
    fn the_simulator_port_is_named_in_the_language() {
        assert_eq!(port_label(zlog_serial::SIMULATOR, "en"), "Simulator");
        assert_eq!(port_label(zlog_serial::SIMULATOR, "ja"), "シミュレーター");
        assert_eq!(port_label("COM3", "ja"), "COM3");
    }

    #[test]
    fn settings_form_draws_every_kind_and_writes_only_changes() {
        use crate::device::{SettingField, SettingKind};
        let fields = vec![
            SettingField::new("port", "Port", SettingKind::Port(vec!["COM3".into()])),
            SettingField::new("baud", "Speed", SettingKind::Baud).default_value("9600"),
            SettingField::new(
                "model",
                "Model",
                SettingKind::Choice(vec![("wk".into(), "WinKeyer".into())]),
            )
            .default_value("wk"),
            SettingField::new("ptt", "PTT", SettingKind::Bool).default_value("true"),
            SettingField::new("wpm", "Speed (WPM)", SettingKind::Int { min: 5, max: 60 })
                .default_value("25"),
            SettingField::new("name", "Name", SettingKind::Text),
        ];
        let ctx = Context::default();
        let mut section = crate::Section::new();
        let mut changed = true;
        let mut width = 0.0;
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(600.0, 400.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                changed = settings_form(ui, &fields, &mut section, "ja");
                width = ui.min_rect().width();
            });
            out.textures_delta.clear();
        }
        assert!(!changed, "defaults are not written");
        assert!(section.is_empty());
        assert!(width > 300.0, "the fields stretch with the window: {width}");
        section.insert("wpm".into(), "99".into());
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            changed = settings_form(ui, &fields, &mut section, "ja");
        });
        out.textures_delta.clear();
        assert!(changed);
        assert_eq!(section["wpm"], "60");
    }

    #[test]
    fn restored_window_rects_are_kept_on_the_screen() {
        let r =
            |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h));
        let fhd = Some(Vec2::new(1920.0, 1080.0));
        let qhd = Some(Vec2::new(2560.0, 1440.0));
        let portrait = Some(Vec2::new(1080.0, 1920.0));
        let main = Some(r(100.0, 100.0, 1000.0, 700.0));
        let on_qhd_right = Some(r(2000.0, 100.0, 1000.0, 700.0));
        let on_portrait = Some(r(50.0, 1200.0, 900.0, 700.0));
        let kept = [
            ("same monitor", r(400.0, 300.0, 300.0, 200.0), main, fhd),
            (
                "second monitor right",
                r(2500.0, 300.0, 400.0, 300.0),
                main,
                fhd,
            ),
            (
                "second monitor left",
                r(-1500.0, 300.0, 400.0, 300.0),
                main,
                fhd,
            ),
            ("monitor above", r(300.0, -900.0, 400.0, 300.0), main, fhd),
            (
                "spanning two monitors",
                r(100.0, 100.0, 3000.0, 900.0),
                main,
                fhd,
            ),
            (
                "smaller monitor left",
                r(200.0, 200.0, 400.0, 300.0),
                on_qhd_right,
                qhd,
            ),
            (
                "same larger monitor",
                r(4500.0, 1000.0, 400.0, 300.0),
                on_qhd_right,
                qhd,
            ),
            (
                "portrait below",
                r(60.0, 1300.0, 400.0, 300.0),
                on_portrait,
                portrait,
            ),
            (
                "landscape above portrait",
                r(900.0, 200.0, 400.0, 300.0),
                on_portrait,
                portrait,
            ),
            (
                "title strip on the next monitor",
                r(-1950.0, 300.0, 400.0, 300.0),
                main,
                fhd,
            ),
            (
                "no monitor size",
                r(9000.0, 9000.0, 300.0, 200.0),
                main,
                None,
            ),
        ];
        for (case, saved, main, monitor) in kept {
            assert_eq!(on_screen(saved, main, monitor), saved, "{case}");
        }
        let moved = [
            (
                "three monitors to the left",
                r(-5700.0, 300.0, 400.0, 300.0),
                main,
                fhd,
                r(148.0, 164.0, 400.0, 300.0),
            ),
            (
                "removed monitor far right",
                r(9000.0, 300.0, 300.0, 200.0),
                main,
                fhd,
                r(148.0, 164.0, 300.0, 200.0),
            ),
            (
                "removed monitor far left",
                r(-6000.0, 300.0, 300.0, 200.0),
                main,
                fhd,
                r(148.0, 164.0, 300.0, 200.0),
            ),
            (
                "title bar above every screen",
                r(300.0, -3500.0, 300.0, 400.0),
                main,
                fhd,
                r(148.0, 164.0, 300.0, 400.0),
            ),
            (
                "too large",
                r(9000.0, 9000.0, 4000.0, 3000.0),
                main,
                fhd,
                r(148.0, 164.0, 1872.0, 1016.0),
            ),
            (
                "no main window",
                r(9000.0, 9000.0, 300.0, 200.0),
                None,
                fhd,
                r(40.0, 40.0, 300.0, 200.0),
            ),
            (
                "removed below portrait",
                r(60.0, 9000.0, 300.0, 200.0),
                on_portrait,
                portrait,
                r(98.0, 1264.0, 300.0, 200.0),
            ),
        ];
        for (case, saved, main, monitor, want) in moved {
            assert_eq!(on_screen(saved, main, monitor), want, "{case}");
        }
    }

    #[test]
    fn oversized_or_inverted_windows_are_refitted_onto_the_screen() {
        let main = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0)));
        let fhd = Vec2::new(1920.0, 1080.0);
        let r = |x: f32, y: f32, w: f32, h: f32| {
            Rect::from_min_max(Pos2::new(x, y), Pos2::new(x + w, y + h))
        };
        for saved in [
            r(-500.0, -300.0, 3000.0, 2000.0),
            r(-100.0, 20.0, 1000.0, 1500.0),
            r(-4000.0, -40.0, 4100.0, 900.0),
            r(300.0, 300.0, -200.0, -100.0),
            r(f32::NEG_INFINITY, 0.0, 400.0, 300.0),
        ] {
            let got = on_screen(saved, main, Some(fhd));
            let screen = Rect::from_min_size(Pos2::ZERO, fhd);
            assert!(screen.contains_rect(got), "{saved:?} -> {got:?}");
            assert!(got.width() >= TITLE_STRIP.x && got.height() >= TITLE_STRIP.y);
        }
    }

    #[test]
    fn windows_beyond_the_adjacent_monitors_move_beside_the_main_window() {
        let main = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0)));
        let fhd = Some(Vec2::new(1920.0, 1080.0));
        let size = Vec2::new(400.0, 300.0);
        let beside = Rect::from_min_size(Pos2::new(48.0, 64.0), size);
        for at in [
            Pos2::new(100.0, -2500.0),
            Pos2::new(5000.0, 100.0),
            Pos2::new(-4000.0, 100.0),
            Pos2::new(100.0, 3500.0),
            Pos2::new(4500.0, -2000.0),
            Pos2::new(-3000.0, 2500.0),
        ] {
            let saved = Rect::from_min_size(at, size);
            assert_eq!(on_screen(saved, main, fhd), beside, "{at:?}");
        }
    }

    #[test]
    fn focusing_a_floating_window_restores_it_from_minimized() {
        let ctx = Context::default();
        ctx.set_embed_viewports(false);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.ctx().show_viewport_deferred(
                viewport_id("w"),
                ViewportBuilder::default(),
                |_, _| {},
            );
            focus(ui.ctx(), "w");
        });
        out.textures_delta.clear();
        let sent = out
            .viewport_output
            .get(&viewport_id("w"))
            .map(|v| v.commands.clone())
            .unwrap_or_default();
        assert_eq!(
            sent,
            [ViewportCommand::Minimized(false), ViewportCommand::Focus]
        );
    }

    #[test]
    fn scroll_bar_nodes_match_the_bars_egui_shows() {
        let sizes = [
            (Vec2::new(200.0, 100.0), Vec2::new(150.0, 80.0)),
            (Vec2::new(200.0, 100.0), Vec2::new(400.0, 80.0)),
            (Vec2::new(200.0, 100.0), Vec2::new(150.0, 300.0)),
            (Vec2::new(200.0, 100.0), Vec2::new(400.0, 300.0)),
            (Vec2::new(200.4, 100.4), Vec2::new(200.6, 100.6)),
            (Vec2::new(200.4, 100.4), Vec2::new(201.2, 101.2)),
            (Vec2::new(200.6, 100.6), Vec2::new(200.9, 100.9)),
            (Vec2::new(200.0, 100.0), Vec2::new(200.0, 100.0)),
            (Vec2::new(200.0, 100.0), Vec2::new(200.5, 99.5)),
            (Vec2::new(199.5, 99.5), Vec2::new(199.75, 100.25)),
        ];
        for (area, content) in sizes {
            let ctx = Context::default();
            ctx.enable_accesskit();
            let mut shown = [false; 2];
            let mut nodes = Vec::new();
            for _ in 0..3 {
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let out = ui
                        .allocate_ui(area, |ui| {
                            egui::ScrollArea::both()
                                .auto_shrink(false)
                                .max_width(area.x)
                                .max_height(area.y)
                                .show(ui, |ui| {
                                    ui.allocate_exact_size(content, egui::Sense::hover())
                                })
                        })
                        .inner;
                    scroll_bars(ui, &out, egui::Vec2b::TRUE);
                    shown = [0usize, 1].map(|d| ui.ctx().read_response(out.id.with(d)).is_some());
                });
                out.textures_delta.clear();
                nodes = out
                    .platform_output
                    .accesskit_update
                    .map(|u| {
                        u.nodes
                            .into_iter()
                            .filter_map(|(_, n)| n.label().map(String::from))
                            .filter(|l| l.ends_with("scroll bar"))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
            }
            let reported = [
                nodes.iter().any(|l| l.starts_with("Horizontal")),
                nodes.iter().any(|l| l.starts_with("Vertical")),
            ];
            assert_eq!(reported, shown, "{area:?} {content:?}");
        }
    }

    #[test]
    fn place_clamps_a_saved_rect_once() {
        let ctx = Context::default();
        let mut input = egui::RawInput::default();
        input
            .viewports
            .entry(ViewportId::ROOT)
            .or_default()
            .monitor_size = Some(Vec2::new(800.0, 600.0));
        let saved = Rect::from_min_size(Pos2::new(5000.0, 5000.0), Vec2::new(200.0, 100.0));
        let mut out = ctx.run_ui(input, |ui| place(ui.ctx(), "w", saved));
        out.textures_delta.clear();
        let got = ctx.data(|d| d.get_temp::<Rect>(place_id("w")));
        assert_eq!(
            got,
            Some(Rect::from_min_size(
                Pos2::new(40.0, 40.0),
                Vec2::new(200.0, 100.0)
            ))
        );
    }

    #[test]
    fn a_dialog_above_left_of_an_offset_main_window_is_kept() {
        let main = Some(Rect::from_min_size(
            Pos2::new(600.0, 400.0),
            Vec2::new(800.0, 600.0),
        ));
        let fhd = Some(Vec2::new(1920.0, 1080.0));
        let saved = Rect::from_min_size(Pos2::new(50.0, 50.0), Vec2::new(300.0, 200.0));
        assert_eq!(on_screen(saved, main, fhd), saved);
    }

    #[test]
    fn a_dialog_low_on_a_taller_right_monitor_is_kept() {
        let main = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)));
        let fhd = Some(Vec2::new(1920.0, 1080.0));
        let saved = Rect::from_min_size(Pos2::new(2200.0, 1200.0), Vec2::new(300.0, 200.0));
        assert_eq!(on_screen(saved, main, fhd), saved);
    }

    #[test]
    fn diagonal_neighbours_are_kept_but_not_beyond_them() {
        let main = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)));
        let fhd = Some(Vec2::new(1920.0, 1080.0));
        let size = Vec2::new(300.0, 200.0);
        let kept = Rect::from_min_size(Pos2::new(2500.0, -900.0), size);
        assert_eq!(on_screen(kept, main, fhd), kept);
        let far = Rect::from_min_size(Pos2::new(4200.0, -2200.0), size);
        assert_eq!(
            on_screen(far, main, fhd),
            Rect::from_min_size(Pos2::new(48.0, 64.0), size)
        );
    }

    #[test]
    fn place_clamps_once_the_monitor_size_is_known() {
        let ctx = Context::default();
        let saved = Rect::from_min_size(Pos2::new(5000.0, 5000.0), Vec2::new(200.0, 100.0));
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| place(ui.ctx(), "w", saved));
        out.textures_delta.clear();
        let mut input = egui::RawInput::default();
        input
            .viewports
            .entry(ViewportId::ROOT)
            .or_default()
            .monitor_size = Some(Vec2::new(800.0, 600.0));
        let mut out = ctx.run_ui(input, |ui| place(ui.ctx(), "w", saved));
        out.textures_delta.clear();
        assert_eq!(
            ctx.data(|d| d.get_temp::<Rect>(place_id("w"))),
            Some(Rect::from_min_size(
                Pos2::new(40.0, 40.0),
                Vec2::new(200.0, 100.0)
            ))
        );
    }

    #[test]
    fn a_snapped_dialog_lands_on_the_visible_part_of_an_off_screen_main_window() {
        let main = Some(Rect::from_min_size(
            Pos2::new(-500.0, -300.0),
            Vec2::new(800.0, 600.0),
        ));
        let fhd = Some(Vec2::new(1920.0, 1080.0));
        let size = Vec2::new(300.0, 200.0);
        let lost = Rect::from_min_size(Pos2::new(9000.0, 9000.0), size);
        assert_eq!(
            on_screen(lost, main, fhd),
            Rect::from_min_size(Pos2::ZERO, size)
        );
        let far = Some(Rect::from_min_size(
            Pos2::new(-5000.0, 0.0),
            Vec2::new(800.0, 600.0),
        ));
        assert_eq!(
            on_screen(lost, far, fhd).min,
            Pos2::new(-5000.0 + 48.0, 64.0)
        );
    }

    #[test]
    fn a_dialog_moved_before_the_monitor_is_known_does_not_jump() {
        let ctx = Context::default();
        let saved = Rect::from_min_size(Pos2::new(5000.0, 5000.0), Vec2::new(200.0, 100.0));
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| place(ui.ctx(), "w", saved));
        out.textures_delta.clear();
        let moved = saved.translate(Vec2::new(-4800.0, -4800.0));
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| place(ui.ctx(), "w", moved));
        out.textures_delta.clear();
        let mut input = egui::RawInput::default();
        input
            .viewports
            .entry(ViewportId::ROOT)
            .or_default()
            .monitor_size = Some(Vec2::new(800.0, 600.0));
        let mut out = ctx.run_ui(input, |ui| place(ui.ctx(), "w", moved));
        out.textures_delta.clear();
        assert_eq!(ctx.data(|d| d.get_temp::<Rect>(place_id("w"))), Some(moved));
    }
}
