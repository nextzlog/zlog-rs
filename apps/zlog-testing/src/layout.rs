use egui::accesskit::Role;
use egui::Rect;
use egui_kittest::kittest::NodeT;
use egui_kittest::Harness;

use zlog_gui::testing::layout::PanelHost;

pub type L = Harness<'static, PanelHost>;

pub const NARROW: f32 = 280.0;
pub const WIDE: f32 = 700.0;

pub fn panel(id: &str, contest: &str, dir: &std::path::Path, size: [f32; 2]) -> L {
    let mut host = PanelHost::open(id, crate::setup(dir, contest, None)).expect("panel");
    host.sample();
    let mut h = Harness::builder()
        .with_size(size)
        .with_accessibility_check(false)
        .allow_missing_glyphs()
        .with_step_dt(crate::STEP)
        .build_ui_state(|ui, host: &mut PanelHost| host.show(ui), host);
    let mut fonts = egui::FontDefinitions::default();
    zlog_ui::icon::add_font(&mut fonts);
    h.ctx.set_fonts(fonts);
    h.run_steps(4);
    h
}

pub fn resize(h: &mut L, size: [f32; 2]) {
    h.set_size(egui::vec2(size[0], size[1]));
    h.run_steps(4);
}

const WIDGETS: &[Role] = &[
    Role::Button,
    Role::CheckBox,
    Role::RadioButton,
    Role::ComboBox,
    Role::Tab,
    Role::Link,
    Role::SpinButton,
    Role::Slider,
    Role::TextInput,
    Role::MultilineTextInput,
    Role::Label,
    Role::ProgressIndicator,
    Role::Image,
];

pub fn widgets(h: &L) -> Vec<(Role, String, Rect)> {
    h.root()
        .children_recursive()
        .filter(|n| {
            let a = n.accesskit_node();
            WIDGETS.contains(&a.role()) && !a.is_hidden()
        })
        .map(|n| {
            let a = n.accesskit_node();
            let label = a.label().or_else(|| a.value()).unwrap_or_default();
            (a.role(), label, n.rect())
        })
        .filter(|(_, _, r)| r.width() > 0.0)
        .collect()
}

pub fn clipped(h: &L) -> Vec<String> {
    let body = h.state().body;
    widgets(h)
        .into_iter()
        .filter(|(_, _, r)| r.min.x < body.min.x - 0.5 || r.max.x > body.max.x + 0.5)
        .map(|(role, label, r)| format!("{role:?} {label:?} {:.0}..{:.0}", r.min.x, r.max.x))
        .collect()
}

pub fn overlaps(h: &L) -> Vec<String> {
    let w = widgets(h);
    let mut out = Vec::new();
    for (i, a) in w.iter().enumerate() {
        for b in &w[i + 1..] {
            let (ra, rb) = (a.2.shrink(0.5), b.2.shrink(0.5));
            let nested = ra.contains_rect(rb) || rb.contains_rect(ra);
            if ra.intersects(rb) && !nested {
                out.push(format!("{:?} {:?} / {:?} {:?}", a.0, a.1, b.0, b.1));
            }
        }
    }
    out
}

pub fn span(h: &L) -> f32 {
    let body = h.state().body;
    let (lo, hi) = widgets(h)
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), (_, _, r)| {
            (lo.min(r.min.x), hi.max(r.max.x))
        });
    ((hi - lo) / body.width()).max(0.0)
}

pub fn fills_height(h: &L) -> bool {
    let s = h.state();
    s.content.bottom() >= s.body.bottom() - 2.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Fixed,
    Fills,
    Scrolls,
    External,
}

