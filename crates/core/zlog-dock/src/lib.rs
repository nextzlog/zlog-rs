use egui::{
    pos2, Color32, CursorIcon, Id, LayerId, Order, Rect, Sense, Stroke, TextStyle, Ui, UiBuilder,
    Vec2,
};
use zlog_ui::chrome::{HostedWindow, WindowHost};
use zlog_ui::{Command, Section};

zlog_core::texts!();

pub const SECTION: &str = "dock";

pub const TOGGLE: &str = "toggle";

pub const DOCK: &str = "dock";

pub const FLOAT: &str = "float";

const MIN_SIZE: f32 = 80.0;

const MIN_LOG_WIDTH: f32 = 240.0;

const SIDE_SIZE: f32 = 280.0;

const MIN_LOG_HEIGHT: f32 = 120.0;

const MIN_PANE: f32 = 48.0;

const SPLITTER: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Left,
    Right,
    Bottom,
}

impl Area {
    pub const ALL: [Area; 3] = [Area::Left, Area::Right, Area::Bottom];

    pub fn key(self) -> &'static str {
        match self {
            Area::Left => "left",
            Area::Right => "right",
            Area::Bottom => "bottom",
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    fn across(self, v: Vec2) -> f32 {
        match self {
            Area::Bottom => v.y,
            _ => v.x,
        }
    }

    fn along(self, v: Vec2) -> f32 {
        match self {
            Area::Bottom => v.x,
            _ => v.y,
        }
    }

    fn default_size(self, extent: f32) -> f32 {
        match self {
            Area::Bottom => (extent * 0.3).min(200.0),
            _ => SIDE_SIZE,
        }
    }

    fn max_size(self, extent: f32, log_width: f32) -> f32 {
        let log = match self {
            Area::Bottom => MIN_LOG_HEIGHT,
            _ => log_width,
        };
        (extent - log).max(MIN_SIZE)
    }

    fn label(self, language: &str) -> &'static str {
        texts().get(
            language,
            match self {
                Area::Left => "area.left",
                Area::Right => "area.right",
                Area::Bottom => "area.bottom",
            },
        )
    }

    fn edge(self, rect: Rect) -> Rect {
        let side = (rect.width() / 6.0).clamp(32.0, 96.0);
        let low = (rect.height() / 5.0).clamp(32.0, 96.0);
        match self {
            Area::Left => Rect::from_min_size(rect.min, Vec2::new(side, rect.height())),
            Area::Right => Rect::from_min_max(pos2(rect.max.x - side, rect.min.y), rect.max),
            Area::Bottom => Rect::from_min_max(pos2(rect.min.x, rect.max.y - low), rect.max),
        }
    }

