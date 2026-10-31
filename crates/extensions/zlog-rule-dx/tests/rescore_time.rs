use std::sync::Arc;
use std::time::{Duration, Instant};
use zlog_core::{rescore, Band, ContestMode, Log, Mode, Qso};
use zlog_rule_dx::cty::Cty;
use zlog_rule_dx::{WorldContest, WorldRules};

fn cty() -> String {
    let conts = ["AS", "EU", "NA", "SA", "AF", "OC"];
    let mut s = String::from("Japan: 25: 45: AS: 36.40: -138.38: -9.0: JA:\n    JA,JE,JR,7K;\n");
    for i in 0..340 {
        let code = format!(
            "{}{}",
            (b'A' + (i / 26) as u8) as char,
            (b'A' + (i % 26) as u8) as char
        );
        let prefixes: Vec<String> = (0..30).map(|k| format!("{code}{k}")).collect();
        s += &format!(
            "C{i}: {}: {}: {}: 0: 0: 0: {code}:\n    {};\n",
            i % 40 + 1,
            i % 75 + 1,
            conts[i % 6],
            prefixes.join(",")
        );
    }
    s
}

fn time(kind: WorldContest, log: &mut Log, cty: &Arc<Cty>) -> Duration {
    let mut w = WorldRules::new(kind, ContestMode::Cw, cty.clone());
    rescore(log, &mut w).unwrap();
    let t = Instant::now();
    for _ in 0..10 {
        rescore(log, &mut w).unwrap();
    }
    t.elapsed() / 10
}

#[test]
fn arrl_dx_rescores_as_fast_as_the_other_world_contests() {
    let cty = Arc::new(Cty::parse(&cty()));
    let bands = [Band::B3_5, Band::B7, Band::B14, Band::B21, Band::B28];
    let states = ["MA", "CT", "NY", "TX", "CA", "ON", "BC", "QC", "FL", "WA"];
    let mut log = Log::default();
    log.header.callsign = "JA1ZLO".into();
    for i in 0..1000usize {
        let code = format!(
            "{}{}",
            (b'A' + ((i % 340) / 26) as u8) as char,
            (b'A' + (i % 26) as u8) as char
        );
        log.qsos.push(Qso {
            callsign: format!("{code}{}X{i}", i % 30),
            band: bands[i % 5],
            mode: Mode::Cw,
            nr_rcvd: states[i % 10].into(),
            nr_sent: "25".into(),
            ..Qso::default()
        });
    }
    let arrl = time(WorldContest::ArrlDxDx, &mut log, &cty);
    let cqww = time(WorldContest::Cqww, &mut log, &cty);
    eprintln!(
        "1000 QSOs: {} {arrl:?}, {} {cqww:?}",
        WorldContest::ArrlDxDx.id(),
        WorldContest::Cqww.id()
    );
    if cfg!(debug_assertions) {
        return;
    }
    assert!(arrl < Duration::from_millis(5), "{arrl:?}");
    assert!(
        arrl < cqww * 4 + Duration::from_micros(500),
        "{arrl:?} vs {cqww:?}"
    );
}
