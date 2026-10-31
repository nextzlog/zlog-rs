mod windows;

use egui::accesskit::Role;
use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, Rect, Vec2};
use egui_kittest::kittest::NodeT;
use windows::*;

const HORIZONTAL: &str = "Horizontal scroll bar";

const VERTICAL: &str = "Vertical scroll bar";

struct Wide;

impl zlog_ui::Panel for Wide {
    fn id(&self) -> &str {
        "wide"
    }

    fn title(&self, _language: &str) -> String {
        "Wide".into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {
        ui.horizontal(|ui| {
            for i in 0..10 {
                ui.add_sized([90.0, 20.0], egui::Label::new(format!("wide{i}")));
            }
        });
    }
}

struct Form;

impl zlog_ui::Panel for Form {
    fn id(&self) -> &str {
        "form"
    }

    fn title(&self, _language: &str) -> String {
        "Form".into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {
        let width = ui.available_width().max(200.0);
        ui.add_sized([width, 20.0], egui::Button::new("Form field"));
    }
}

struct Tall {
    rows: usize,
}

impl zlog_ui::Panel for Tall {
    fn id(&self) -> &str {
        "tall"
    }

    fn title(&self, _language: &str) -> String {
        "Tall".into()
    }

    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::Dialog
    }

    fn ui(&mut self, ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) {
        let width = ui.available_width().max(200.0);
        for i in 0..self.rows {
            ui.add_sized([width, 20.0], egui::Button::new(format!("tall{i}")));
        }
    }

    fn capability_mut(&mut self, want: zlog_ui::WantMut) -> Option<zlog_ui::CapabilityMut<'_>> {
        match want {
            zlog_ui::WantMut::Buttons => Some(zlog_ui::CapabilityMut::Buttons(self)),
            _ => None,
        }
    }
}

impl zlog_ui::ButtonRow for Tall {
    fn buttons(&mut self, ui: &mut egui::Ui, _view: &zlog_ui::View<'_>) -> bool {
        ui.button("Done").clicked()
    }
}

fn init() {
    zlog_gui::testing::registry::install(zlog_std::register);
}

fn setup(env: &Env, dock: &str, size: Vec2, rows: usize) -> H {
    env.write_settings(&format!("[dock]\n{dock}\n"));
    let mut h = harness(env);
    h.state_mut().push_panel(Box::new(Wide));
    h.state_mut().push_panel(Box::new(Form));
    h.state_mut().push_panel(Box::new(Tall { rows }));
    resize(&mut h, size);
    h
}

fn resize(h: &mut H, size: Vec2) {
    h.set_size(size);
    for _ in 0..4 {
        settle(h);
    }
}

fn rect_of(n: &egui_kittest::Node<'_>) -> Rect {
    n.rect()
}

fn find(h: &H, role: Role, label: &str, inside: Option<&str>) -> Option<Rect> {
    h.root()
        .children_recursive()
        .find(|n| {
            let a = n.accesskit_node();
            a.role() == role
                && (a.label().as_deref() == Some(label) || a.value().as_deref() == Some(label))
                && window_of(n).as_deref() == inside
        })
        .map(|n| rect_of(&n))
}

fn bars(h: &H, label: &str, inside: Option<&str>) -> Vec<Rect> {
    h.root()
        .children_recursive()
        .filter(|n| {
            let a = n.accesskit_node();
            a.role() == Role::ScrollBar
                && a.label().as_deref() == Some(label)
                && window_of(n).as_deref() == inside
        })
        .map(|n| rect_of(&n))
        .collect()
}

fn log_view(h: &H) -> Rect {
    find(h, Role::ScrollView, "Log", None).expect("log view")
}

fn log_bar(h: &H) -> Option<Rect> {
    let view = log_view(h);
    bars(h, HORIZONTAL, None)
        .into_iter()
        .find(|b| (b.left() - view.left()).abs() < 1.0 && (b.top() - view.bottom()).abs() < 1.0)
}

fn header(h: &H, key: &str) -> Rect {
    find(h, Role::Label, tr(key), None).unwrap_or_else(|| panic!("no header {key}"))
}

fn entry_fields(h: &H) -> Vec<Rect> {
    let call = h
        .root()
        .children_recursive()
        .filter(|n| n.accesskit_node().role() == Role::TextInput && window_of(n).is_none())
        .map(|n| rect_of(&n))
        .min_by(|a, b| a.left().total_cmp(&b.left()))
        .expect("call field");
    let mut fields: Vec<Rect> = h
        .root()
        .children_recursive()
        .filter(|n| {
            matches!(n.accesskit_node().role(), Role::TextInput | Role::Button)
                && window_of(n).is_none()
                && (rect_of(n).center().y - call.center().y).abs() < 3.0
        })
        .map(|n| rect_of(&n))
        .collect();
    fields.sort_by(|a, b| a.left().total_cmp(&b.left()));
    assert!(
        fields.len() >= 8,
        "five inputs and three buttons {fields:?}"
    );
    fields
}