    fn half(self, rect: Rect, after: bool) -> Rect {
        let c = rect.center();
        match (self, after) {
            (Area::Bottom, false) => Rect::from_min_max(rect.min, pos2(c.x, rect.max.y)),
            (Area::Bottom, true) => Rect::from_min_max(pos2(c.x, rect.min.y), rect.max),
            (_, false) => Rect::from_min_max(rect.min, pos2(rect.max.x, c.y)),
            (_, true) => Rect::from_min_max(pos2(rect.min.x, c.y), rect.max),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stack {
    pub tabs: Vec<String>,
    pub active: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Region {
    pub stacks: Vec<Stack>,
    pub weights: Vec<f32>,
    pub size: Option<f32>,
}

impl Region {
    fn find(&self, id: &str) -> Option<usize> {
        self.stacks
            .iter()
            .position(|s| s.tabs.iter().any(|t| t == id))
    }

    pub fn ratios(&self) -> Vec<f32> {
        let sum: f32 = self.weights.iter().sum();
        self.weights.iter().map(|w| w / sum).collect()
    }

    fn first(&mut self) -> usize {
        if self.stacks.is_empty() {
            self.stacks.push(Stack::default());
            self.weights.push(1.0);
        }
        0
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Place {
    End,
    Before(String),
    Into(usize),
    Split(usize, bool),
}

#[derive(Debug, Clone, PartialEq)]
enum Move {
    Dock(String, Area, Place),
    Float(String),
}

#[derive(Debug, Clone)]
struct Dragged(String);

struct Pane {
    index: usize,
    open: Vec<usize>,
    current: usize,
}

const SETTLE: f64 = 0.3;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Track {
    pos: Option<egui::Pos2>,
    since: f64,
    born: f64,
    moving: bool,
}

const SETTLE_IN: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drag {
    Still,
    Over(Area),
    Moving,
    Drop(Area),
}

impl Track {
    pub fn update(
        &mut self,
        pos: Option<egui::Pos2>,
        now: f64,
        zone: impl Fn(egui::Pos2) -> Option<Area>,
    ) -> Drag {
        let Some(p) = pos else {
            *self = Track::default();
            return Drag::Still;
        };
        let moved = self.pos.is_some_and(|old| old.distance(p) > 0.5);
        if self.pos.is_none() {
            self.pos = Some(p);
            self.since = now;
            self.born = now;
            return Drag::Still;
        }
        if moved && now - self.born < SETTLE_IN {
            self.pos = Some(p);
            return Drag::Still;
        }
        if moved {
            self.pos = Some(p);
            self.since = now;
            self.moving = true;
        }
        if !self.moving {
            return Drag::Still;
        }
        match zone(p) {
            Some(area) if now - self.since >= SETTLE => {
                self.moving = false;
                Drag::Drop(area)
            }
            Some(area) => Drag::Over(area),
            None if now - self.since >= SETTLE => {
                self.moving = false;
                Drag::Still
            }
            None => Drag::Moving,
        }
    }
}

fn grip(outer: Rect) -> egui::Pos2 {
    pos2(outer.center().x, outer.min.y + 12.0)
}

#[derive(Default)]
pub struct Dock {
    tracks: std::collections::BTreeMap<String, Track>,
    hover: Option<Area>,
    zone: Option<Rect>,
    regions: [Region; 3],
    active: Option<String>,
    changed: bool,
    shown: [u32; 3],
    visible: [Option<bool>; 3],
    displayed: [Option<f32>; 3],
    log_width: Option<f32>,
    last: Vec<(String, Area, usize, bool)>,
}

fn clamped_size(area: Area, preferred: Option<f32>, extent: f32, room: f32, log: f32) -> f32 {
    preferred
        .unwrap_or(area.default_size(extent))
        .max(MIN_SIZE)
        .min(area.max_size(room, log))
        .round()
}

impl Dock {
    fn log_width(&self) -> f32 {
        self.log_width.unwrap_or(MIN_LOG_WIDTH)
    }

    fn locate(&self, id: &str) -> Option<(Area, usize)> {
        Area::ALL
            .into_iter()
            .find_map(|a| self.regions[a.index()].find(id).map(|p| (a, p)))
    }

    pub fn area_of(&self, id: &str) -> Option<Area> {
        self.locate(id).map(|(a, _)| a)
    }

    pub fn region(&self, area: Area) -> &Region {
        &self.regions[area.index()]
    }

    pub fn stack(&self, area: Area) -> &Stack {
        const EMPTY: &Stack = &Stack {
            tabs: Vec::new(),
            active: None,
        };
        self.regions[area.index()].stacks.first().unwrap_or(EMPTY)
    }

    pub fn dock(&mut self, id: &str, area: Area, before: Option<&str>) {
        let place = before.map_or(Place::End, |b| Place::Before(b.to_string()));
        self.place(id, area, place);
    }

    pub fn split(&mut self, id: &str, area: Area, pane: usize, after: bool) {
        self.place(id, area, Place::Split(pane, after));
    }

    fn place(&mut self, id: &str, area: Area, place: Place) {
        let from = self.locate(id);
        let alone = from.is_some_and(|(a, p)| self.regions[a.index()].stacks[p].tabs.len() == 1);
        let mine = |p: usize| from == Some((area, p)) && alone;
        let keep = match &place {
            Place::Before(b) => b == id && from.is_some_and(|(a, _)| a == area),
            Place::Into(p) | Place::Split(p, _) => mine(*p),
            Place::End => false,
        };
        if keep {
            self.focus(id);
            return;
        }
        let gone = self.remove(id);
        self.unhide(area);
        let shift = |p: usize| match gone {
            Some((a, g)) if a == area && g < p => p - 1,
            _ => p,
        };
        let region = &mut self.regions[area.index()];
        let (pane, before) = match place {
            Place::End => (region.first(), None),
            Place::Before(b) => match region.find(&b) {
                Some(p) => (p, Some(b)),
                None => (region.first(), None),
            },
            Place::Into(p) => match shift(p) {
                p if p < region.stacks.len() => (p, None),
                _ => (region.first(), None),
            },
            Place::Split(p, after) => {
                if region.stacks.is_empty() {
                    (region.first(), None)
                } else {
                    let p = shift(p).min(region.stacks.len() - 1);
                    let half = region.weights[p] / 2.0;
                    region.weights[p] = half;
                    let at = p + usize::from(after);
                    region.stacks.insert(at, Stack::default());
                    region.weights.insert(at, half);
                    (at, None)
                }
            }
        };
        let stack = &mut region.stacks[pane];
        let at = before
            .and_then(|b| stack.tabs.iter().position(|t| *t == b))
            .unwrap_or(stack.tabs.len());
        stack.tabs.insert(at, id.to_string());
        stack.active = Some(id.to_string());
        self.active = Some(id.to_string());
        self.changed = true;
    }

    pub fn float(&mut self, id: &str) {
        if let Some((area, p)) = self.locate(id) {
            let alone = self.regions[area.index()].stacks[p].tabs.len() == 1;
            self.last.retain(|l| l.0 != id);
            self.last.push((id.to_string(), area, p, alone));
            self.remove(id);
            self.changed = true;
        }
    }

    pub fn last_area(&self, id: &str) -> Option<Area> {
        self.last.iter().find(|l| l.0 == id).map(|l| l.1)
    }

    pub fn areas_for(&self, id: &str) -> Vec<Area> {
        let first = self.last_area(id).unwrap_or(Area::Right);
        let rest = Area::ALL.into_iter().filter(|a| *a != first);
        std::iter::once(first).chain(rest).collect()
    }

    pub fn dock_into(&mut self, id: &str, area: Area) {
        let len = self.regions[area.index()].stacks.len();
        let place = match self.last.iter().find(|l| l.0 == id) {
            Some(&(_, a, p, alone)) if a == area && len > 0 => match (alone, p < len) {
                (false, _) => Place::Into(p),
                (true, true) => Place::Split(p, false),
                (true, false) => Place::Split(len - 1, true),
            },
            _ => Place::End,
        };
        self.place(id, area, place);
    }

    pub fn toggle(&mut self, id: &str) {
        if self.area_of(id).is_some() {
            self.float(id);
        } else {
            self.dock_into(id, self.areas_for(id)[0]);
        }
    }

    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    fn remove(&mut self, id: &str) -> Option<(Area, usize)> {
        let (area, p) = self.locate(id)?;
        let region = &mut self.regions[area.index()];
        let stack = &mut region.stacks[p];
        stack.tabs.retain(|t| t != id);
        if stack.active.as_deref() == Some(id) {
            stack.active = stack.tabs.first().cloned();
        }
        if !stack.tabs.is_empty() {
            return None;
        }
        region.stacks.remove(p);
        region.weights.remove(p);
        Some((area, p))
    }

    fn follow_window(&mut self, ui: &Ui, id: &str) {
        let outer = ui.input(|i| i.viewport().outer_rect);
        let now = ui.input(|i| i.time);
        let zone = self.zone;
        let pos = outer.zip(zone).map(|(o, _)| grip(o));
        let track = self.tracks.entry(id.to_string()).or_default();
        let drag = track.update(pos, now, |p| {
            let z = zone?;
            Area::ALL.into_iter().find(|a| a.edge(z).contains(p))
        });
        match drag {
            Drag::Over(area) => {
                self.hover = Some(area);
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
            Drag::Drop(area) => {
                self.tracks.remove(id);
                self.dock_into(id, area);
            }
            Drag::Still => {}
            Drag::Moving => {}
        }
        if matches!(drag, Drag::Over(_) | Drag::Moving) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn target<'w>(&self, windows: &'w [(&str, String)]) -> Option<&'w (&'w str, String)> {
        self.active
            .as_deref()
            .and_then(|active| windows.iter().find(|(id, _)| *id == active))
            .or_else(|| windows.last())
    }

    fn apply(&mut self, moves: Vec<Move>) {
        for m in moves {
            match m {
                Move::Dock(id, area, place) => self.place(&id, area, place),
                Move::Float(id) => self.float(&id),
            }
        }
    }

    fn drawn(&self, area: Area, open: &dyn Fn(&str) -> bool) -> bool {
        match self.visible[area.index()] {
            Some(v) => v,
            None => self.regions[area.index()]
                .stacks
                .iter()
                .flat_map(|s| &s.tabs)
                .any(|t| open(t)),
        }
    }

    pub fn shown(&self, area: Area, open: &[&str]) -> bool {
        self.drawn(area, &|t| open.contains(&t))
    }

    pub fn show_area(&mut self, area: Area, on: bool) {
        self.visible[area.index()] = Some(on);
        self.changed = true;
    }

    fn unhide(&mut self, area: Area) {
        if self.visible[area.index()] == Some(false) {
            self.visible[area.index()] = None;
            self.changed = true;
        }
    }

    fn empty_area(&mut self, ui: &mut Ui, area: Area, language: &str, moves: &mut Vec<Move>) {
        let room = ui.available_rect_before_wrap();
        let extent = area.across(room.size());
        let max = area.max_size(extent, self.log_width());
        let region = &self.regions[area.index()];
        let size = region
            .size
            .unwrap_or(area.default_size(extent))
            .max(MIN_SIZE)
            .min(max);
        let id = Id::unique(("dock_empty", area.key()));
        let panel = match area {
            Area::Left => egui::Panel::left(id),
            Area::Right => egui::Panel::right(id),
            Area::Bottom => egui::Panel::bottom(id),
        };
        let shown = panel.resizable(false).exact_size(size).show(ui, |ui| {
            ui.centered_and_justified(|ui| ui.weak(texts().get(language, "hint")));
        });
        let rect = shown.response.rect;
        self.displayed[area.index()] = Some(area.across(rect.size()).round());
        drop_zone(ui, rect, |d| Move::Dock(d, area, Place::End), moves);
    }

    fn area(
        &mut self,
        ui: &mut Ui,
        area: Area,
        reserve: f32,
        windows: &mut [HostedWindow<'_>],
        language: &str,
        moves: &mut Vec<Move>,
    ) {
        let region = &self.regions[area.index()];
        let panes: Vec<Pane> = region
            .stacks
            .iter()
            .enumerate()
            .filter_map(|(index, s)| {
                let open: Vec<usize> = s
                    .tabs
                    .iter()
                    .filter_map(|t| windows.iter().position(|w| &w.id == t))
                    .collect();
                let first = *open.first()?;
                let current = open
                    .iter()
                    .copied()
                    .find(|&i| s.active.as_deref() == Some(windows[i].id.as_str()))
                    .unwrap_or(first);
                Some(Pane {
                    index,
                    open,
                    current,
                })
            })
            .collect();
        match self.visible[area.index()] {
            Some(false) => {
                self.displayed[area.index()] = None;
                return;
            }
            Some(true) if panes.is_empty() => return self.empty_area(ui, area, language, moves),
            _ if panes.is_empty() => {
                self.displayed[area.index()] = None;
                return;
            }
            _ => {}
        }
        let mut weights: Vec<f32> = panes
            .iter()
            .map(|p| {
                region
                    .weights
                    .get(p.index)
                    .copied()
                    .unwrap_or(1.0)
                    .max(0.01)
            })
            .collect();
        let room = ui.available_rect_before_wrap();
        let extent = area.across(room.size());
        let max = area.max_size(extent - reserve, self.log_width());
        let size = clamped_size(
            area,
            region.size,
            extent,
            extent - reserve,
            self.log_width(),
        );
        let id = Id::unique(("dock", area.key(), self.shown[area.index()]));
        let panel = match area {
            Area::Left => egui::Panel::left(id),
            Area::Right => egui::Panel::right(id),
            Area::Bottom => egui::Panel::bottom(id),
        };
        let mut chosen = Vec::new();
        let (mut stopped, mut splitting) = (false, false);
        let shown = panel
            .resizable(true)
            .default_size(size)
            .min_size(MIN_SIZE.min(max))
            .max_size(max)
            .show(ui, |ui| {
                let rect = ui.available_rect_before_wrap();
                ui.advance_cursor_after_rect(rect);
                let rects = pane_rects(area, rect, &weights);
                for (p, r) in panes.iter().zip(&rects) {
                    let mut child = ui.new_child(UiBuilder::new().max_rect(*r).id_salt((
                        "dock_pane",
                        area.key(),
                        p.index,
                    )));
                    child.set_clip_rect(r.intersect(ui.clip_rect()));
                    if let Some(c) = pane(&mut child, area, p, windows, language, moves) {
                        chosen.push((p.index, c));
                    }
                }
                (stopped, splitting) = splitters(ui, area, &rects, &mut weights);
            });
        let now = area.across(shown.response.rect.size()).round();
        self.displayed[area.index()] = Some(now);
        let region = &mut self.regions[area.index()];
        for (p, w) in panes.iter().zip(weights) {
            if let Some(slot) = region.weights.get_mut(p.index) {
                *slot = w;
            }
        }
        self.changed |= stopped;
        if (now - size).abs() > 0.5 {
            let ctx = ui.ctx();
            let tab = egui::DragAndDrop::has_payload_of_type::<Dragged>(ctx);
            let drag = ctx.dragged_id().is_some() || ctx.drag_stopped_id().is_some();
            if drag && !tab && !splitting {
                self.changed |= region.size != Some(now);
                region.size = Some(now);
            } else {
                self.shown[area.index()] += 1;
                ui.ctx().request_repaint();
            }
        }
        for (p, c) in chosen {
            region.stacks[p].active = Some(c.clone());
            self.active = Some(c);
        }
    }
}

fn pane_rects(area: Area, rect: Rect, weights: &[f32]) -> Vec<Rect> {
    let n = weights.len() as f32;
    let total = (area.along(rect.size()) - SPLITTER * (n - 1.0)).max(0.0);
    let sum: f32 = weights.iter().sum();
    let mut at = area.along(rect.min.to_vec2());
    weights
        .iter()
        .map(|w| {
            let len = total * w / sum;
            let r = match area {
                Area::Bottom => Rect::from_x_y_ranges(at..=at + len, rect.y_range()),
                _ => Rect::from_x_y_ranges(rect.x_range(), at..=at + len),
            };
            at += len + SPLITTER;
            r
        })
        .collect()
}

fn splitters(ui: &mut Ui, area: Area, rects: &[Rect], weights: &mut [f32]) -> (bool, bool) {
    let total: f32 = rects.iter().map(|r| area.along(r.size())).sum();
    let sum: f32 = weights.iter().sum();
    if total <= 0.0 {
        return (false, false);
    }
    let (mut stopped, mut dragging) = (false, false);
    for k in 1..rects.len() {
        let (a, b) = (rects[k - 1], rects[k]);
        let gap = match area {
            Area::Bottom => Rect::from_x_y_ranges(a.right()..=b.left(), a.y_range()),
            _ => Rect::from_x_y_ranges(a.x_range(), a.bottom()..=b.top()),
        };
        let r = ui.interact(
            gap,
            Id::unique(("dock_split", area.key(), k)),
            Sense::drag(),
        );
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(match area {
                Area::Bottom => CursorIcon::ResizeHorizontal,
                _ => CursorIcon::ResizeVertical,
            });
        }
        let stroke = if r.hovered() || r.dragged() {
            ui.visuals().widgets.hovered.fg_stroke
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke
        };
        let c = gap.center();
        match area {
            Area::Bottom => ui.painter().vline(c.x, gap.y_range(), stroke),
            _ => ui.painter().hline(gap.x_range(), c.y, stroke),
        };
        if r.dragged() {
            let scale = sum / total;
            let min = MIN_PANE * scale;
            let d = area.along(r.drag_delta()) * scale;
            let low = (min - weights[k - 1]).min(0.0);
            let high = (weights[k] - min).max(0.0);
            let d = d.clamp(low, high);
            weights[k - 1] += d;
            weights[k] -= d;
        }
        stopped |= r.drag_stopped();
        dragging |= r.dragged() || r.drag_stopped();
    }
    (stopped, dragging)
}

fn pane(
    ui: &mut Ui,
    area: Area,
    pane: &Pane,
    windows: &mut [HostedWindow<'_>],
    language: &str,
    moves: &mut Vec<Move>,
) -> Option<String> {
    let mut chosen = None;
    let bar = ui.horizontal_wrapped(|ui| {
        for &i in &pane.open {
            let current = i == pane.current;
            let c = tab(ui, area, &mut windows[i], current, language, moves);
            if c.is_some() {
                chosen = c;
            }
        }
        ui.cursor().min
    });
    let end = bar.inner;
    let bar = bar.response.rect;
    let rest = Rect::from_min_max(
        end,
        pos2(ui.max_rect().right(), bar.bottom().max(end.y + 1.0)),
    );
    let rest = ui.interact(rest, ui.scope_id().with("dock_rest"), Sense::hover());
    if let Some(p) = rest.dnd_release_payload::<Dragged>() {
        moves.push(Move::Dock(p.0.clone(), area, Place::Into(pane.index)));
    }
    let line = ui.visuals().widgets.noninteractive.bg_stroke;
    ui.painter()
        .hline(ui.max_rect().x_range(), bar.bottom() + 1.0, line);
    ui.add_space(3.0);
    let body = ui.available_rect_before_wrap();
    let w = &mut windows[pane.current];
    let out = egui::ScrollArea::both()
        .id_salt(("dock_body", w.id.as_str()))
        .auto_shrink(false)
        .show(ui, |ui| {
            if (w.body)(ui) {
                *w.open = false;
            }
            pointer_pressed(ui)
        });
    zlog_ui::dialog::scroll_bars(ui, &out, egui::Vec2b::TRUE);
    if out.inner {
        chosen = Some(w.id.clone());
    }
    split_target(ui, area, pane.index, body, moves);
    chosen
}

fn split_target(ui: &Ui, area: Area, index: usize, body: Rect, moves: &mut Vec<Move>) {
    let ctx = ui.ctx();
    if !egui::DragAndDrop::has_payload_of_type::<Dragged>(ctx) {
        return;
    }
    let Some(p) = ui
        .input(|i| i.pointer.interact_pos())
        .filter(|p| body.contains(*p))
    else {
        return;
    };
    let after = match area {
        Area::Bottom => p.x > body.center().x,
        _ => p.y > body.center().y,
    };
    let fill = ui.visuals().selection.bg_fill;
    ctx.layer_painter(LayerId::new(
        Order::Foreground,
        Id::unique("dock_split_target"),
    ))
    .rect_filled(
        area.half(body, after),
        4.0,
        Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), 64),
    );
    if ui.input(|i| i.pointer.any_released()) {
        if let Some(d) = egui::DragAndDrop::take_payload::<Dragged>(ctx) {
            moves.push(Move::Dock(d.0.clone(), area, Place::Split(index, after)));
        }
    }
}

fn drop_zone(ui: &Ui, rect: Rect, to: impl FnOnce(String) -> Move, moves: &mut Vec<Move>) {
    let ctx = ui.ctx();
    if !egui::DragAndDrop::has_payload_of_type::<Dragged>(ctx) {
        return;
    }
    if !ui
        .input(|i| i.pointer.interact_pos())
        .is_some_and(|p| rect.contains(p))
    {
        return;
    }
    let fill = ui.visuals().selection.bg_fill;
    ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("dock_drop")))
        .rect_filled(
            rect,
            4.0,
            Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), 64),
        );
    if ui.input(|i| i.pointer.any_released()) {
        if let Some(d) = egui::DragAndDrop::take_payload::<Dragged>(ctx) {
            moves.push(to(d.0.clone()));
        }
    }
}

fn pointer_pressed(ui: &Ui) -> bool {
    ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(ui.min_rect())
}

fn plain(
    ui: &mut Ui,
    text: &str,
    sense: Sense,
    color: impl Fn(&egui::Response) -> Color32,
) -> egui::Response {
    let font = TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font, Color32::PLACEHOLDER);
    let pad = Vec2::new(4.0, 3.0);
    let (rect, r) = ui.allocate_exact_size(galley.size() + pad * 2.0, sense);
    let c = color(&r);
    ui.painter().galley(rect.min + pad, galley, c);
    r
}

