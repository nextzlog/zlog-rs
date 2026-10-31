// Boundaries simplified from the National Land Numerical Information administrative
// boundaries; credited as 出典: 国土交通省国土数値情報.

mod data;

use std::collections::{BTreeMap, HashMap, HashSet};

use egui::{Color32, Pos2, Response, Shape as Paint, Stroke, Ui};
use geo::{Contains, Point, Polygon, TriangulateEarcut};
use walkers::sources::OpenStreetMap;
use walkers::{lon_lat, HeaderValue, HttpOptions, HttpTiles, Map, MapMemory, Plugin, Projector};
use zlog_ui::{Panel, View};

pub use data::{map_data, source, worked_codes};

zlog_core::texts!();

const FILL: Color32 = Color32::from_rgba_premultiplied(120, 0, 0, 60);
const EDGE: Color32 = Color32::from_rgb(200, 0, 0);
const BORDER: Color32 = Color32::from_gray(150);

pub struct Maplot {
    memory: MapMemory,
    tiles: Option<HttpTiles>,
    show_tiles: bool,
    drawn: HashMap<String, Drawn>,
    hidden: (Vec<String>, HashSet<u32>),
}

struct Drawn {
    polygons: Vec<Polygon<f64>>,
    triangles: Vec<[f64; 2]>,
    rings: Vec<Vec<(f64, f64)>>,
    inner: Vec<u32>,
}

impl Default for Maplot {
    fn default() -> Self {
        let mut memory = MapMemory::default();
        memory.center_at(lon_lat(137.0, 37.5));
        let _ = memory.set_zoom(5.0);
        Maplot {
            memory,
            tiles: None,
            show_tiles: false,
            drawn: HashMap::new(),
            hidden: (Vec::new(), HashSet::new()),
        }
    }
}

impl Maplot {
    fn prepare(&mut self, worked: &[Worked]) {
        for (code, area, _) in worked {
            self.drawn.entry(code.clone()).or_insert_with(|| {
                let polygons = data::dissolve(&area.shapes);
                let triangles = geo::unary_union(&polygons)
                    .iter()
                    .flat_map(|p| p.earcut_triangles())
                    .flat_map(|t| t.to_array().map(|c| [c.x, c.y]))
                    .collect();
                let rings = data::outline(&area.shapes)
                    .iter()
                    .map(|r| data::ring_points(r))
                    .collect();
                Drawn {
                    polygons,
                    triangles,
                    rings,
                    inner: data::inner_arcs(&area.shapes),
                }
            });
        }
        let codes: Vec<String> = worked.iter().map(|w| w.0.clone()).collect();
        if self.hidden.0 != codes {
            let inner = codes
                .iter()
                .flat_map(|c| self.drawn[c].inner.iter().copied())
                .collect();
            self.hidden = (codes, inner);
        }
    }
}

type Worked = (String, &'static data::Area, usize);

struct Areas<'a> {
    worked: &'a [Worked],
    drawn: &'a HashMap<String, Drawn>,
    hidden: &'a HashSet<u32>,
}

impl Plugin for Areas<'_> {
    fn run(
        self: Box<Self>,
        ui: &mut Ui,
        response: &Response,
        projector: &Projector,
        _memory: &MapMemory,
    ) {
        let painter = ui.painter();
        let to_screen = |x: f64, y: f64| projector.project(lon_lat(x, y)).to_pos2();
        let data = map_data();

        for (code, _, _) in self.worked {
            let mut mesh = egui::Mesh::default();
            for p in &self.drawn[code].triangles {
                mesh.colored_vertex(to_screen(p[0], p[1]), FILL);
            }
            mesh.indices = (0..mesh.vertices.len() as u32).collect();
            painter.add(Paint::mesh(mesh));
        }
        let rect = response.rect;
        let a = projector.unproject(rect.left_bottom().to_vec2());
        let b = projector.unproject(rect.right_top().to_vec2());
        let view = [a.x() * 1e4, a.y() * 1e4, b.x() * 1e4, b.y() * 1e4].map(|v| v as i32);
        for (i, arc) in data.arcs.iter().enumerate() {
            let [x0, y0, x1, y1] = data.bounds[i];
            if x1 < view[0] || x0 > view[2] || y1 < view[1] || y0 > view[3] {
                continue;
            }
            if self.hidden.contains(&(i as u32)) {
                continue;
            }
            let pts = outline(
                arc.iter()
                    .map(|p| to_screen(p[0] as f64 / 1e4, p[1] as f64 / 1e4)),
            );
            if pts.len() >= 2 {
                painter.add(Paint::line(pts, Stroke::new(0.4, BORDER)));
            }
        }
        for (code, _, _) in self.worked {
            for ring in &self.drawn[code].rings {
                let pts = outline(ring.iter().map(|&(x, y)| to_screen(x, y)));
                if pts.len() >= 3 {
                    painter.add(Paint::closed_line(pts, Stroke::new(1.0, EDGE)));
                }
            }
        }

        if let Some(pos) = response.hover_pos() {
            let at = projector.unproject(pos.to_vec2());
            let point = Point::new(at.x(), at.y());
            let hit = self
                .worked
                .iter()
                .find(|(code, _, _)| self.drawn[code].polygons.iter().any(|p| p.contains(&point)));
            if let Some((code, area, count)) = hit {
                response
                    .clone()
                    .on_hover_text_at_pointer(format!("{code}  {}  ({count})", area.name));
            }
        }
    }
}

