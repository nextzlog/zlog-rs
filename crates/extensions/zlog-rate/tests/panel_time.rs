use std::time::{Duration, Instant};
use zlog_core::{
    Band, Contest, ContestMode, DelphiTime, Log, Mode, Qso, QsoScore, ScoreSheet, Scoring, Station,
};
use zlog_ui::{Panel, View};

struct Plain;

impl Contest for Plain {
    fn name(&self) -> &str {
        "plain"
    }
    fn band_range(&self) -> (Band, Band) {
        (Band::B1_9, Band::HIGHEST)
    }
    fn contest_mode(&self) -> ContestMode {
        ContestMode::All
    }
    fn sent_number(&self, _band: Band, _power: zlog_core::Power, _station: &Station) -> String {
        String::new()
    }
    fn valid_number(&self, _q: &Qso) -> bool {
        true
    }
    fn score(&mut self, log: &Log) -> Scoring {
        Scoring {
            qsos: vec![QsoScore::default(); log.qsos.len()],
            sheet: ScoreSheet::default(),
        }
    }
}

fn input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(520.0, 400.0),
        )),
        ..Default::default()
    }
}

#[test]
fn the_rate_panel_stays_cheap_with_1000_qsos() {
    let now = zlog_rate::count::now(None);
    let bands = [Band::B3_5, Band::B7, Band::B14, Band::B21, Band::B28];
    let log = Log {
        qsos: (0..1000)
            .map(|i| Qso {
                time: DelphiTime(now - 1.0 + i as f64 / 1000.0),
                callsign: format!("JA{}X{i}", i % 10),
                band: bands[i % 5],
                mode: Mode::Cw,
                points: 1,
                ..Qso::default()
            })
            .collect(),
        ..Log::default()
    };
    let mut longer = log.clone();
    longer.qsos.push(log.qsos[999].clone());
    let (sheet, station) = (ScoreSheet::default(), Station::default());
    let view = View {
        log: &log,
        contest: &Plain,
        sheet: &sheet,
        callsign: "",
        station: &station,
        number: "",
        memo: "",
        rst_sent: "599",
        power: zlog_core::Power::P010,
        band: Band::B14,
        mode: Mode::Cw,
        language: "en",
        spots: &[],
        hz: None,
        rig: 1,
        cq: false,
        operator: "",
        period_hours: 24,
        pc_name: "",
        log_path: "",
        focus: None,
        editing: None,
        selected: None,
        period: &(),
        tab: Default::default(),
        changes: 0,
    };
    let mut panel = zlog_rate::Rate::default();
    let ctx = egui::Context::default();
    let t = Instant::now();
    ctx.run_ui(input(), |ui| panel.ui(ui, &view))
        .textures_delta
        .clear();
    let first = t.elapsed();
    let mut best = Duration::MAX;
    for _ in 0..20 {
        let t = Instant::now();
        ctx.run_ui(input(), |ui| panel.ui(ui, &view))
            .textures_delta
            .clear();
        best = best.min(t.elapsed());
    }
    let mut changed = Duration::MAX;
    for i in 0..20 {
        let view = View {
            log: if i % 2 == 0 { &longer } else { &log },
            tab: view.tab.clone(),
            ..view
        };
        let t = Instant::now();
        ctx.run_ui(input(), |ui| panel.ui(ui, &view))
            .textures_delta
            .clear();
        changed = changed.min(t.elapsed());
    }
    eprintln!(
        "rate panel with 1000 QSOs: first frame {first:?}, same log {best:?}, new QSO {changed:?}"
    );
    if !cfg!(debug_assertions) {
        assert!(best < Duration::from_millis(1), "{best:?}");
        assert!(changed < Duration::from_millis(3), "{changed:?}");
    }
}
