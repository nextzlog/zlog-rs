use std::time::Instant;

use zlog_cfg::UserDefinedDef;
use zlog_core::{Band, ContestMode, DelphiTime, Log, Mode, Qso};

const CFG: &str = "#Timing Contest\npower -HHHHHHMM----\npt3.5 3333\npt7 3333\npt14 3333\npt21 3333\npt28 3333\npt50 3333\npt144 3333\npt430 3333\ndat timing\ncut 0\nsendnr $Q\nexit ;\n";

fn codes() -> Vec<String> {
    (0..1300)
        .map(|i| format!("{:02}{:02}{:02}", 1 + i / 400, i / 20 % 20, i % 20))
        .collect()
}

fn dat() -> String {
    let mut text = String::from("#Timing Contest\n");
    for (i, code) in codes().iter().enumerate() {
        text.push_str(&format!("{code} Area {i}\n"));
    }
    text
}

fn plain_def() -> UserDefinedDef {
    UserDefinedDef::from_cfg(CFG, |_| Some(dat()))
}

pub fn log(len: usize, codes: &[String]) -> Log {
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = |n: usize| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % n as u64) as usize
    };
    let bands = [
        Band::B3_5,
        Band::B7,
        Band::B14,
        Band::B21,
        Band::B50,
        Band::B144,
        Band::B430,
    ];
    let modes = [Mode::Cw, Mode::Ssb, Mode::Fm];
    let qsos = (0..len)
        .map(|i| Qso {
            time: DelphiTime(46000.0 + i as f64 / 1440.0),
            callsign: format!("JA{}{}", next(10), ["ABC", "XYZ", "QQ", "ZLO"][next(4)]),
            nr_rcvd: format!("{}{}", codes[next(codes.len())], ["", "M", "H"][next(3)]),
            band: bands[next(bands.len())],
            mode: modes[next(modes.len())],
            serial: i as i32 + 1,
            ..Qso::default()
        })
        .collect();
    Log {
        qsos,
        ..Log::default()
    }
}

#[test]
fn plain_cfg_rescores_1000_qsos_fast() {
    let def = plain_def();
    assert!(def.dat_loaded() && !def.has_script());
    let codes = codes();
    let mut log = log(1000, &codes);
    let mut contest = def.build_rules(ContestMode::Mix);
    let first = zlog_core::rescore(&mut log, &mut contest).unwrap();
    assert!(first.score > 0);
    let rounds = 50;
    let t = Instant::now();
    for _ in 0..rounds {
        let sheet = zlog_core::rescore(&mut log, &mut contest).unwrap();
        assert_eq!(sheet, first);
    }
    let ms = t.elapsed().as_secs_f64() * 1e3 / rounds as f64;
    println!("plain CFG: rescore of 1000 QSOs {ms:.3} ms");
    if !cfg!(debug_assertions) {
        assert!(ms <= 1.0, "rescore {ms} ms");
    }
}
