use zlog_core::{Band, ContestMode, DelphiTime, Mode, Power, Station};
use zlog_gui::testing::entry::*;
use zlog_rule_ja::JarlContest;

fn init() {
    zlog_gui::testing::registry::install(zlog_max::register);
}

fn entry(band: Band, mode: Mode) -> Entry {
    Entry::new(band, mode, Station::default_power(band))
}

#[test]
fn band_cycle_skips_warc_wraps_and_sets_power() {
    init();
    let c = JarlContest::AllJa.build_rules(ContestMode::Mix);
    let st = Station::default();
    let mut e = entry(Band::B7, Mode::Cw);
    e.cycle_band(&c, &st, false);
    assert_eq!(e.band, Band::B14);
    e.band = Band::B50;
    e.cycle_band(&c, &st, false);
    assert_eq!(e.band, Band::B1_9);
    e.cycle_band(&c, &st, true);
    assert_eq!(e.band, Band::B50);
    let six = JarlContest::SixDown.build_rules(ContestMode::Mix);
    e.band = Band::B430;
    e.cycle_band(&six, &st, false);
    assert_eq!((e.band, e.power), (Band::B1200, Power::P002));
}

#[test]
fn mode_cycle_respects_the_band() {
    init();
    let mut e = entry(Band::B7, Mode::Cw);
    e.cycle_mode(ContestMode::Mix, false, false);
    assert_eq!((e.mode, e.rst_sent.as_str()), (Mode::Ssb, "59"));
    e.cycle_mode(ContestMode::Mix, false, false);
    assert_eq!(e.mode, Mode::Cw, "SSB is the top on 7 MHz");
    e.band = Band::B28;
    e.mode = Mode::Ssb;
    e.cycle_mode(ContestMode::Mix, false, false);
    assert_eq!(e.mode, Mode::Fm);
    e.cycle_mode(ContestMode::All, false, false);
    assert_eq!(e.mode, Mode::Am);
}

#[test]
fn mode_change_rst_follows_update_mode() {
    init();
    for (m, rst) in [
        (Mode::Cw, 599),
        (Mode::Ssb, 59),
        (Mode::Fm, 59),
        (Mode::Am, 59),
        (Mode::Dv, 59),
        (Mode::Rtty, 599),
        (Mode::Ft4, 599),
        (Mode::Ft8, 599),
        (Mode::Other, 599),
    ] {
        assert_eq!(default_rst(m), rst, "{m:?}");
        let mut e = entry(Band::B7, Mode::Cw);
        e.set_mode(Mode::Ssb);
        e.set_mode(m);
        assert_eq!(e.rst_sent, rst.to_string(), "{m:?}");
        assert_eq!(e.rst_rcvd, rst.to_string(), "{m:?}");
    }
}

#[test]
fn logged_rst_follows_the_post_log_reset() {
    init();
    for (m, rst) in [
        (Mode::Cw, "599"),
        (Mode::Rtty, "599"),
        (Mode::Ft8, "59"),
        (Mode::Ft4, "59"),
        (Mode::Other, "59"),
        (Mode::Ssb, "59"),
    ] {
        let mut e = entry(Band::B7, m);
        e.callsign = "JA1ZLO".into();
        e.wipe();
        assert_eq!(
            (e.rst_sent.as_str(), e.rst_rcvd.as_str()),
            (rst, rst),
            "{m:?}"
        );
        assert_eq!(e.callsign, "");
    }
}

#[test]
fn set_mode_to_the_same_mode_keeps_typed_rst() {
    init();
    let mut e = entry(Band::B7, Mode::Cw);
    e.rst_rcvd = "579".into();
    e.set_mode(Mode::Cw);
    assert_eq!(e.rst_rcvd, "579");
}

#[test]
fn all_modes_reach_dv_and_wrap() {
    init();
    let mut e = entry(Band::B7, Mode::Ft8);
    e.cycle_mode(ContestMode::Mix, true, false);
    assert_eq!(e.mode, Mode::Dv, "Pedition goes up to DV");
    e.cycle_mode(ContestMode::Mix, true, false);
    assert_eq!(e.mode, Mode::Other);
    e.cycle_mode(ContestMode::Mix, true, false);
    assert_eq!(e.mode, Mode::Cw);
    e.cycle_mode(ContestMode::Mix, true, true);
    assert_eq!(e.mode, Mode::Other);
    e.cycle_mode(ContestMode::Mix, true, true);
    assert_eq!(e.mode, Mode::Dv);
    let mut e = entry(Band::B7, Mode::Cw);
    e.cycle_mode(ContestMode::Mix, false, true);
    assert_eq!(e.mode, Mode::Ssb);
}

#[test]
fn power_cycles_h_p_l_m() {
    init();
    let mut e = entry(Band::B7, Mode::Cw);
    let codes: Vec<char> = (0..4)
        .map(|_| {
            e.cycle_power(false);
            e.power.code().unwrap()
        })
        .collect();
    assert_eq!(codes, vec!['P', 'L', 'M', 'H']);
}

#[test]
fn s_figure_goes_round() {
    init();
    let mut r = "599".to_string();
    Entry::bump_s(&mut r, true);
    assert_eq!(r, "519");
    Entry::bump_s(&mut r, false);
    assert_eq!(r, "599");
    let mut r = "51".to_string();
    Entry::bump_s(&mut r, false);
    assert_eq!(r, "59");
    let mut r = "65535".to_string();
    Entry::bump_s(&mut r, true);
    assert_eq!(r, "65545");
}

#[test]
fn clear_keeps_rst() {
    init();
    let mut e = entry(Band::B7, Mode::Cw);
    e.callsign = "JA1ZLO".into();
    e.rst_rcvd = "579".into();
    e.clear_call_and_number();
    assert_eq!((e.callsign.as_str(), e.rst_rcvd.as_str()), ("", "579"));
}

#[test]
fn to_qso_fills_sent_number_and_power() {
    init();
    let c = JarlContest::AllJa.build_rules(ContestMode::Mix);
    let st = Station {
        prov: "10".into(),
        operator: "OP".into(),
        tx_nr: 2,
        ..Station::default()
    };
    let mut e = entry(Band::B7, Mode::Cw);
    e.power = Power::P005;
    e.cq = true;
    e.callsign = "ja1zlo ".into();
    e.nr_rcvd = "25h".into();
    let q = e.to_qso(&c, &st, DelphiTime(1.0), 7);
    assert_eq!(
        (q.callsign.as_str(), q.nr_sent.as_str(), q.nr_rcvd.as_str()),
        ("JA1ZLO", "10M", "25H")
    );
    assert_eq!(
        (q.rst_sent, q.power, q.operator.as_str(), q.reserve3),
        (599, Power::P005, "OP", 7)
    );
    assert_eq!((q.cq, q.tx), (true, 2));
    assert_eq!(q.serial, 0, "ALL JA has no serial numbers");
    e.wipe();
    assert!(e.cq, "CQ mode survives clearing the row");
}

#[test]
fn power_back_cycles_m_l_p_h() {
    init();
    let mut e = entry(Band::B7, Mode::Cw);
    let codes: Vec<char> = (0..4)
        .map(|_| {
            e.cycle_power(true);
            e.power.code().unwrap()
        })
        .collect();
    assert_eq!(codes, vec!['M', 'L', 'P', 'H']);
}