fn columns(h: &H) -> Vec<f32> {
    let call = header(h, "column.call").left();
    entry_fields(h)
        .iter()
        .map(|f| (f.left() - call).round())
        .collect()
}

fn wheel(h: &mut H, at: Pos2, delta: Vec2) {
    h.event(Event::PointerMoved(at));
    h.event(Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta,
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: Modifiers::NONE,
    });
    for _ in 0..20 {
        h.step();
    }
    settle(h);
}

fn scroll_log(h: &mut H, end: bool) {
    let view = log_view(h);
    let at = egui::pos2(view.center().x, view.bottom() - 4.0);
    let dx = if end { -10000.0 } else { 10000.0 };
    wheel(h, at, egui::vec2(dx, 0.0));
}

fn inside(outer: Rect, r: Rect) -> bool {
    r.left() >= outer.left() - 0.5 && r.right() <= outer.right() + 0.5
}

fn check_entry_reachable(h: &mut H, what: &str) {
    let before = columns(h);
    let view = log_view(h);
    let start = entry_fields(h);
    scroll_log(h, true);
    let end = entry_fields(h);
    assert_eq!(
        columns(h),
        before,
        "{what}: columns aligned after scrolling"
    );
    let shift = start[0].left() - end[0].left();
    for (a, b) in start.iter().zip(&end) {
        let need = (a.right() - view.right()).max(0.0);
        let room = shift.min(a.left() - view.left());
        assert!(
            need <= room + 0.5,
            "{what}: field {a:?} / {b:?} reachable in {view:?}"
        );
        assert!(a.width() >= 12.0, "{what}: field not squeezed {a:?}");
    }
    let pts = header(h, "column.points");
    if log_bar(h).is_some() {
        assert!(inside(view, pts), "{what}: last column {pts:?} in {view:?}");
    }
    scroll_log(h, false);
}

#[test]
fn every_layout_keeps_entry_fields_reachable_and_aligned() {
    init();
    let layouts = [
        ("right", "right = \"alpha\""),
        ("left", "left = \"alpha\""),
        ("bottom", "bottom = \"alpha\""),
        ("split", "right = \"alpha|beta\""),
    ];
    for size in [egui::vec2(960.0, 640.0), egui::vec2(700.0, 500.0)] {
        for (name, dock) in layouts {
            let env = Env::new(&format!("scroll-{name}-{}", size.x));
            let mut h = setup(&env, dock, size, 3);
            let alone = columns(&h);
            open(&mut h, "Alpha");
            if name == "split" {
                open(&mut h, "Beta");
            }
            resize(&mut h, size);
            assert!(tab(&h, "Alpha").is_some(), "{name}: docked");
            let what = format!("{name} at {size:?}");
            assert_eq!(columns(&h), alone, "{what}: columns");
            let view = log_view(&h);
            let need = entry_fields(&h).last().unwrap().right() - view.left();
            if view.width() >= need {
                for f in entry_fields(&h) {
                    assert!(inside(view, f), "{what}: {f:?} fully shown in {view:?}");
                }
            } else {
                assert!(log_bar(&h).is_some(), "{what}: shared scroll bar");
            }
            check_entry_reachable(&mut h, &what);
        }
    }
}