fn tab(
    ui: &mut Ui,
    area: Area,
    w: &mut HostedWindow<'_>,
    selected: bool,
    language: &str,
    moves: &mut Vec<Move>,
) -> Option<String> {
    let text = |key| texts().get(language, key);
    let v = ui.visuals().clone();
    let gap = std::mem::replace(&mut ui.spacing_mut().item_spacing.x, 0.0);
    let r = plain(ui, &w.title, Sense::click_and_drag(), |r| {
        if selected {
            v.strong_text_color()
        } else if r.hovered() {
            v.text_color()
        } else {
            v.weak_text_color()
        }
    });
    let title = w.title.clone();
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, &title));
    if r.dragged() {
        r.dnd_set_drag_payload(Dragged(w.id.clone()));
    }
    if r.drag_stopped() {
        let screen = ui.ctx().content_rect();
        let outside = ui
            .input(|i| i.pointer.latest_pos())
            .is_none_or(|p| !screen.contains(p));
        if outside {
            egui::DragAndDrop::clear_payload(ui.ctx());
            moves.push(Move::Float(w.id.clone()));
        }
    }
    if r.dnd_hover_payload::<Dragged>().is_some() {
        let x = r.rect.left() - ui.spacing().item_spacing.x / 2.0;
        ui.painter()
            .vline(x, r.rect.y_range(), Stroke::new(2.0, v.selection.bg_fill));
    }
    if let Some(p) = r.dnd_release_payload::<Dragged>() {
        moves.push(Move::Dock(p.0.clone(), area, Place::Before(w.id.clone())));
    }
    r.context_menu(|ui| {
        if ui.button(text("float.tab")).clicked() {
            moves.push(Move::Float(w.id.clone()));
            ui.close();
        }
        for other in Area::ALL.into_iter().filter(|a| *a != area) {
            if ui.button(other.label(language)).clicked() {
                moves.push(Move::Dock(w.id.clone(), other, Place::End));
                ui.close();
            }
        }
        ui.separator();
        if ui.button(text("close")).clicked() {
            *w.open = false;
            ui.close();
        }
    });
    let hovered = r.hovered();
    let close = plain(ui, "×", Sense::click(), |c| {
        if c.hovered() {
            v.error_fg_color
        } else if selected || hovered {
            v.weak_text_color()
        } else {
            Color32::TRANSPARENT
        }
    })
    .on_hover_text(text("close"));
    ui.spacing_mut().item_spacing.x = gap;
    if selected {
        let y = r.rect.bottom() - 1.0;
        let x = r.rect.left()..=close.rect.right();
        ui.painter()
            .hline(x, y, Stroke::new(2.0, v.selection.bg_fill));
    }
    ui.add_space(4.0);
    close.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text("close")));
    if close.clicked() {
        *w.open = false;
    }
    r.clicked().then(|| w.id.clone())
}

