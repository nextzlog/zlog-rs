use std::sync::Arc;
use std::time::{Duration, Instant};
use zlog_core::{Band, Contest, ContestMode, Log, Mode, Qso, Station};
use zlog_rule_dx::cty::Cty;
use zlog_rule_dx::{WorldContest, WorldRules};
use zlog_ui::{Panel, View};

fn cty() -> String {
    let conts = ["AS", "EU", "NA", "SA", "AF", "OC"];
    let mut s = String::from("Japan: 25: 45: AS: 36.40: -138.38: -9.0: JA:\n    JA,JE,JR,7K;\n");
    for i in 0..340 {
        let code = format!(
            "{}{}",
            (b'A' + (i / 26) as u8) as char,
            (b'A' + (i % 26) as u8) as char
        );
        s += &format!(
            "C{i}: {}: {}: {}: 0: 0: 0: {code}:\n    {code};\n",
            i % 40 + 1,
            i % 75 + 1,
            conts[i % 6]
        );
    }
    s
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
fn score_panels_stay_cheap_with_1000_qsos() {
    let mut contest = WorldRules::new(
        WorldContest::Cqww,
        ContestMode::Cw,
        Arc::new(Cty::parse(&cty())),
    );
    let bands = [Band::B3_5, Band::B7, Band::B14, Band::B21, Band::B28];
    let mut log = Log::default();
    log.header.callsign = "JA1ZLO".into();
    for i in 0..1000usize {
        let code = format!(
            "{}{}",
            (b'A' + ((i % 340) / 26) as u8) as char,
            (b'A' + (i % 26) as u8) as char
        );
        log.qsos.push(Qso {
            callsign: format!("{code}{}X{i}", i % 10),
            band: bands[i % 5],
            mode: Mode::Cw,
            nr_rcvd: format!("{}", i % 40 + 1),
            ..Qso::default()
        });
    }
    zlog_core::rescore(&mut log, &mut contest).unwrap();
    let sheet = contest.score(&log).sheet;
    let station = Station::default();
    let view = View {
        log: &log,
        contest: &contest,
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
    let ctx = egui::Context::default();
    let panels: Vec<(&str, Box<dyn Panel>)> = vec![
        ("score", Box::new(zlog_score::Score)),
        ("multipliers", Box::new(zlog_score::Multipliers::default())),
    ];
    for (name, mut panel) in panels {
        ctx.run_ui(input(), |ui| panel.ui(ui, &view))
            .textures_delta
            .clear();
        let mut best = Duration::MAX;
        for _ in 0..20 {
            let t = Instant::now();
            ctx.run_ui(input(), |ui| panel.ui(ui, &view))
                .textures_delta
                .clear();
            best = best.min(t.elapsed());
        }
        eprintln!("{name} panel frame with 1000 QSOs: {best:?}");
        if !cfg!(debug_assertions) {
            assert!(best < Duration::from_millis(2), "{name} {best:?}");
        }
    }
}