#[test]
fn the_log_scroll_bar_shows_only_when_the_grid_does_not_fit() {
    init();
    let env = Env::new("scroll-log");
    let mut h = setup(&env, "", egui::vec2(1600.0, 900.0), 3);
    assert!(log_bar(&h).is_none(), "wide window, no bar");
    assert!(bars(&h, VERTICAL, None).is_empty());
    let entry = log_view(&h).bottom();
    resize(&mut h, egui::vec2(600.0, 500.0));
    let bar = log_bar(&h).expect("narrow window shows the bar");
    let view = log_view(&h);
    let fields = entry_fields(&h);
    assert!(
        bar.top() >= fields.last().unwrap().bottom() - 0.5,
        "the bar sits below the entry row {bar:?} {fields:?}"
    );
    assert!(
        (bar.width() - view.width()).abs() < 1.0,
        "the bar spans the log"
    );
    let memo = header(&h, "column.memo").left();
    let memo_field = entry_fields(&h).last().unwrap().left();
    let pts = header(&h, "column.points");
    assert!(!inside(view, pts));
    scroll_log(&mut h, true);
    let pts = header(&h, "column.points");
    assert!(inside(view, pts), "the last column is reachable {pts:?}");
    assert!(view.contains(text_pos(&h, tr("column.points"))));
    let shift = header(&h, "column.memo").left() - memo;
    assert!(shift < -50.0, "scrolled {shift}");
    let last = *entry_fields(&h).last().unwrap();
    assert!(
        (last.left() - memo_field - shift).abs() < 1.0,
        "the last entry field moves with its column"
    );
    assert!(inside(view, last));
    resize(&mut h, egui::vec2(1600.0, 900.0));
    assert!(
        log_bar(&h).is_none(),
        "the bar goes away when it fits again"
    );
    assert_eq!(log_view(&h).bottom(), entry, "no gutter is kept");
}

#[test]
fn dragging_a_dock_edge_shows_and_hides_the_log_bar() {
    init();
    let env = Env::new("scroll-drag");
    let size = egui::vec2(1600.0, 900.0);
    let mut h = setup(&env, "right = \"alpha\"", size, 3);
    open(&mut h, "Alpha");
    resize(&mut h, size);
    assert!(log_bar(&h).is_none());
    let edge = egui::pos2(log_view(&h).right() + 2.0, 300.0);
    drag(&mut h, edge, egui::pos2(100.0, 300.0));
    let view = log_view(&h);
    assert!(
        (view.width() - 240.0).abs() < 2.0,
        "the area stops at the minimum log width {view:?}"
    );
    assert!(log_bar(&h).is_some(), "the grid no longer fits");
    check_entry_reachable(&mut h, "after the drag");
    assert!(
        env.settings().contains("right.size"),
        "the dragged size is kept"
    );
    let edge = egui::pos2(log_view(&h).right() + 2.0, 300.0);
    drag(&mut h, edge, egui::pos2(size.x - 200.0, 300.0));
    assert!(log_bar(&h).is_none(), "wide again");
}

fn wide_reaches_its_last_column(h: &mut H, inside_window: Option<&str>) {
    let bar = bars(h, HORIZONTAL, inside_window);
    assert_eq!(bar.len(), 1, "one bar {bar:?}");
    assert!(
        bars(h, VERTICAL, inside_window).is_empty(),
        "no vertical bar"
    );
    let first = find(h, Role::Label, "wide0", inside_window).unwrap();
    let last = find(h, Role::Label, "wide9", inside_window).unwrap();
    let bar = bar[0];
    assert!(last.right() > bar.right(), "clipped at first");
    wheel(h, first.center(), egui::vec2(-10000.0, 0.0));
    let last = find(h, Role::Label, "wide9", inside_window).unwrap();
    assert!(
        last.right() <= bar.right() + 0.5 && last.left() >= bar.left(),
        "the last column is reachable {last:?} {bar:?}"
    );
}

#[test]
fn a_wide_panel_scrolls_docked_and_floating() {
    init();
    let env = Env::new("scroll-wide");
    let size = egui::vec2(1600.0, 900.0);
    let mut h = setup(&env, "right = \"wide\"", size, 3);
    open(&mut h, "Wide");
    resize(&mut h, size);
    assert!(tab(&h, "Wide").is_some() && has_text(&h, "wide0"));
    assert_eq!(
        menu_item(&mut h, tr("menu.window"), "Float Wide"),
        Some(true)
    );
    wide_reaches_its_last_column(&mut h, None);
    let at = tab(&h, "Wide").unwrap();
    click_at(&mut h, at, PointerButton::Secondary);
    press(&mut h, Role::Button, "Float");
    assert!(window(&h, "Wide"));
    wide_reaches_its_last_column(&mut h, Some("Wide"));
}