fn edges(ui: &Ui, rect: Rect, moves: &mut Vec<Move>) {
    let ctx = ui.ctx();
    if !egui::DragAndDrop::has_payload_of_type::<Dragged>(ctx) {
        return;
    }
    let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("dock_edges")));
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let released = ui.input(|i| i.pointer.any_released());
    let fill = ui.visuals().selection.bg_fill;
    for area in Area::ALL {
        let edge = area.edge(rect);
        let over = pointer.is_some_and(|p| edge.contains(p));
        let alpha = if over { 96 } else { 32 };
        painter.rect_filled(
            edge,
            4.0,
            Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), alpha),
        );
        if over && released {
            if let Some(p) = egui::DragAndDrop::take_payload::<Dragged>(ctx) {
                moves.push(Move::Dock(p.0.clone(), area, Place::End));
            }
        }
    }
}

fn parse_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if !out.iter().any(|o| o == id) {
            out.push(id.to_string());
        }
    }
    out
}

fn parse_last(text: &str) -> Option<(String, Area, usize, bool)> {
    let mut it = text.trim().split(':');
    let id = it.next().filter(|i| !i.is_empty())?.to_string();
    let area = it.next()?;
    let area = Area::ALL.into_iter().find(|a| a.key() == area)?;
    let pane = it.next()?.parse().ok()?;
    let alone = it.next()? == "1";
    Some((id, area, pane, alone))
}

impl WindowHost for Dock {
    fn hosts(&self, id: &str) -> bool {
        self.area_of(id).is_some()
    }

    fn reserve_log(&mut self, width: f32) {
        self.log_width = Some(width.clamp(MIN_SIZE, MIN_LOG_WIDTH).ceil());
    }

