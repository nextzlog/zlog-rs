mod support;

use support::{builtin, gen, ported, IDS};
use zlog_core::{Band, ContestMode, DelphiTime, Mode, Qso};

#[test]
fn ported_contests_match_the_builtin_rules_on_generated_logs() {
    for id in IDS {
        let (new, old) = (ported(id), builtin(id));
        let dat = std::fs::read_to_string(support::fixture(&format!("{id}.dat"))).unwrap();
        let codes = gen::codes(&dat);
        let mut rng = gen::Rng::new(77);
        for seed in 100..112u64 {
            let len = rng.below(1001);
            let log = gen::log(seed, len, &codes);
            for (mode, _) in gen::MODES.iter().chain(&[(ContestMode::Phone, "ph")]) {
                let mut a = old.build_rules(*mode);
                let mut b = new.build_rules(*mode);
                let mut l = log.clone();
                support::known(&mut l, &mut b);
                assert_eq!(
                    gen::describe(&log, &mut a),
                    gen::describe(&log, &mut b),
                    "{id} seed {seed} len {len} {mode:?}"
                );
            }
        }
    }
}

#[test]
fn ported_contests_keep_the_multiplier_lists() {
    for id in IDS {
        let (new, old) = (ported(id), builtin(id));
        use zlog_core::Contest;
        assert_eq!(
            new.build_rules(ContestMode::Mix).multi_table(),
            old.build_rules(ContestMode::Mix).multi_table()
        );
        assert_eq!(new.config().contest_name, old.config().contest_name);
    }
}

fn qso(call: &str, band: Band, mode: Mode, nr: &str) -> Qso {
    Qso {
        callsign: call.into(),
        band,
        mode,
        nr_rcvd: nr.into(),
        ..Qso::default()
    }
}

fn points(l: &zlog_core::Log) -> Vec<u8> {
    l.qsos.iter().map(|q| q.points).collect()
}

#[test]
fn sample_fixture_logs() {
    for def in [ported("sample"), builtin("sample")] {
        let (l, sheet) = support::scored(
            &def,
            ContestMode::Mix,
            vec![
                qso("JA1AAA", Band::B7, Mode::Cw, "10T"),
                qso("JA1AAA", Band::B21, Mode::Ssb, "10T"),
                qso("JA1BBB", Band::B7, Mode::Cw, "11C"),
            ],
        );
        assert_eq!(points(&l), vec![3, 1, 3]);
        assert!(l.qsos[0].new_multi2 && !l.qsos[2].new_multi2);
        assert_eq!(sheet.score, 7 * (5 + 1));
        let (l, sheet) = support::scored(
            &def,
            ContestMode::Mix,
            vec![
                qso("JA1AAA", Band::B7, Mode::Cw, "10T"),
                qso("JA1AAA", Band::B7, Mode::Cw, "10T"),
            ],
        );
        assert_eq!(points(&l), vec![3, 0]);
        assert_eq!(sheet.score, 3 * 3);
    }
}

#[test]
fn days_fixture_logs() {
    for def in [ported("days"), builtin("days")] {
        let mut a = qso("JA2AAA", Band::B1200, Mode::Fm, "001");
        a.time = DelphiTime::from_ymd_hms(2026, 1, 2, 10, 0, 0);
        let mut b = qso("JA2BBB/2", Band::B5600, Mode::Fm, "002");
        b.time = DelphiTime::from_ymd_hms(2026, 1, 3, 10, 0, 0);
        let (l, sheet) = support::scored(&def, ContestMode::Mix, vec![a, b]);
        assert_eq!(l.qsos[1].multi1, "B");
        assert_eq!(sheet.score, 10 * 2 * 2);
    }
}