#[test]
fn flexible_content_fills_the_room_without_bars() {
    init();
    let env = Env::new("scroll-form");
    let size = egui::vec2(1600.0, 900.0);
    let mut h = setup(&env, "right = \"form\"", size, 3);
    open(&mut h, "Form");
    resize(&mut h, size);
    let docked = find(&h, Role::Button, "Form field", None).expect("docked form");
    assert!(bars(&h, HORIZONTAL, None).is_empty() && bars(&h, VERTICAL, None).is_empty());
    assert!(log_bar(&h).is_none());
    let view = log_view(&h);
    assert!(
        docked.right() > size.x - 30.0 && docked.left() > view.right(),
        "the field fills the area {docked:?}"
    );
    resize(&mut h, egui::vec2(700.0, 500.0));
    assert!(
        bars(&h, HORIZONTAL, None)
            .iter()
            .all(|b| b.left() < log_view(&h).right()),
        "the area keeps its default width and the form fits"
    );
    resize(&mut h, egui::vec2(400.0, 500.0));
    let view = log_view(&h);
    let docked = bars(&h, HORIZONTAL, None)
        .into_iter()
        .filter(|b| b.left() >= view.right())
        .count();
    assert_eq!(docked, 1, "below its minimum the docked form scrolls");
    resize(&mut h, size);
    let view = log_view(&h);
    assert!(
        bars(&h, HORIZONTAL, None)
            .iter()
            .all(|b| b.left() < view.right()),
        "and stops scrolling when it fits again"
    );
    let at = tab(&h, "Form").unwrap();
    click_at(&mut h, at, PointerButton::Secondary);
    press(&mut h, Role::Button, "Float");
    let field = find(&h, Role::Button, "Form field", Some("Form")).expect("floating form");
    let win = find(&h, Role::Window, "Form", None).expect("form window");
    assert!(
        field.width() > win.width() - 40.0,
        "the field fills the window {field:?} {win:?}"
    );
    assert!(bars(&h, HORIZONTAL, Some("Form")).is_empty());
    assert!(bars(&h, VERTICAL, Some("Form")).is_empty());
}

#[test]
fn a_dialog_keeps_its_buttons_and_scrolls_its_content() {
    init();
    let env = Env::new("scroll-dialog");
    let mut h = setup(&env, "", egui::vec2(1600.0, 900.0), 40);
    open(&mut h, "Tall…");
    assert!(window(&h, "Tall"));
    let win = find(&h, Role::Window, "Tall", None).unwrap();
    let done = find(&h, Role::Button, "Done", Some("Tall")).expect("pinned button");
    assert!(
        done.bottom() <= win.bottom() + 0.5 && done.top() >= win.top(),
        "buttons stay visible {done:?} {win:?}"
    );
    let bar = bars(&h, VERTICAL, Some("Tall"));
    assert_eq!(bar.len(), 1, "content scrolls {win:?} {done:?}");
    assert!(
        bar[0].bottom() <= done.top(),
        "buttons stay outside the scroll"
    );
    assert!(bars(&h, HORIZONTAL, Some("Tall")).is_empty());
    let first = find(&h, Role::Button, "tall0", Some("Tall")).unwrap();
    wheel(&mut h, first.center(), egui::vec2(0.0, -10000.0));
    let last = find(&h, Role::Button, "tall39", Some("Tall")).unwrap();
    assert!(
        last.bottom() <= bar[0].bottom() + 0.5,
        "the end is reachable"
    );
}

#[test]
fn a_dialog_that_fits_shows_no_bar() {
    init();
    let env = Env::new("scroll-dialog-fits");
    let mut h = setup(&env, "", egui::vec2(1600.0, 900.0), 2);
    open(&mut h, "Tall…");
    assert!(find(&h, Role::Button, "Done", Some("Tall")).is_some());
    assert!(bars(&h, VERTICAL, Some("Tall")).is_empty());
    assert!(bars(&h, HORIZONTAL, Some("Tall")).is_empty());
}

fn drag(h: &mut H, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for n in 1..=8 {
        let t = n as f32 / 8.0;
        h.event(Event::PointerMoved(from + (to - from) * t));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    for _ in 0..4 {
        settle(h);
    }
}

#[test]
fn a_side_area_keeps_its_default_width_and_the_log_scrolls() {
    init();
    for (dock, size) in [
        ("right = \"alpha\"", egui::vec2(700.0, 500.0)),
        ("left = \"alpha\"", egui::vec2(640.0, 500.0)),
        ("right = \"alpha|beta\"", egui::vec2(960.0, 640.0)),
    ] {
        let env = Env::new("scroll-default-width");
        let mut h = setup(&env, dock, size, 3);
        open(&mut h, "Alpha");
        open(&mut h, "Beta");
        resize(&mut h, size);
        let view = log_view(&h);
        assert!(
            (size.x - view.width() - 280.0).abs() < 2.0,
            "{dock} at {size:?}: the area keeps 280 px {view:?}"
        );
        let need = entry_fields(&h).last().unwrap().right() - view.left();
        if view.width() < need {
            assert!(log_bar(&h).is_some(), "{dock}: the log unit scrolls");
        }
        check_entry_reachable(&mut h, dock);
    }
}