    fn show(&mut self, ui: &mut Ui, windows: &mut [HostedWindow<'_>], language: &str) {
        let mut moves = Vec::new();
        let open = |t: &str| windows.iter().any(|w| w.id == t);
        let reserve = if self.drawn(Area::Right, &open) {
            MIN_SIZE
        } else {
            0.0
        };
        for area in [Area::Left, Area::Right, Area::Bottom] {
            let reserve = if area == Area::Left { reserve } else { 0.0 };
            self.area(ui, area, reserve, windows, language, &mut moves);
        }
        let rect = ui.available_rect_before_wrap();
        edges(ui, rect, &mut moves);
        let origin = ui
            .input(|i| i.viewport().inner_rect)
            .map(|r| r.min.to_vec2());
        self.zone = origin.map(|o| rect.translate(o));
        if let Some(area) = self.hover.take() {
            let fill = ui.visuals().selection.bg_fill;
            ui.ctx()
                .layer_painter(LayerId::new(
                    Order::Foreground,
                    Id::unique("dock_window_over"),
                ))
                .rect_filled(
                    area.edge(rect),
                    4.0,
                    Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), 96),
                );
        }
        self.apply(moves);
    }

    fn floating(&mut self, ui: &mut Ui, id: &str, _language: &str, body: &mut dyn FnMut(&mut Ui)) {
        let rect = ui.max_rect();
        body(ui);
        self.follow_window(ui, id);
        let focused = ui.ctx().viewport_id() != egui::ViewportId::ROOT
            && ui.input(|i| i.viewport().focused == Some(true));
        let pressed = ui.input(|i| i.pointer.any_pressed())
            && ui.rect_contains_pointer(rect.union(ui.min_rect()));
        if focused || pressed {
            self.active = Some(id.to_string());
        }
    }

    fn commands(&self, windows: &[(&str, String)], language: &str) -> Vec<Command> {
        let tip = texts().get(language, "tip");
        let toggle = |label: String| {
            Command::action(TOGGLE, label, tip)
                .on_toolbar(false)
                .toolbar_key(zlog_ui::keys::command_key(SECTION, TOGGLE))
                .icon(zlog_ui::icon::STACK)
        };
        let mut all = match self.target(windows) {
            Some((id, title)) if self.hosts(id) => {
                let label = texts().format(language, "float", &[title]);
                vec![toggle(label.clone()), Command::new(FLOAT, label, tip)]
            }
            Some((id, title)) => {
                let label = texts().format(language, "dock", &[title]);
                let children = self
                    .areas_for(id)
                    .into_iter()
                    .map(|a| {
                        let key = format!("show.{}", a.key());
                        let mut c = Command::new(
                            format!("{DOCK}.{}", a.key()),
                            texts().get(language, &key),
                            texts().get(language, "into.tip"),
                        );
                        c.checkable = true;
                        c.selected = self.last_area(id) == Some(a);
                        c
                    })
                    .collect();
                let menu = Command::new(DOCK, label.clone(), tip).children(children);
                vec![toggle(label), menu]
            }
            None => {
                let label = texts().get(language, "none");
                let reason = texts().get(language, "none.tip");
                vec![
                    toggle(label.into()).disabled_because(reason),
                    Command::new(DOCK, label, tip).disabled_because(reason),
                ]
            }
        };
        let open = |t: &str| windows.iter().any(|(w, _)| *w == t);
        for (weight, (area, icon)) in [
            (Area::Left, zlog_ui::icon::SIDEBAR_SIMPLE),
            (Area::Right, zlog_ui::icon::SIDEBAR),
            (Area::Bottom, zlog_ui::icon::ROWS),
        ]
        .into_iter()
        .enumerate()
        {
            let key = format!("show.{}", area.key());
            let mut c = Command::new(
                key.clone(),
                texts().get(language, &key),
                texts().get(language, "show.tip"),
            )
            .icon(icon)
            .on_toolbar(false);
            c.checkable = true;
            c.selected = self.drawn(area, &open);
            c.weight = weight as i32 + 1;
            all.push(c);
        }
        all
    }

    fn run(&mut self, id: &str, windows: &[(&str, String)]) {
        if let Some(area) = Area::ALL
            .into_iter()
            .find(|a| id.strip_prefix("show.") == Some(a.key()))
        {
            let open = |t: &str| windows.iter().any(|(w, _)| *w == t);
            let on = !self.drawn(area, &open);
            self.show_area(area, on);
            return;
        }
        let Some((target, _)) = self.target(windows) else {
            return;
        };
        let target = target.to_string();
        let into = id.strip_prefix(DOCK).and_then(|k| k.strip_prefix('.'));
        if let Some(area) = Area::ALL.into_iter().find(|a| into == Some(a.key())) {
            self.dock_into(&target, area);
        } else if id == TOGGLE || id == FLOAT && self.hosts(&target) {
            self.toggle(&target);
        }
    }

    fn focus(&mut self, id: &str) -> bool {
        self.active = Some(id.to_string());
        let Some((area, p)) = self.locate(id) else {
            return false;
        };
        self.regions[area.index()].stacks[p].active = Some(id.to_string());
        self.unhide(area);
        true
    }

    fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn section(&self) -> &str {
        SECTION
    }

    fn load(&mut self, section: &Section) {
        let mut seen: Vec<String> = Vec::new();
        for area in Area::ALL {
            let key = area.key();
            let get = |name: &str| section.get(&format!("{key}{name}")).map(String::as_str);
            let lists: Vec<Vec<String>> = get("")
                .map(|t| t.split('|').map(parse_list).collect())
                .unwrap_or_default();
            let actives: Vec<&str> = get(".active")
                .map(|a| a.split('|').map(str::trim).collect())
                .unwrap_or_default();
            let weights: Vec<f32> = get(".split")
                .map(|t| t.split(',').filter_map(|w| w.trim().parse().ok()).collect())
                .unwrap_or_default();
            let weighted = weights.len() == lists.len()
                && weights.iter().all(|w: &f32| w.is_finite() && *w > 0.0);
            let size = get(".size")
                .and_then(|s| s.trim().parse::<f32>().ok())
                .filter(|s| s.is_finite() && *s >= 40.0);
            let mut region = Region {
                size,
                ..Region::default()
            };
            for (n, mut tabs) in lists.into_iter().enumerate() {
                tabs.retain(|t| !seen.contains(t));
                seen.extend(tabs.iter().cloned());
                if tabs.is_empty() {
                    continue;
                }
                let active = actives
                    .get(n)
                    .filter(|a| tabs.iter().any(|t| t == *a))
                    .map(|a| a.to_string())
                    .or_else(|| tabs.first().cloned());
                region.stacks.push(Stack { tabs, active });
                region.weights.push(if weighted { weights[n] } else { 1.0 });
            }
            self.regions[area.index()] = region;
            self.visible[area.index()] = match get(".shown") {
                Some("0") => Some(false),
                Some("1") => Some(true),
                _ => None,
            };
        }
        self.last = section
            .get("last")
            .map(|t| t.split(',').filter_map(parse_last).collect())
            .unwrap_or_default();
        self.changed = false;
    }

    fn save(&self, section: &mut Section) {
        for area in Area::ALL {
            let key = area.key();
            let region = &self.regions[area.index()];
            let mut put = |name: &str, value: Option<String>| match value {
                Some(v) => {
                    section.insert(format!("{key}{name}"), v);
                }
                None => {
                    section.remove(&format!("{key}{name}"));
                }
            };
            let join = |f: &dyn Fn(&Stack) -> String| {
                region.stacks.iter().map(f).collect::<Vec<_>>().join("|")
            };
            put("", Some(join(&|s| s.tabs.join(","))));
            let active = region.stacks.iter().any(|s| s.active.is_some());
            put(
                ".active",
                active.then(|| join(&|s| s.active.clone().unwrap_or_default())),
            );
            let ratios = region.ratios();
            put(
                ".split",
                (ratios.len() > 1).then(|| {
                    let r: Vec<String> = ratios.iter().map(|r| format!("{r:.3}")).collect();
                    r.join(",")
                }),
            );
            put(".size", region.size.map(|s| format!("{s:.0}")));
            let shown = self.visible[area.index()].map(|v| if v { "1" } else { "0" }.to_string());
            put(".shown", shown);
        }
        let last: Vec<String> = self
            .last
            .iter()
            .map(|(id, a, p, alone)| format!("{id}:{}:{p}:{}", a.key(), u8::from(*alone)))
            .collect();
        if last.is_empty() {
            section.remove("last");
        } else {
            section.insert("last".into(), last.join(","));
        }
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.window_host(Dock::default);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows<'a>(list: &'a [(&'a str, &'a str)]) -> Vec<(&'a str, String)> {
        list.iter().map(|(i, t)| (*i, t.to_string())).collect()
    }

    #[test]
    fn dock_command_falls_back_when_the_active_window_is_closed() {
        let mut d = Dock::default();
        d.dock("a", Area::Right, None);
        d.dock("b", Area::Right, None);
        assert!(d.focus("a"));
        let open = windows(&[("b", "Beta")]);
        let c = &d.commands(&open, "en")[0];
        assert!(c.enabled);
        assert_eq!(c.label, "Float Beta");
    }

    #[test]
    fn the_dock_submenu_lists_the_last_area_first_and_docks_into_it() {
        let mut d = Dock::default();
        let open = windows(&[("a", "Alpha")]);
        let menu = |d: &Dock| d.commands(&open, "en")[1].clone();
        let keys = |c: &Command| c.children.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
        assert_eq!(menu(&d).label, "Dock Alpha");
        assert_eq!(keys(&menu(&d)), ["dock.right", "dock.left", "dock.bottom"]);
        assert!(menu(&d).children.iter().all(|c| !c.selected));
        for area in Area::ALL {
            d.run(&format!("dock.{}", area.key()), &open);
            assert_eq!(d.area_of("a"), Some(area));
            assert_eq!(d.commands(&open, "en")[1].id, FLOAT);
            d.run(FLOAT, &open);
            assert!(!d.hosts("a"));
            assert_eq!(keys(&menu(&d))[0], format!("dock.{}", area.key()));
            assert!(menu(&d).children[0].selected);
        }
        d.run(TOGGLE, &open);
        assert_eq!(d.area_of("a"), Some(Area::Bottom), "the plain command");
    }

    #[test]
    fn docking_back_restores_the_stack_or_split_and_shows_the_area() {
        let mut d = Dock::default();
        d.dock("a", Area::Left, None);
        d.split("b", Area::Left, 0, true);
        d.dock("c", Area::Left, Some("b"));
        d.float("b");
        d.dock_into("b", Area::Left);
        assert_eq!(d.region(Area::Left).stacks[1].tabs, ["c", "b"]);
        d.float("a");
        assert_eq!(d.region(Area::Left).stacks.len(), 1);
        d.show_area(Area::Left, false);
        d.dock_into("a", Area::Left);
        assert_eq!(d.region(Area::Left).stacks.len(), 2);
        assert_eq!(d.region(Area::Left).stacks[0].tabs, ["a"]);
        assert!(d.shown(Area::Left, &["a"]));
        d.float("a");
        d.dock_into("a", Area::Bottom);
        assert_eq!(d.stack(Area::Bottom).tabs, ["a"]);
        let mut s = Section::new();
        d.float("a");
        d.save(&mut s);
        let mut e = Dock::default();
        e.load(&s);
        assert_eq!(e.last_area("a"), Some(Area::Bottom));
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn docking_moves_a_window_between_areas_and_floats_it() {
        let mut d = Dock::default();
        d.dock("a", Area::Left, None);
        d.dock("b", Area::Left, None);
        assert_eq!(d.stack(Area::Left).tabs, ["a", "b"]);
        assert_eq!(d.stack(Area::Left).active.as_deref(), Some("b"));
        d.dock("b", Area::Left, Some("a"));
        assert_eq!(d.stack(Area::Left).tabs, ["b", "a"]);
        d.dock("a", Area::Bottom, None);
        assert_eq!(d.area_of("a"), Some(Area::Bottom));
        assert_eq!(d.stack(Area::Left).tabs, ["b"]);
        d.float("b");
        assert!(!d.hosts("b"));
        assert!(d.stack(Area::Left).active.is_none());
        assert!(d.take_changed());
        assert!(!d.take_changed());
        d.float("b");
        assert!(
            !d.take_changed(),
            "floating a floating window changes nothing"
        );
    }

    #[test]
    fn the_command_toggles_the_last_used_open_tool_window() {
        let mut d = Dock::default();
        let open = windows(&[("a", "Alpha"), ("b", "Beta")]);
        let c = &d.commands(&[], "en")[0];
        assert!(!c.enabled);
        assert_eq!(c.label, "Dock");
        assert_eq!(
            d.commands(&open, "en")[0].label,
            "Dock Beta",
            "nothing used yet: the last opened window"
        );
        d.active = Some("a".into());
        assert_eq!(d.commands(&open, "en")[0].label, "Dock Alpha");
        d.run(TOGGLE, &open);
        assert_eq!(d.area_of("a"), Some(Area::Right));
        assert_eq!(d.commands(&open, "en")[0].label, "Float Alpha");
        d.run(TOGGLE, &open);
        assert!(!d.hosts("a"));
        d.active = Some("dialog".into());
        d.run(TOGGLE, &open);
        assert!(!d.hosts("dialog"), "only the listed tool windows dock");
        assert!(
            d.hosts("b"),
            "an id that is not open falls back to the last"
        );
        assert_eq!(d.commands(&open, "en")[0].label, "Float Beta");
    }

    #[test]
    fn focus_selects_a_docked_tab_only() {
        let mut d = Dock::default();
        d.dock("a", Area::Bottom, None);
        d.dock("b", Area::Bottom, None);
        assert!(d.focus("a"));
        assert_eq!(d.stack(Area::Bottom).active.as_deref(), Some("a"));
        assert!(!d.focus("c"));
    }

    #[test]
    fn layout_round_trips_through_its_section() {
        let mut d = Dock::default();
        d.dock("a", Area::Left, None);
        d.dock("b", Area::Bottom, None);
        d.dock("c", Area::Bottom, None);
        d.focus("b");
        d.regions[Area::Bottom.index()].size = Some(240.4);
        let mut s = Section::new();
        s.insert("other".into(), "kept".into());
        d.save(&mut s);
        assert_eq!(s.get("bottom").map(String::as_str), Some("b,c"));
        assert_eq!(s.get("bottom.size").map(String::as_str), Some("240"));
        assert_eq!(s.get("other").map(String::as_str), Some("kept"));
        let mut e = Dock::default();
        e.load(&s);
        assert_eq!(e.stack(Area::Left), d.stack(Area::Left));
        assert_eq!(e.stack(Area::Bottom).tabs, ["b", "c"]);
        assert_eq!(e.stack(Area::Bottom).active.as_deref(), Some("b"));
        assert_eq!(e.region(Area::Bottom).size, Some(240.0));
        assert!(e.stack(Area::Right).tabs.is_empty());
        d.float("a");
        d.save(&mut s);
        assert_eq!(s.get("left").map(String::as_str), Some(""));
        assert!(!s.contains_key("left.active"));
    }

    #[test]
    fn broken_or_old_sections_load_safely() {
        let mut s = Section::new();
        s.insert("left".into(), " a, ,a,b ".into());
        s.insert("left.active".into(), "gone".into());
        s.insert("left.size".into(), "wide".into());
        s.insert("right".into(), "b,c".into());
        s.insert("right.size".into(), "NaN".into());
        s.insert("bottom.size".into(), "5".into());
        s.insert("future".into(), "x".into());
        let mut d = Dock::default();
        d.load(&s);
        assert_eq!(d.stack(Area::Left).tabs, ["a", "b"]);
        assert_eq!(d.stack(Area::Left).active.as_deref(), Some("a"));
        assert_eq!(d.region(Area::Left).size, None);
        assert_eq!(d.stack(Area::Right).tabs, ["c"], "a window docks once");
        assert_eq!(d.region(Area::Right).size, None);
        assert_eq!(d.region(Area::Bottom).size, None);
        assert!(!d.take_changed());
        let mut empty = Dock::default();
        empty.load(&Section::new());
        assert!(Area::ALL.iter().all(|a| empty.stack(*a).tabs.is_empty()));
    }

    #[test]
    fn dropping_a_tab_on_itself_keeps_its_place() {
        let mut d = Dock::default();
        for id in ["a", "b", "c"] {
            d.dock(id, Area::Left, None);
        }
        d.take_changed();
        d.apply(vec![Move::Dock(
            "a".into(),
            Area::Left,
            Place::Before("a".into()),
        )]);
        assert_eq!(d.stack(Area::Left).tabs, ["a", "b", "c"]);
        assert_eq!(d.stack(Area::Left).active.as_deref(), Some("a"));
        assert!(!d.take_changed());
    }

    fn frame(d: &mut Dock, ctx: &egui::Context, size: Vec2) {
        frame_with(d, ctx, size, Vec::new());
    }

    fn frame_with(d: &mut Dock, ctx: &egui::Context, size: Vec2, events: Vec<egui::Event>) {
        let mut open = [true; 3];
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            let mut windows: Vec<HostedWindow<'_>> = ["a", "b", "c"]
                .into_iter()
                .zip(open.iter_mut())
                .map(|(id, open)| HostedWindow {
                    id: id.into(),
                    title: id.into(),
                    open,
                    body: Box::new(|_| false),
                })
                .collect();
            d.show(ui, &mut windows, "en");
        });
        out.textures_delta.clear();
    }

    #[test]
    fn docked_areas_leave_room_for_the_log() {
        let mut d = Dock::default();
        d.dock("a", Area::Left, None);
        d.dock("b", Area::Right, None);
        d.dock("c", Area::Bottom, None);
        let ctx = egui::Context::default();
        let room = |d: &Dock, size: Vec2| {
            let side = |a: Area| d.displayed[a.index()].unwrap_or(0.0);
            (
                size.x - side(Area::Left) - side(Area::Right),
                size.y - side(Area::Bottom),
            )
        };
        let small = Vec2::new(500.0, 400.0);
        for _ in 0..3 {
            frame(&mut d, &ctx, small);
        }
        let (w, h) = room(&d, small);
        assert!(
            w >= MIN_LOG_WIDTH - 1.0 && h >= MIN_LOG_HEIGHT - 1.0,
            "{w} {h}"
        );
        d.take_changed();
        let big = Vec2::new(1600.0, 1000.0);
        frame(&mut d, &ctx, big);
        let tiny = Vec2::new(420.0, 260.0);
        for _ in 0..3 {
            frame(&mut d, &ctx, tiny);
        }
        let (w, h) = room(&d, tiny);
        assert!(
            w >= MIN_LOG_WIDTH - 1.0 && h >= MIN_LOG_HEIGHT - 1.0,
            "{w} {h}"
        );
        assert!(!d.take_changed(), "a clamped size is not saved");
    }

    #[test]
    fn side_areas_leave_the_reserved_log_width() {
        let log = 700.0;
        assert_eq!(
            clamped_size(Area::Right, None, 1600.0, 1600.0, log),
            SIDE_SIZE
        );
        assert_eq!(clamped_size(Area::Right, None, 960.0, 960.0, log), 260.0);
        assert_eq!(
            clamped_size(Area::Left, Some(500.0), 960.0, 880.0, log),
            180.0
        );
        assert_eq!(
            clamped_size(Area::Right, Some(30.0), 960.0, 960.0, log),
            MIN_SIZE
        );
        assert_eq!(clamped_size(Area::Right, None, 700.0, 700.0, log), MIN_SIZE);
        assert_eq!(clamped_size(Area::Bottom, None, 640.0, 640.0, log), 192.0);
    }

    #[test]
    fn side_areas_keep_their_width_and_the_log_keeps_its_minimum() {
        let mut d = Dock::default();
        d.dock("a", Area::Left, None);
        d.dock("b", Area::Right, None);
        d.take_changed();
        let ctx = egui::Context::default();
        let side = |d: &Dock, a: Area| d.displayed[a.index()].unwrap();
        for (reserve, width, left, right) in [
            (900.0, 960.0, SIDE_SIZE, SIDE_SIZE),
            (600.0, 960.0, SIDE_SIZE, SIDE_SIZE),
            (200.0, 960.0, SIDE_SIZE, SIDE_SIZE),
            (900.0, 700.0, SIDE_SIZE, 700.0 - SIDE_SIZE - MIN_LOG_WIDTH),
            (900.0, 600.0, SIDE_SIZE, MIN_SIZE),
            (900.0, 400.0, MIN_SIZE, MIN_SIZE),
        ] {
            d.reserve_log(reserve);
            let size = Vec2::new(width, 640.0);
            for _ in 0..3 {
                frame(&mut d, &ctx, size);
            }
            let got = (side(&d, Area::Left), side(&d, Area::Right));
            assert_eq!(got, (left, right), "reserve {reserve} width {width}");
            if width >= 600.0 {
                assert!(width - got.0 - got.1 >= MIN_LOG_WIDTH.min(reserve) - 1.0);
            }
        }
        assert!(!d.take_changed(), "clamping never saves a size");
        assert_eq!(d.region(Area::Right).size, None);
    }

    #[test]
    fn a_dragged_side_width_is_kept_beside_a_wide_entry() {
        let mut d = Dock::default();
        d.dock("b", Area::Right, None);
        d.regions[Area::Right.index()].size = Some(400.0);
        d.reserve_log(1200.0);
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&mut d, &ctx, Vec2::new(960.0, 640.0));
        }
        assert_eq!(d.displayed[Area::Right.index()], Some(400.0));
    }

    #[test]
    fn a_hidden_right_area_reserves_no_room_beside_the_left() {
        let mut d = Dock::default();
        d.dock("a", Area::Left, None);
        d.dock("b", Area::Right, None);
        let ctx = egui::Context::default();
        let size = Vec2::new(560.0, 640.0);
        d.show_area(Area::Right, false);
        for _ in 0..3 {
            frame(&mut d, &ctx, size);
        }
        assert_eq!(d.displayed[Area::Left.index()], Some(SIDE_SIZE));
        assert_eq!(d.displayed[Area::Right.index()], None);
        d.show_area(Area::Right, true);
        for _ in 0..3 {
            frame(&mut d, &ctx, size);
        }
        assert_eq!(
            d.displayed[Area::Left.index()],
            Some(560.0 - MIN_SIZE - MIN_LOG_WIDTH)
        );
    }

    #[test]
    fn idle_frames_after_a_resize_do_not_churn() {
        let mut d = Dock::default();
        d.dock("a", Area::Left, None);
        d.dock("b", Area::Right, None);
        d.dock("c", Area::Bottom, None);
        let ctx = egui::Context::default();
        for size in [
            Vec2::new(1600.0, 1000.0),
            Vec2::new(500.0, 300.0),
            Vec2::new(1600.0, 1000.0),
        ] {
            for _ in 0..4 {
                frame(&mut d, &ctx, size);
            }
            let before = d.shown;
            for _ in 0..5 {
                frame(&mut d, &ctx, size);
            }
            assert_eq!(before, d.shown, "{size:?}");
        }
    }

    #[test]
    fn the_splitter_never_shrinks_a_pane_below_its_minimum() {
        let ctx = egui::Context::default();
        let rects = [
            Rect::from_min_size(pos2(0.0, 0.0), Vec2::new(200.0, 20.0)),
            Rect::from_min_size(pos2(0.0, 26.0), Vec2::new(200.0, 30.0)),
        ];
        for delta in [-30.0, 30.0] {
            let mut weights = [20.0, 30.0];
            let press = |pressed| egui::Event::PointerButton {
                pos: pos2(100.0, 23.0),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let events = [
                vec![egui::Event::PointerMoved(pos2(100.0, 23.0))],
                vec![press(true)],
                vec![egui::Event::PointerMoved(pos2(100.0, 23.0 + delta))],
                vec![egui::Event::PointerMoved(pos2(100.0, 23.0 + 2.0 * delta))],
            ];
            for e in events {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(400.0, 500.0),
                    )),
                    events: e,
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| {
                    splitters(ui, Area::Right, &rects, &mut weights);
                });
                out.textures_delta.clear();
            }
            assert!(
                weights[0] >= 20.0 - 0.01 && weights[1] >= 30.0 - 0.01,
                "{delta}: {weights:?}"
            );
        }
    }

    #[test]
    fn preferred_sizes_come_back_when_the_window_grows() {
        let mut d = Dock::default();
        d.dock("a", Area::Left, None);
        d.dock("b", Area::Right, None);
        d.regions[Area::Right.index()].size = Some(300.0);
        let ctx = egui::Context::default();
        let sizes = |d: &mut Dock, size: Vec2| {
            for _ in 0..3 {
                frame(d, &ctx, size);
            }
            (
                d.displayed[Area::Left.index()],
                d.displayed[Area::Right.index()],
            )
        };
        let big = Vec2::new(1600.0, 1000.0);
        let before = sizes(&mut d, big);
        d.take_changed();
        let small = sizes(&mut d, Vec2::new(500.0, 400.0));
        assert_ne!(small, before);
        assert!(!d.take_changed(), "the clamped size is only shown");
        assert_eq!(d.region(Area::Right).size, Some(300.0));
        assert_eq!(d.region(Area::Left).size, None);
        assert_eq!(sizes(&mut d, big), before);
    }

    #[test]
    fn dragging_the_edge_sets_the_preferred_size() {
        let mut d = Dock::default();
        d.dock("b", Area::Right, None);
        let ctx = egui::Context::default();
        let big = Vec2::new(1600.0, 1000.0);
        for _ in 0..3 {
            frame(&mut d, &ctx, big);
        }
        let edge = big.x - d.displayed[Area::Right.index()].unwrap();
        let button = |pos: egui::Pos2, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let at = |x: f32| egui::pos2(x, 500.0);
        frame_with(&mut d, &ctx, big, vec![egui::Event::PointerMoved(at(edge))]);
        frame_with(&mut d, &ctx, big, vec![button(at(edge), true)]);
        for x in [edge - 40.0, edge - 80.0, edge - 100.0] {
            frame_with(&mut d, &ctx, big, vec![egui::Event::PointerMoved(at(x))]);
        }
        frame_with(&mut d, &ctx, big, vec![button(at(edge - 100.0), false)]);
        frame(&mut d, &ctx, big);
        let want = big.x - edge + 100.0;
        let got = d.region(Area::Right).size.expect("dragged size saved");
        assert!((got - want).abs() <= 2.0, "{got} {want}");
        assert!(d.take_changed());
        for _ in 0..3 {
            frame(&mut d, &ctx, big);
        }
        assert_eq!(d.region(Area::Right).size, Some(got));
    }

    #[test]
    fn a_split_holds_two_stacks_and_goes_away_with_its_last_tab() {
        let mut d = Dock::default();
        d.dock("a", Area::Right, None);
        d.dock("b", Area::Right, None);
        d.split("b", Area::Right, 0, true);
        let r = d.region(Area::Right);
        assert_eq!(r.stacks.len(), 2);
        assert_eq!(r.stacks[0].tabs, ["a"]);
        assert_eq!(r.stacks[1].tabs, ["b"]);
        assert_eq!(r.ratios(), [0.5, 0.5]);
        d.split("b", Area::Right, 1, false);
        assert_eq!(d.region(Area::Right).stacks.len(), 2, "own pane: no-op");
        d.split("c", Area::Right, 0, false);
        assert_eq!(d.region(Area::Right).stacks[0].tabs, ["c"]);
        assert_eq!(d.region(Area::Right).stacks.len(), 3);
        d.dock("b", Area::Left, None);
        let r = d.region(Area::Right);
        assert_eq!(r.stacks.len(), 2);
        assert_eq!(r.weights.len(), 2);
        d.apply(vec![Move::Dock("c".into(), Area::Right, Place::Into(1))]);
        let r = d.region(Area::Right);
        assert_eq!(r.stacks.len(), 1);
        assert_eq!(r.stacks[0].tabs, ["a", "c"]);
    }

    #[test]
    fn splits_round_trip_and_old_sections_load_as_one_stack() {
        let mut d = Dock::default();
        d.dock("a", Area::Right, None);
        d.split("b", Area::Right, 0, true);
        d.regions[Area::Right.index()].weights = vec![3.0, 1.0];
        let mut s = Section::new();
        d.save(&mut s);
        assert_eq!(s.get("right").map(String::as_str), Some("a|b"));
        assert_eq!(s.get("right.active").map(String::as_str), Some("a|b"));
        assert_eq!(
            s.get("right.split").map(String::as_str),
            Some("0.750,0.250")
        );
        let mut e = Dock::default();
        e.load(&s);
        assert_eq!(e.region(Area::Right).stacks, d.region(Area::Right).stacks);
        assert_eq!(e.region(Area::Right).ratios(), [0.75, 0.25]);
        e.float("b");
        e.save(&mut s);
        assert_eq!(s.get("right").map(String::as_str), Some("a"));
        assert!(!s.contains_key("right.split"));
        let mut old = Section::new();
        old.insert("right".into(), "a,b".into());
        old.insert("right.active".into(), "b".into());
        old.insert("right.split".into(), "x".into());
        let mut f = Dock::default();
        f.load(&old);
        assert_eq!(f.region(Area::Right).stacks.len(), 1);
        assert_eq!(f.stack(Area::Right).active.as_deref(), Some("b"));
        assert_eq!(f.region(Area::Right).weights, [1.0]);
    }

    fn big_frame(d: &mut Dock, ctx: &egui::Context, size: Vec2) -> Vec<egui::epaint::ClippedShape> {
        let mut open = [true; 3];
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            let mut windows: Vec<HostedWindow<'_>> = ["a", "b", "c"]
                .into_iter()
                .zip(open.iter_mut())
                .map(|(id, open)| HostedWindow {
                    id: id.into(),
                    title: format!("Tab {id}"),
                    open,
                    body: Box::new(|ui| {
                        ui.allocate_space(Vec2::splat(3000.0));
                        false
                    }),
                })
                .collect();
            d.show(ui, &mut windows, "en");
        });
        out.textures_delta.clear();
        out.shapes
    }

    #[test]
    fn large_content_scrolls_inside_the_area_without_widening_it() {
        let mut d = Dock::default();
        d.dock("a", Area::Right, None);
        d.dock("b", Area::Bottom, None);
        let ctx = egui::Context::default();
        let size = Vec2::new(1000.0, 700.0);
        for _ in 0..4 {
            big_frame(&mut d, &ctx, size);
        }
        let right = d.displayed[Area::Right.index()].unwrap();
        let bottom = d.displayed[Area::Bottom.index()].unwrap();
        assert!(right <= size.x - MIN_LOG_WIDTH + 1.0, "{right}");
        assert!(bottom <= size.y - MIN_LOG_HEIGHT + 1.0, "{bottom}");
        assert!(!d.take_changed() || d.region(Area::Right).size.is_none());
        assert_eq!(right, SIDE_SIZE, "a wide panel does not widen the area");
    }

    #[test]
    fn tabs_and_close_marks_draw_no_box() {
        let mut d = Dock::default();
        d.dock("a", Area::Right, None);
        d.dock("b", Area::Right, None);
        let ctx = egui::Context::default();
        let size = Vec2::new(1000.0, 700.0);
        let mut shapes = Vec::new();
        for _ in 0..4 {
            shapes = big_frame(&mut d, &ctx, size);
        }
        let left = size.x - d.displayed[Area::Right.index()].unwrap();
        let bar = Rect::from_min_max(pos2(left + 2.0, 0.0), pos2(size.x, 24.0));
        let mut boxes = Vec::new();
        let mut texts = 0;
        for c in &shapes {
            flat(&c.shape, &mut |s| match s {
                egui::Shape::Rect(r) if bar.intersects(r.rect) && r.rect.height() < 40.0 => {
                    boxes.push(r.rect)
                }
                egui::Shape::Text(t) if bar.contains(t.pos) => texts += 1,
                _ => {}
            });
        }
        assert!(boxes.is_empty(), "{boxes:?}");
        assert!(texts >= 3, "two titles and the active close mark: {texts}");
    }

    fn flat(shape: &egui::Shape, f: &mut dyn FnMut(&egui::Shape)) {
        match shape {
            egui::Shape::Vec(v) => v.iter().for_each(|s| flat(s, f)),
            s => f(s),
        }
    }

    struct Rng(u64);

    impl Rng {
        fn next(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) % n as u64) as usize
        }
    }

    fn valid(d: &Dock) {
        let mut seen = Vec::new();
        for area in Area::ALL {
            let r = d.region(area);
            assert_eq!(r.stacks.len(), r.weights.len());
            for s in &r.stacks {
                assert!(!s.tabs.is_empty(), "no empty stack");
                let active = s.active.as_ref().expect("active tab");
                assert!(s.tabs.contains(active));
                for t in &s.tabs {
                    assert!(!seen.contains(t), "{t} placed twice");
                    seen.push(t.clone());
                }
            }
            assert!(r.weights.iter().all(|w| w.is_finite() && *w > 0.0));
        }
    }

    #[test]
    fn random_operations_keep_the_layout_valid_and_round_trip() {
        let ids = ["a", "b", "c", "d", "e"];
        for seed in 0..200u64 {
            let mut rng = Rng(seed);
            let mut d = Dock::default();
            for _ in 0..60 {
                let id = ids[rng.next(ids.len())];
                let area = Area::ALL[rng.next(3)];
                let panes = d.region(area).stacks.len().max(1);
                match rng.next(6) {
                    0 => d.dock(id, area, None),
                    1 => d.dock(id, area, Some(ids[rng.next(ids.len())])),
                    2 => d.split(id, area, rng.next(panes + 1), rng.next(2) == 1),
                    3 => d.apply(vec![Move::Dock(
                        id.into(),
                        area,
                        Place::Into(rng.next(panes + 1)),
                    )]),
                    4 => d.float(id),
                    _ => d.toggle(id),
                }
                valid(&d);
                let mut s = Section::new();
                d.save(&mut s);
                let mut e = Dock::default();
                e.load(&s);
                for area in Area::ALL {
                    let (x, y) = (d.region(area), e.region(area));
                    assert_eq!(x.stacks, y.stacks, "seed {seed}");
                    for (p, q) in x.ratios().iter().zip(y.ratios()) {
                        assert!((p - q).abs() < 0.002, "seed {seed}");
                    }
                }
            }
        }
    }

    #[test]
    fn corrupt_split_entries_load_safely() {
        let mut s = Section::new();
        s.insert("right".into(), "|a,,|b|a|".into());
        s.insert("right.active".into(), "x|||b".into());
        s.insert("right.split".into(), "1,-2,inf".into());
        s.insert("bottom".into(), "c|d".into());
        s.insert("bottom.split".into(), "2,1".into());
        s.insert("left.split".into(), "0.5".into());
        let mut d = Dock::default();
        d.load(&s);
        valid(&d);
        let r = d.region(Area::Right);
        assert_eq!(r.stacks.len(), 2);
        assert_eq!(r.stacks[0].tabs, ["a"]);
        assert_eq!(r.stacks[1].active.as_deref(), Some("b"));
        assert_eq!(r.weights, [1.0, 1.0]);
        let b = d.region(Area::Bottom);
        assert!((b.ratios()[0] - 2.0 / 3.0).abs() < 1e-6);
        assert!(d.region(Area::Left).stacks.is_empty());
    }

    #[test]
    fn floating_and_redocking_in_any_order_keeps_closed_tabs_in_place() {
        let mut d = Dock::default();
        d.dock("a", Area::Right, None);
        d.dock("b", Area::Right, None);
        d.split("c", Area::Right, 0, true);
        d.focus("a");
        assert_eq!(d.region(Area::Right).stacks[0].active.as_deref(), Some("a"));
        assert_eq!(d.region(Area::Right).stacks[1].tabs, ["c"]);
        d.float("c");
        assert_eq!(
            d.region(Area::Right).stacks.len(),
            1,
            "the split goes with its last tab"
        );
        valid(&d);
    }

    #[test]
    fn areas_hide_and_show_and_remember_it() {
        let mut d = Dock::default();
        d.dock("a", Area::Right, None);
        d.regions[Area::Right.index()].size = Some(300.0);
        let open = windows(&[("a", "Alpha")]);
        let ids = ["a"];
        let label = |d: &Dock, key: &str| {
            d.commands(&open, "en")
                .into_iter()
                .find(|c| c.id == key)
                .unwrap()
        };
        assert!(label(&d, "show.right").selected);
        assert!(
            !label(&d, "show.left").selected,
            "an empty area is not shown"
        );
        assert!(label(&d, "show.left").enabled);
        assert_eq!(label(&d, "show.bottom").label, "Bottom panel");
        d.run("show.right", &open);
        assert!(!d.shown(Area::Right, &ids));
        let ctx = egui::Context::default();
        let size = Vec2::new(1000.0, 600.0);
        for _ in 0..3 {
            frame(&mut d, &ctx, size);
        }
        assert_eq!(d.displayed[Area::Right.index()], None, "hidden");
        assert!(d.hosts("a"), "still docked");
        let mut s = Section::new();
        d.save(&mut s);
        assert_eq!(s.get("right.shown").map(String::as_str), Some("0"));
        let mut e = Dock::default();
        e.load(&s);
        assert!(!e.shown(Area::Right, &ids));
        e.run("show.right", &open);
        for _ in 0..3 {
            frame(&mut e, &ctx, size);
        }
        assert_eq!(e.displayed[Area::Right.index()], Some(300.0), "size kept");
        e.show_area(Area::Right, false);
        assert!(e.focus("a"));
        assert!(e.shown(Area::Right, &ids), "a focused panel shows its area");
        e.show_area(Area::Bottom, false);
        e.dock("a", Area::Bottom, None);
        assert!(e.shown(Area::Bottom, &ids), "docking shows the area");
    }

    #[test]
    fn an_empty_area_shows_a_hint_and_takes_a_drop() {
        let mut d = Dock::default();
        let open = windows(&[("a", "Alpha")]);
        d.run("show.left", &open);
        assert!(d.shown(Area::Left, &[]));
        let ctx = egui::Context::default();
        let size = Vec2::new(1000.0, 600.0);
        for _ in 0..2 {
            frame(&mut d, &ctx, size);
        }
        let w = d.displayed[Area::Left.index()].expect("shown empty");
        assert!(w >= MIN_SIZE);
        d.run("show.left", &open);
        frame(&mut d, &ctx, size);
        assert_eq!(d.displayed[Area::Left.index()], None);
        let mut s = Section::new();
        d.save(&mut s);
        assert_eq!(s.get("left").map(String::as_str), Some(""));
    }

    #[test]
    fn a_moved_window_highlights_the_zone_and_docks_when_it_stops() {
        let zone = |p: egui::Pos2| (p.x < 50.0).then_some(Area::Left);
        let mut t = Track::default();
        assert_eq!(t.update(Some(pos2(300.0, 100.0)), 0.0, zone), Drag::Still);
        assert_eq!(
            t.update(Some(pos2(305.0, 100.0)), 0.5, zone),
            Drag::Still,
            "settling in"
        );
        assert_eq!(t.update(Some(pos2(305.0, 100.0)), 2.0, zone), Drag::Still);
        assert_eq!(t.update(Some(pos2(200.0, 100.0)), 2.1, zone), Drag::Moving);
        assert_eq!(
            t.update(Some(pos2(20.0, 100.0)), 2.2, zone),
            Drag::Over(Area::Left)
        );
        assert_eq!(
            t.update(Some(pos2(20.0, 100.0)), 2.4, zone),
            Drag::Over(Area::Left)
        );
        assert_eq!(
            t.update(Some(pos2(20.0, 100.0)), 2.6, zone),
            Drag::Drop(Area::Left)
        );
        assert_eq!(t.update(Some(pos2(20.0, 100.0)), 3.0, zone), Drag::Still);
    }

    #[test]
    fn a_window_stopping_outside_the_zones_stays_floating() {
        let zone = |p: egui::Pos2| (p.x < 50.0).then_some(Area::Left);
        let mut t = Track::default();
        t.update(Some(pos2(300.0, 100.0)), 0.0, zone);
        assert_eq!(t.update(Some(pos2(400.0, 100.0)), 2.0, zone), Drag::Moving);
        assert_eq!(t.update(Some(pos2(400.0, 100.0)), 2.5, zone), Drag::Still);
        assert_eq!(t.update(Some(pos2(400.0, 100.0)), 3.0, zone), Drag::Still);
    }

    #[test]
    fn dragging_a_tab_out_of_the_main_window_floats_it() {
        use egui::{Event, PointerButton};
        let mut d = Dock::default();
        d.dock("a", Area::Left, None);
        d.dock("b", Area::Left, None);
        let ctx = egui::Context::default();
        let size = Vec2::new(800.0, 600.0);
        frame(&mut d, &ctx, size);
        let at = pos2(10.0, 12.0);
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame_with(
            &mut d,
            &ctx,
            size,
            vec![Event::PointerMoved(at), button(at, true)],
        );
        for x in [20.0, 40.0, 0.0] {
            frame_with(&mut d, &ctx, size, vec![Event::PointerMoved(pos2(x, 12.0))]);
        }
        let out = pos2(-60.0, 300.0);
        frame_with(
            &mut d,
            &ctx,
            size,
            vec![Event::PointerMoved(out), button(out, false)],
        );
        frame(&mut d, &ctx, size);
        assert_eq!(d.area_of("a"), None, "floated");
        assert_eq!(d.area_of("b"), Some(Area::Left));
    }

    #[test]
    fn without_a_window_position_nothing_happens() {
        let mut t = Track::default();
        for n in 0..10 {
            assert_eq!(t.update(None, n as f64, |_| Some(Area::Left)), Drag::Still);
        }
    }

    #[test]
    fn register_installs_the_host() {
        let mut b = zlog_ui::Builder::new();
        assert!(b.build_window_host().is_none());
        register(&mut b);
        let host = b.build_window_host().expect("host");
        assert_eq!(host.section(), SECTION);
    }
}