fn outline(points: impl Iterator<Item = Pos2>) -> Vec<Pos2> {
    let mut out: Vec<Pos2> = Vec::new();
    let mut end = None;
    for q in points {
        if out.last().is_none_or(|last| last.distance(q) >= 1.0) {
            out.push(q);
        }
        end = Some(q);
    }
    if let Some(q) = end.filter(|q| out.last() != Some(q)) {
        out.push(q);
    }
    out
}

impl Panel for Maplot {
    fn migrations(&self) -> Vec<zlog_ui::migration::Migration> {
        vec![zlog_ui::migration::Migration::RenamedPanel("maplot", "map")]
    }

    fn id(&self) -> &str {
        "map"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Report
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Progress, 2)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::MAP_TRIFOLD)]
    }

    fn default_size(&self) -> [f32; 2] {
        [520.0, 560.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let data = map_data();
        let codes: BTreeMap<String, usize> = worked_codes(view.log);
        let worked: Vec<Worked> = codes
            .iter()
            .filter_map(|(code, n)| data.areas.get(code).map(|a| (code.clone(), a, *n)))
            .collect();
        let unknown = codes.len() - worked.len();
        let lang = view.language;
        let areas = texts().format(lang, "areas", &[&worked.len()]);
        let unknown = if unknown > 0 {
            texts().format(lang, "unknown", &[&unknown])
        } else {
            String::new()
        };
        zlog_ui::layout::toolbar(ui, &[(&areas, false), (&unknown, false)], |ui| {
            let glyph = zlog_ui::icon::MAP_TRIFOLD;
            let label = texts().get(lang, "background");
            if zlog_ui::layout::toggle(ui, glyph, label, self.show_tiles).clicked() {
                self.show_tiles = !self.show_tiles;
            }
        });
        self.prepare(&worked);
        if self.show_tiles && self.tiles.is_none() {
            let options = HttpOptions {
                user_agent: Some(HeaderValue::from_static(zlog_net::USER_AGENT)),
                ..HttpOptions::default()
            };
            self.tiles = Some(HttpTiles::with_options(
                OpenStreetMap,
                options,
                ui.ctx().clone(),
            ));
        }
        let tiles = match (&mut self.tiles, self.show_tiles) {
            (Some(t), true) => Some(t as &mut dyn walkers::Tiles),
            _ => None,
        };
        let layer = Areas {
            worked: &worked,
            drawn: &self.drawn,
            hidden: &self.hidden.1,
        };
        let size = ui.available_size().max(egui::vec2(80.0, 80.0));
        let map = Map::new(tiles, &mut self.memory, lon_lat(137.0, 37.5)).with_plugin(layer);
        ui.scope(|ui| {
            let rect = egui::Rect::from_min_size(ui.cursor().min, size);
            ui.set_clip_rect(ui.clip_rect().intersect(rect));
            ui.add_sized(size, map);
        });
        let rect = egui::Rect::from_min_size(ui.min_rect().min, size);
        let line = 18.0;
        let credit = egui::RichText::new(texts().get(lang, "attribution")).small();
        let corner = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 4.0, rect.max.y - line),
            egui::pos2(rect.max.x, rect.max.y),
        );
        ui.put(corner, egui::Label::new(credit).halign(egui::Align::Min));
        if self.show_tiles {
            let link = egui::Hyperlink::from_label_and_url(
                egui::RichText::new(texts().get(lang, "osm")).small(),
                "https://www.openstreetmap.org/copyright",
            );
            let corner = egui::Rect::from_min_max(
                egui::pos2(rect.min.x + 4.0, rect.max.y - 2.0 * line),
                egui::pos2(rect.max.x, rect.max.y - line),
            );
            ui.put(corner, link);
        }
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(140, |_| Maplot::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