pub fn problems(id: &str, contest: &str, fit: Fit) -> Vec<String> {
    let env = crate::Env::new(&format!("layout-{id}"));
    let mut h = panel(id, contest, &env.dir, [NARROW, 480.0]);
    let mut out = Vec::new();
    for width in [NARROW, 300.0] {
        resize(&mut h, [width, 480.0]);
        let o = overlaps(&h);
        if !o.is_empty() {
            out.push(format!("{id} overlaps at {width}: {o:?}"));
        }
    }
    resize(&mut h, [NARROW, 480.0]);
    let c = clipped(&h);
    if !c.is_empty() && fit != Fit::Scrolls {
        out.push(format!("{id} clipped at {NARROW}: {c:?}"));
    }
    resize(&mut h, [WIDE, 480.0]);
    let c = clipped(&h);
    if !c.is_empty() {
        let b = h.state().body;
        out.push(format!(
            "{id} clipped at {WIDE} in {:.0}..{:.0}: {c:?}",
            b.min.x, b.max.x
        ));
    }
    let s = span(&h);
    if s < 0.9 && fit != Fit::External {
        out.push(format!("{id} spans {s:.2} of {WIDE}"));
    }
    if fit == Fit::Fills && !fills_height(&h) {
        out.push(format!("{id} does not fill the height"));
    }
    out
}

pub fn assert_layout(windows: &[(&str, Fit)], contest: &str) {
    let all: Vec<String> = windows
        .iter()
        .flat_map(|(id, fit)| problems(id, contest, *fit))
        .collect();
    assert!(all.is_empty(), "{all:#?}");
}

pub fn node_rect(h: &L, role: Role) -> Option<Rect> {
    h.root()
        .children_recursive()
        .find(|n| n.accesskit_node().role() == role)
        .map(|n| n.rect())
}

pub fn drag(h: &mut L, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for i in 1..=4 {
        h.hover_at(from + (to - from) * (i as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(to);
    h.run_steps(2);
}

pub fn saved(h: &L) -> zlog_ui::Section {
    let mut s = zlog_ui::Section::new();
    h.state().panel.save(&mut s);
    s
}

pub fn split_persists(id: &str, contest: &str) {
    let env = crate::Env::new(&format!("split-{id}"));
    let mut h = panel(id, contest, &env.dir, [WIDE, 480.0]);
    let before = saved(&h).get("split").cloned();
    let r = node_rect(&h, Role::Splitter).unwrap_or_else(|| panic!("{id} has a divider"));
    drag(&mut h, r.center(), r.center() + egui::vec2(0.0, 80.0));
    let after = node_rect(&h, Role::Splitter).expect("divider");
    assert!(after.center().y > r.center().y + 40.0, "{id} divider moved");
    let section = saved(&h);
    assert_ne!(section.get("split"), before.as_ref(), "{id} split saved");
    let mut again = panel(id, contest, &env.dir, [WIDE, 480.0]);
    again.state_mut().panel.load(&section);
    again.run_steps(4);
    let back = node_rect(&again, Role::Splitter).expect("divider");
    assert!(
        (back.center().y - after.center().y).abs() < 2.0,
        "{id} split restored"
    );
}

pub const TALL: [f32; 2] = [NARROW, 800.0];

pub fn tall_problems(id: &str, contest: &str, fit: Fit) -> Vec<String> {
    let env = crate::Env::new(&format!("tall-{id}"));
    let h = panel(id, contest, &env.dir, TALL);
    let mut out = Vec::new();
    if let Some(t) = zlog_ui::layout::toolbar_height(&h.ctx) {
        if t > 0.2 * TALL[1] {
            out.push(format!("{id} toolbar takes {t:.0} of {}", TALL[1]));
        }
    }
    let o = overlaps(&h);
    if !o.is_empty() {
        out.push(format!("{id} overlaps when tall: {o:?}"));
    }
    let c = clipped(&h);
    if !c.is_empty() && fit != Fit::Scrolls {
        out.push(format!("{id} clipped when tall: {c:?}"));
    }
    out
}

pub fn assert_tall(windows: &[(&str, Fit)], contest: &str) {
    let all: Vec<String> = windows
        .iter()
        .flat_map(|(id, fit)| tall_problems(id, contest, *fit))
        .collect();
    assert!(all.is_empty(), "{all:#?}");
}
