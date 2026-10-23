//! Map of the JCC, JCG and prefecture codes received, in a window of the application.
//!
//! The map is drawn in the window itself with the `walkers` map widget, from
//! boundaries embedded in the binary, so it works offline; OpenStreetMap tiles can be
//! shown behind it when online. What is drawn is recomputed from the log each frame,
//! so opening a file, editing or deleting a QSO is reflected at once.
//!
//! `data/shapes.bin` and `data/jcc.tsv` are built by `tools/build_data.py`; the
//! version is on the first line of `jcc.tsv`. The data has no boundaries for
//! Kanagawa, Wakayama and Kagoshima. The boundaries are simplified from the National
//! Land Numerical Information administrative boundaries; credit as their terms
//! require: 出典: 国土交通省国土数値情報（行政区域データ）を加工.

mod data;

use std::collections::{BTreeMap, HashMap};

use egui::{Color32, Pos2, Response, Shape as Paint, Stroke, Ui};
use geo::{Contains, Point, TriangulateEarcut};
use walkers::sources::OpenStreetMap;
use walkers::{lon_lat, HttpTiles, Map, MapMemory, Plugin, Projector};
use zlog_ui::{Panel, View};

pub use data::{map_data, source, worked_codes};

zlog_core::texts!();

/// Credit required by the terms of use of the boundary data, in `language`.
pub fn attribution(language: &str) -> &'static str {
    texts().get(language, "attribution")
}

const FILL: Color32 = Color32::from_rgba_premultiplied(120, 0, 0, 60);
const EDGE: Color32 = Color32::from_rgb(200, 0, 0);
const BORDER: Color32 = Color32::from_gray(150);

pub struct Maplot {
    memory: MapMemory,
    /// OpenStreetMap tiles, created when first switched on.
    tiles: Option<HttpTiles>,
    show_tiles: bool,
    /// Triangles of each shape in degrees, made on first use.
    triangles: HashMap<usize, Vec<[f64; 2]>>,
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
            triangles: HashMap::new(),
        }
    }
}

impl Maplot {
    fn triangulate(&mut self, shapes: &[usize]) {
        for &s in shapes {
            self.triangles.entry(s).or_insert_with(|| {
                map_data().shapes[s]
                    .polygons
                    .iter()
                    .flat_map(|p| p.earcut_triangles())
                    .flat_map(|t| t.to_array().map(|c| [c.x, c.y]))
                    .collect()
            });
        }
    }
}

/// A worked code, its area and how many QSOs gave it.
type Worked = (String, &'static data::Area, usize);

/// The layer drawn on top of the map: every boundary, and worked areas filled.
struct Areas<'a> {
    worked: &'a [Worked],
    triangles: &'a HashMap<usize, Vec<[f64; 2]>>,
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

        for (_, area, _) in self.worked {
            for s in &area.shapes {
                let mut mesh = egui::Mesh::default();
                for p in &self.triangles[s] {
                    mesh.colored_vertex(to_screen(p[0], p[1]), FILL);
                }
                mesh.indices = (0..mesh.vertices.len() as u32).collect();
                painter.add(Paint::mesh(mesh));
            }
        }
        for shape in &data.shapes {
            for polygon in &shape.polygons {
                let pts = outline(polygon.exterior().points().map(|p| to_screen(p.x(), p.y())));
                if pts.len() >= 3 {
                    painter.extend(segments(&pts, Stroke::new(0.4, BORDER)));
                }
            }
        }
        for (_, area, _) in self.worked {
            for &s in &area.shapes {
                for polygon in &data.shapes[s].polygons {
                    let pts = outline(polygon.exterior().points().map(|p| to_screen(p.x(), p.y())));
                    painter.extend(segments(&pts, Stroke::new(1.0, EDGE)));
                }
            }
        }

        // Name of the worked area under the pointer.
        if let Some(pos) = response.hover_pos() {
            let at = projector.unproject(pos.to_vec2());
            let point = Point::new(at.x(), at.y());
            let hit = self.worked.iter().find(|(_, area, _)| {
                area.shapes
                    .iter()
                    .any(|&s| data.shapes[s].polygons.iter().any(|p| p.contains(&point)))
            });
            if let Some((code, area, count)) = hit {
                response
                    .clone()
                    .on_hover_text_at_pointer(format!("{code}  {}  ({count})", area.name));
            }
        }
    }
}

/// A closed ring as separate segments. A path would join them with mitres, which at
/// near-reversals of the boundary shoot out as long spikes.
fn segments(pts: &[Pos2], stroke: Stroke) -> impl Iterator<Item = Paint> + '_ {
    (0..pts.len()).map(move |i| Paint::line_segment([pts[i], pts[(i + 1) % pts.len()]], stroke))
}

/// Screen points of a ring, without points closer than a pixel to the previous one.
fn outline(points: impl Iterator<Item = Pos2>) -> Vec<Pos2> {
    let mut out: Vec<Pos2> = Vec::new();
    for q in points {
        if out.last().is_none_or(|last| last.distance(q) >= 1.0) {
            out.push(q);
        }
    }
    out
}

impl Panel for Maplot {
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
        ui.horizontal(|ui| {
            ui.label(texts().format(lang, "areas", &[&worked.len()]));
            if unknown > 0 {
                ui.label(texts().format(lang, "unknown", &[&unknown]));
            }
            ui.checkbox(&mut self.show_tiles, texts().get(lang, "background"));
        });
        ui.weak(format!("{}  /  {}", source(), attribution(lang)));

        let shapes: Vec<usize> = worked
            .iter()
            .flat_map(|(_, a, _)| a.shapes.iter().copied())
            .collect();
        self.triangulate(&shapes);
        if self.show_tiles && self.tiles.is_none() {
            self.tiles = Some(HttpTiles::new(OpenStreetMap, ui.ctx().clone()));
        }
        let tiles = match (&mut self.tiles, self.show_tiles) {
            (Some(t), true) => Some(t as &mut dyn walkers::Tiles),
            _ => None,
        };
        let layer = Areas {
            worked: &worked,
            triangles: &self.triangles,
        };
        ui.add(Map::new(tiles, &mut self.memory, lon_lat(137.0, 37.5)).with_plugin(layer));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
