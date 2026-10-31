use zlog_cfg::cfg::{Config, PrefixMulti, SerialType};
use zlog_cfg::from_cfg;
use zlog_core::{rescore, Band, Contest, ContestDef, ContestMode, Log, Mode, MultiCheck, Qso};
use zlog_rule_ja::ja0::Ja0Contest;
use zlog_rule_ja::nyp::NypContest;
use zlog_rule_ja::JapanContest;

fn qso(call: &str, band: Band, mode: Mode, nr: &str) -> Qso {
    Qso {
        callsign: call.into(),
        band,
        mode,
        nr_rcvd: nr.into(),
        ..Qso::default()
    }
}

fn log(qsos: Vec<Qso>) -> Log {
    Log {
        qsos,
        ..Log::default()
    }
}

const LOCAL_CFG: &str = "\
; sample\r
#Sample Local Contest\r
f1_a\tCQ TEST DE $M $M TEST;\tz repeats\r
f3_b\tGL\r
power\t--H-HHHMM----;\t\t1.9 MHz ... 10.1GHz&Up\r
dat\tSAMPLE;\tDAT FILE\r
spc\tzlog;\t\tSPC FILE\r
cut\t1;\t\t-x[n] option\r
mode\ton;\r
sendnr\t$Q$P;\r
";

const SAMPLE_DAT: &str = "\
SAMPLE.DAT for test
204        Asahikawa
214        Wakkanai
101,102    Two codes
end of file
999        never read
";

fn local() -> zlog_cfg::UserDefinedDef {
    from_cfg(LOCAL_CFG, |name| {
        (name == "SAMPLE.DAT").then(|| SAMPLE_DAT.into())
    })
}

#[test]
fn cfg_keywords() {
    let c = Config::parse(LOCAL_CFG);
    assert_eq!(c.contest_name, "Sample Local Contest");
    assert_eq!(c.dat, "SAMPLE.DAT");
    assert_eq!(c.cut, 1);
    assert!(c.accept_different_mode);
    assert_eq!(c.sent, "$Q$P");
    assert_eq!(c.cw_message_a[0], "CQ TEST DE $M $M TEST");
    assert_eq!(c.cw_message_b[2], "GL");
    assert_eq!((c.band_low(), c.band_high()), (Band::B7, Band::B430));
    assert!(!c.band_available(Band::B14));
    assert_eq!(c.ignored, vec!["SPC".to_string()]);
}

#[test]
fn cfg_points_and_options() {
    let c = Config::parse(
        "#X\npt7\t23\nxpt21\t0510\nexpt50\t1,2,3,4,5,6,7,8,9\ndefaultpt\t5,0,4,0\n\
         alphapt\tA1B2X3J10\nserial\tband\npxmulti\twpx\nsameexchange off\nlocal 10,11\n\
         cty\t;\ntime utc\nCOEFF ON\nexit x\nprov 99\n",
    );
    let at = |b, m| Config::at(&c.points, b, m);
    assert_eq!((at(Band::B7, Mode::Ssb), at(Band::B7, Mode::Cw)), (2, 3));
    assert_eq!((at(Band::B21, Mode::Ssb), at(Band::B21, Mode::Cw)), (5, 10));
    assert_eq!(at(Band::B50, Mode::Dv), 8);
    assert_eq!(at(Band::B50, Mode::Other), 9);
    assert_eq!((at(Band::B1_9, Mode::Cw), at(Band::B1_9, Mode::Fm)), (5, 0));
    assert_eq!(
        (at(Band::B3_5, Mode::Cw), at(Band::B3_5, Mode::Ssb)),
        (4, 0)
    );
    let a = c.alphabet_points.unwrap();
    assert_eq!(
        (a[(b'A' - b'0') as usize], a[(b'J' - b'0') as usize]),
        (1, 10)
    );
    assert_eq!(c.serial, SerialType::Band);
    assert_eq!(c.prefix_multi, PrefixMulti::Wpx);
    assert!(!c.same_exchange && c.use_cty && c.use_utc && c.coeff);
    assert_eq!(c.local.len(), 2);
    assert_eq!(c.prov, "", "EXIT stops reading");
}

#[test]
fn local_contest_score() {
    let def = local();
    assert!(def.dat_loaded());
    assert_eq!(def.caption("ja"), "Sample Local Contest");
    assert_eq!(def.group("en"), "Local");
    let mut c = def.build_rules(ContestMode::Mix);
    let mut l = log(vec![
        qso("JA8AAA", Band::B7, Mode::Cw, "204H"),
        qso("JA8AAA", Band::B7, Mode::Ssb, "204H"),
        qso("JA8AAA", Band::B7, Mode::Cw, "204H"),
        qso("JA8BBB", Band::B21, Mode::Fm, "102M"),
        qso("JA8CCC", Band::B21, Mode::Cw, "555M"),
        qso("JA8DDD", Band::B1200, Mode::Fm, "214L"),
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    assert_eq!(l.qsos[0].multi1, "204");
    assert!(l.qsos[0].new_multi1 && !l.qsos[1].new_multi1);
    assert!(l.qsos[2].dupe && l.qsos[2].points == 0);
    assert!(l.qsos[3].new_multi1);
    assert!(!l.qsos[4].new_multi1 && l.qsos[4].points == 1);
    assert_eq!(l.qsos[5].points, 0);
    let t = sheet.total;
    assert_eq!((t.qsos, t.cw_qsos, t.points, t.multi), (5, 3, 4, 2));
    assert_eq!(sheet.score, 8);
    assert!(!sheet.bands.contains_key(&Band::B14));
    assert!(c.valid_number(&qso("X", Band::B7, Mode::Cw, "214H")));
    assert!(!c.valid_number(&qso("X", Band::B7, Mode::Cw, "300H")));
    match c.check_multi(&qso("X", Band::B21, Mode::Cw, "204M")) {
        MultiCheck::Known {
            name,
            needed,
            worked_on,
            ..
        } => {
            assert_eq!(name, "Asahikawa");
            assert!(needed);
            assert_eq!(worked_on, vec![Band::B7]);
        }
        MultiCheck::Invalid => panic!("204 is listed"),
    }
    assert_eq!(c.multi_table().len(), 3, "the list stops at end of file");
    assert_eq!(
        c.exchange_from(Band::B21, &l.qsos[0]),
        Some("204H".to_string())
    );
}

#[test]
fn missing_dat_is_reported() {
    let def = from_cfg(LOCAL_CFG, |_| None);
    assert!(!def.dat_loaded());
}

#[test]
fn local_points_alphabet_and_special_calls() {
    let cfg = "#A\npower HHHHHH\nlocal 01\nlocalpt 3,2\nspecialcalls JA1ZZZ\n\
               specialcallpt 10,10\nundefmulti on\njarl on\n";
    let mut c = from_cfg(cfg, |_| None).build_rules(ContestMode::Mix);
    let mut l = log(vec![
        qso("JA1AAA", Band::B1_9, Mode::Cw, "01H"),
        qso("JA1BBB", Band::B1_9, Mode::Ssb, "02H"),
        qso("JA1ZZZ", Band::B1_9, Mode::Fm, "02M"),
        qso("JA1CCC", Band::B3_5, Mode::Cw, "1H"),
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    let pts: Vec<u8> = l.qsos.iter().map(|q| q.points).collect();
    assert_eq!(pts, vec![3, 1, 10, 1]);
    assert_eq!(l.qsos[3].multi1, "");
    assert_eq!(
        sheet.total.multi, 2,
        "an empty multiplier counts 0 (CommaText)"
    );
    assert_eq!(sheet.score, 15 * 2);

    let cfg = "#B\npower HHHHHH\nalphapt A1B2?5\nnomulti on\n";
    let mut c = from_cfg(cfg, |_| None).build_rules(ContestMode::Mix);
    let mut l = log(vec![
        qso("JA1AAA", Band::B7, Mode::Cw, "10A"),
        qso("JA1BBB", Band::B7, Mode::Cw, "10B"),
        qso("JA1CCC", Band::B7, Mode::Cw, "10Z"),
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    assert_eq!((sheet.total.points, sheet.score), (8, 8));
}

#[test]
fn count_higher_points() {
    let cfg = "#C\npower HHHHHH\nmode on\ncounthigh on\nexpt7 1,3\nundefmulti on\n";
    let mut c = from_cfg(cfg, |_| None).build_rules(ContestMode::Mix);
    let mut l = log(vec![
        qso("JA1AAA", Band::B7, Mode::Cw, "10"),
        qso("JA1AAA", Band::B7, Mode::Ssb, "10"),
        qso("JA1AAA", Band::B7, Mode::Fm, "10"),
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    let pts: Vec<u8> = l.qsos.iter().map(|q| q.points).collect();
    assert_eq!(pts, vec![0, 3, 0]);
    assert_eq!(sheet.total.points, 3);
}

#[test]
fn serial_contest_has_no_exchange_on_space() {
    let mut c = from_cfg("#D\nserial all\nsendnr $S\n", |_| None).build_rules(ContestMode::Mix);
    let past = qso("JA1AAA", Band::B7, Mode::Cw, "001");
    assert_eq!(c.exchange_from(Band::B14, &past), None);
    let st = zlog_core::Station::default();
    assert_eq!(c.sent_number(Band::B7, zlog_core::Power::P010, &st), "001");
    assert_eq!(
        c.sent_number_with_serial(Band::B7, zlog_core::Power::P010, &st, Some(7)),
        "007"
    );
    let _ = c.score(&Log::default());
}

#[test]
fn prefix_multipliers() {
    let mut c = from_cfg("#E\npxmulti normal\n", |_| None).build_rules(ContestMode::Mix);
    let mut l = log(vec![
        qso("JA1ZLO/3", Band::B7, Mode::Cw, ""),
        qso("JD1/JA1ABC", Band::B7, Mode::Cw, ""),
    ]);
    rescore(&mut l, &mut c).unwrap();
    assert_eq!(l.qsos[0].multi1, "JA1");
    assert_eq!(l.qsos[1].multi1, "JA1");
    assert!(l.qsos[0].new_multi1 && !l.qsos[1].new_multi1);
    use zlog_cfg::user::wpx_prefix;
    assert_eq!(wpx_prefix("JA1ZLO/2"), "JA2");
    assert_eq!(wpx_prefix("AD6AJ/PA"), "PA0");
    assert_eq!(wpx_prefix("KH0/AD6AJ"), "KH0");
    assert_eq!(wpx_prefix("XEFTA"), "XE0");
    assert_eq!(wpx_prefix("W1AW/P"), "W1");
}

#[test]
fn ja0_others() {
    let mut c = Ja0Contest::new(false, ContestMode::Mix);
    let mut l = log(vec![
        qso("JA0ABC", Band::B7, Mode::Cw, "001"),
        qso("JR0XYC", Band::B7, Mode::Cw, "002"),
        qso("JA1AAA", Band::B7, Mode::Cw, "003"),
        qso("JA0ABC", Band::B7, Mode::Cw, "004"),
        qso("JA0ABC", Band::B14, Mode::Cw, "005"),
        qso("JA0ABC", Band::B21, Mode::Ssb, "006"),
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    assert_eq!(l.qsos[0].multi1, "A*C");
    assert_eq!(l.qsos[1].multi1, "R*C");
    let pts: Vec<u8> = l.qsos.iter().map(|q| q.points).collect();
    assert_eq!(pts, vec![3, 3, 1, 0, 0, 3]);
    assert_eq!(l.qsos[3].multi1, "");
    let t = sheet.total;
    assert_eq!((t.qsos, t.points, t.multi), (5, 10, 3));
    assert_eq!(sheet.score, 30);
    assert_eq!(sheet.bands.len(), 5);
    assert_eq!(c.exchange_from(Band::B7, &l.qsos[0]), None);
}

#[test]
fn ja0_zero_counts_three_characters() {
    let c = Ja0Contest::new(true, ContestMode::Mix);
    assert_eq!(c.prefix("JA1ABC"), "JA1");
    let st = zlog_core::Station::default();
    assert_eq!(c.sent_number(Band::B7, zlog_core::Power::P010, &st), "001");
}

#[test]
fn nyp_counts_qsos() {
    let mut c = NypContest::new(ContestMode::Mix);
    let mut l = log(vec![
        qso("JA1AAA", Band::B10, Mode::Cw, "TARO"),
        qso("JA1AAA", Band::B10, Mode::Ssb, "TARO"),
        qso("JA1AAA", Band::B10, Mode::Cw, "TARO"),
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    assert!(l.qsos[2].dupe && !l.qsos[1].dupe);
    assert!(l.qsos.iter().all(|q| q.points == 1));
    assert_eq!((sheet.total.qsos, sheet.score), (3, 0));
}

#[test]
fn builtin_captions() {
    assert_eq!(JapanContest::NewYearParty.caption("en"), "New Year Party");
    assert_eq!(JapanContest::Ja0Zero.group("ja"), "国内");
    assert_eq!(
        JapanContest::from_id("ja0-out"),
        Some(JapanContest::Ja0Others)
    );
}

#[test]
fn real_cfg_samples() {
    let Ok(dir) = std::env::var("ZLOG_CFGDAT_DIR") else {
        return;
    };
    let read =
        |p: &std::path::Path| String::from_utf8_lossy(&std::fs::read(p).unwrap()).into_owned();
    let mut seen = 0;
    for sub in std::fs::read_dir(&dir).unwrap() {
        let sub = sub.unwrap().path();
        for f in std::fs::read_dir(&sub).unwrap() {
            let f = f.unwrap().path();
            let ext = f
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if ext != "cfg" {
                continue;
            }
            let text = read(&f);
            let def = from_cfg(&text, |name| {
                std::fs::read_dir(&sub)
                    .unwrap()
                    .map(|e| e.unwrap().path())
                    .find(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.eq_ignore_ascii_case(name))
                    })
                    .map(|p| read(&p))
            });
            assert!(def.dat_loaded(), "{}", f.display());
            let c = def.config();
            assert!(c.band_low() <= c.band_high(), "{}", f.display());
            if f.ends_with("arrl160us.cfg") {
                assert!(c.use_cty);
                assert_eq!(c.no_country_multi, "K,VE");
                assert_eq!(Config::at(&c.local_points, Band::B1_9, Mode::Cw), 2);
                assert_eq!(Config::at(&c.points, Band::B1_9, Mode::Ssb), 0);
            }
            if f.ends_with("JA8_50th.CFG") {
                assert_eq!(c.cut, 1);
                assert_eq!(c.band_high(), Band::B1200);
                assert!(c.alphabet_points.is_some());
            }
            seen += 1;
        }
    }
    assert!(seen >= 6);
}

fn invalid(mut x: Qso) -> Qso {
    x.invalid = true;
    x
}

fn contest_id(c: &dyn Contest) -> String {
    let x = qso("JA1AAA", Band::B7, Mode::Cw, "1");
    c.adif_fields(&x)
        .into_iter()
        .find(|(k, _)| k == "contest_id")
        .map(|(_, v)| v)
        .unwrap_or_default()
}

#[test]
fn cfg_period_start_time_and_contest_id() {
    let def = from_cfg(
        "#Period Test\nuseperiod on\nperiod 6\nstarttime 9\ncontestid JA-LOCAL\n",
        |_| None,
    );
    let c = def.build_rules(ContestMode::Mix);
    assert_eq!(c.period_hours(), 6);
    assert!(c.uses_period());
    assert_eq!(c.start_hour(), Some(9));
    assert_eq!(contest_id(&c), "JA-LOCAL");
}

#[test]
fn cfg_without_period_has_none() {
    let def = from_cfg("#No Period\nperiod 6\n", |_| None);
    let c = def.build_rules(ContestMode::Mix);
    assert_eq!(c.period_hours(), 0);
    assert!(!c.uses_period());
    assert_eq!(c.start_hour(), None);
    assert_eq!(contest_id(&c), "");
    let def = from_cfg("#Bad\nuseperiod on\nperiod -5\nstarttime 25\n", |_| None);
    let c = def.build_rules(ContestMode::Mix);
    assert_eq!((c.period_hours(), c.start_hour()), (0, None));
    assert!(c.uses_period());
    let c = from_cfg("#Open\nuseperiod on\nperiod 0\n", |_| None).build_rules(ContestMode::Mix);
    assert!(c.uses_period() && c.period_hours() == 0);
}

#[test]
fn builtin_periods_and_contest_ids() {
    for def in JapanContest::ALL {
        let c = def.build(ContestMode::Mix);
        assert_eq!(c.period_hours(), 0, "{def:?}");
        assert_eq!(c.start_hour(), None);
        assert_eq!(contest_id(c.as_ref()), "JA_DOMESTIC");
    }
}

#[test]
fn ja0_others_mode_category_and_band_edges() {
    let mut c = Ja0Contest::new(false, ContestMode::Cw);
    let mut l = log(vec![
        qso("JA0ABC", Band::B1_9, Mode::Cw, "001"),
        qso("JA0ABD", Band::B28, Mode::Cw, "002"),
        qso("JA0ABE", Band::B21, Mode::Ssb, "003"),
        qso("JA0ABF", Band::B10, Mode::Cw, "004"),
        qso("JA0ABG", Band::B50, Mode::Cw, "005"),
        invalid(qso("JA0ABH", Band::B7, Mode::Cw, "006")),
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    let pts: Vec<u8> = l.qsos.iter().map(|q| q.points).collect();
    assert_eq!(pts, vec![3, 3, 0, 0, 0, 0]);
    assert_eq!((sheet.total.points, sheet.total.multi), (6, 2));
    assert!(!l.qsos[2].new_multi1);
}

#[test]
fn ja0_others_prefix_shapes() {
    let c = Ja0Contest::new(false, ContestMode::Mix);
    assert_eq!(c.prefix("JA0ABC"), "A*C");
    assert_eq!(c.prefix("JA0AB"), "A*B");
    assert_eq!(c.prefix("JA0A1"), "");
    assert_eq!(c.prefix("JA1ABC"), "");
    assert_eq!(c.prefix(""), "");
    assert_eq!(c.prefix("JA0AB1"), "A*B");
}

#[test]
fn ja0_zero_score() {
    let mut c = Ja0Contest::new(true, ContestMode::Mix);
    let mut l = log(vec![
        qso("JA1AAA", Band::B7, Mode::Cw, "001"),
        qso("JA1BBB", Band::B7, Mode::Cw, "002"),
        qso("JR0CCC", Band::B7, Mode::Cw, "003"),
        qso("JA1AAA", Band::B7, Mode::Ssb, "004"),
        qso("JA1AAA", Band::B14, Mode::Cw, "005"),
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    let pts: Vec<u8> = l.qsos.iter().map(|q| q.points).collect();
    assert_eq!(pts, vec![1, 1, 3, 0, 0]);
    assert!(l.qsos[3].dupe);
    assert_eq!((sheet.total.points, sheet.total.multi), (5, 2));
    assert_eq!(sheet.score, 10);
}

#[test]
fn ja0_zero_empty_log() {
    let mut c = Ja0Contest::new(true, ContestMode::Mix);
    let mut l = log(vec![]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    assert_eq!((sheet.total.qsos, sheet.score), (0, 0));
    assert!(c.multi_table().is_empty());
}

#[test]
fn nyp_counts_warc_and_every_band() {
    let mut c = NypContest::new(ContestMode::Mix);
    let mut l = log(vec![
        qso("JA1AAA", Band::B1_9, Mode::Cw, "TARO"),
        qso("JA1AAA", Band::B18, Mode::Cw, "TARO"),
        qso("JA1AAA", Band::B248G, Mode::Fm, "TARO"),
        invalid(qso("JA1BBB", Band::B7, Mode::Cw, "HANAKO")),
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    assert_eq!((sheet.total.qsos, sheet.total.cw_qsos), (3, 2));
    assert_eq!(l.qsos[3].points, 0);
    assert_eq!(sheet.total.multi, 0);
}

#[test]
fn nyp_accepts_any_number() {
    let c = NypContest::new(ContestMode::Mix);
    assert!(c.valid_number(&qso("JA1AAA", Band::B7, Mode::Cw, "")));
    assert!(c.valid_number(&qso("JA1AAA", Band::B7, Mode::Cw, "ANY TEXT")));
    assert!(c.multi_table().is_empty());
    assert_eq!(
        c.exchange_from(Band::B7, &qso("X", Band::B7, Mode::Cw, "A")),
        None
    );
}

fn sent(c: &dyn Contest, band: Band, st: &zlog_core::Station) -> String {
    c.sent_number(band, zlog_core::Power::P010, st)
}

fn with_serial(call: &str, band: Band, serial: i32) -> Qso {
    Qso {
        serial,
        ..qso(call, band, Mode::Cw, "")
    }
}

#[test]
fn ja0_sends_next_serial() {
    let mut c = Ja0Contest::new(false, ContestMode::Mix);
    let st = zlog_core::Station::default();
    assert_eq!(sent(&c, Band::B7, &st), "001");
    assert_eq!(c.next_serial_on(Band::B7), Some(1));
    let mut l = log(vec![
        with_serial("JA0ABC", Band::B7, 1),
        with_serial("JA0ABD", Band::B21, 2),
    ]);
    rescore(&mut l, &mut c).unwrap();
    assert_eq!(sent(&c, Band::B7, &st), "003");
    assert_eq!(sent(&c, Band::B28, &st), "003");
    assert_eq!(c.next_serial_on(Band::B28), Some(3));
}

#[test]
fn ja0_serial_after_qsos_without_stored_serials_is_one() {
    let mut c = Ja0Contest::new(true, ContestMode::Mix);
    let mut l = log(vec![
        qso("JA0ABC", Band::B7, Mode::Cw, "001"),
        qso("JA0ABD", Band::B7, Mode::Cw, "002"),
    ]);
    rescore(&mut l, &mut c).unwrap();
    assert_eq!(c.next_serial_on(Band::B7), Some(1));
}

#[test]
fn ja0_serial_above_999_has_all_digits() {
    let mut c = Ja0Contest::new(false, ContestMode::Mix);
    let mut l = log(vec![with_serial("JA0ABC", Band::B7, 1234)]);
    rescore(&mut l, &mut c).unwrap();
    assert_eq!(sent(&c, Band::B7, &zlog_core::Station::default()), "1235");
}

#[test]
fn cfg_serial_all_and_band() {
    let st = zlog_core::Station::default();
    let l = log(vec![
        with_serial("JA1AAA", Band::B7, 1),
        with_serial("JA1BBB", Band::B7, 2),
        with_serial("JA1CCC", Band::B14, 3),
    ]);
    let mut all = from_cfg("#A\nserial all\nsendnr $S\n", |_| None).build_rules(ContestMode::Mix);
    all.score(&l);
    assert_eq!(sent(&all, Band::B7, &st), "004");
    assert_eq!(sent(&all, Band::B21, &st), "004");
    let l = log(vec![
        with_serial("JA1AAA", Band::B7, 1),
        with_serial("JA1BBB", Band::B7, 2),
        with_serial("JA1CCC", Band::B14, 1),
    ]);
    let mut band = from_cfg("#B\nserial band\nserialstart 5\nsendnr $S\n", |_| None)
        .build_rules(ContestMode::Mix);
    assert_eq!(sent(&band, Band::B7, &st), "005");
    band.score(&l);
    assert_eq!(sent(&band, Band::B7, &st), "003");
    assert_eq!(sent(&band, Band::B14, &st), "002");
    assert_eq!(sent(&band, Band::B21, &st), "001");
    assert_eq!(band.next_serial_on(Band::B21), Some(1));
}

#[test]
fn cfg_without_serial_has_none() {
    let c = from_cfg("#N\nsendnr $V\n", |_| None).build_rules(ContestMode::Mix);
    assert_eq!(c.next_serial_on(Band::B7), None);
    let st = zlog_core::Station::default();
    let s = from_cfg("#N\nsendnr $S\n", |_| None).build_rules(ContestMode::Mix);
    assert_eq!(sent(&s, Band::B7, &st), "");
}

fn station() -> zlog_core::Station {
    zlog_core::Station {
        callsign: "ja1zlo".into(),
        operator: "taro".into(),
        prov: "10".into(),
        city: "100101".into(),
        age: "45".into(),
        cq_zone: "25".into(),
        iaru_zone: "45".into(),
        iota: "as007".into(),
        handle_cw: "TARO".into(),
        handle_ph: "HANAKO".into(),
        ..zlog_core::Station::default()
    }
}

#[test]
fn cfg_sent_variables_from_station() {
    let st = station();
    let get = |t: &str, mode: ContestMode| {
        let c = from_cfg(&format!("#V\nsendnr {t}\n"), |_| None).build_rules(mode);
        c.sent_number(Band::B7, zlog_core::Power::P010, &st)
    };
    assert_eq!(get("$Z", ContestMode::Mix), "25");
    assert_eq!(get("$I", ContestMode::Mix), "45");
    assert_eq!(get("$A", ContestMode::Mix), "45");
    assert_eq!(get("$T", ContestMode::Mix), "AS007");
    assert_eq!(get("$H", ContestMode::Cw), "TARO");
    assert_eq!(get("$H", ContestMode::Phone), "HANAKO");
    assert_eq!(get("$N", ContestMode::Mix), "10W");
    assert_eq!(get("$O$M", ContestMode::Mix), "TAROJA1ZLO");
    assert_eq!(get("$V$Z", ContestMode::Mix), "1025");
    assert_eq!(get("$Q$P", ContestMode::Mix), "100101");
    assert_eq!(get("$R$C", ContestMode::Mix), "$R$C");
    assert_eq!(get("[AR]$V_", ContestMode::Mix), "10");
    assert_eq!(get("", ContestMode::Mix), "");
}

#[test]
fn cfg_sent_missing_station_values_leave_empty() {
    let st = zlog_core::Station::default();
    for t in ["$Z", "$I", "$A", "$T", "$H", "$V"] {
        let c = from_cfg(&format!("#V\nsendnr {t}\n"), |_| None).build_rules(ContestMode::Cw);
        assert_eq!(sent(&c, Band::B7, &st), "", "{t}");
    }
    let c = from_cfg("#V\nsendnr $V$P\n", |_| None).build_rules(ContestMode::Cw);
    let st = station();
    assert_eq!(c.sent_number(Band::B7, zlog_core::Power::P100, &st), "");
    assert_eq!(c.sent_number(Band::B7, zlog_core::Power::P005, &st), "10");
}

#[test]
fn nyp_sends_handle_by_mode() {
    let st = station();
    assert_eq!(
        sent(&NypContest::new(ContestMode::Cw), Band::B7, &st),
        "TARO"
    );
    assert_eq!(
        sent(&NypContest::new(ContestMode::Rtty), Band::B7, &st),
        "TARO"
    );
    assert_eq!(
        sent(&NypContest::new(ContestMode::Mix), Band::B7, &st),
        "TARO"
    );
    assert_eq!(
        sent(&NypContest::new(ContestMode::Phone), Band::B7, &st),
        "HANAKO"
    );
    let empty = zlog_core::Station::default();
    assert_eq!(
        sent(&NypContest::new(ContestMode::Cw), Band::B7, &empty),
        ""
    );
}

#[test]
fn cfg_coeff_default_on() {
    let mut l = log(vec![qso("JA1AAA", Band::B7, Mode::Cw, "10")]);
    l.set_score_coeff(2.0);
    let score = |cfg: &str, l: &Log| {
        let mut c = from_cfg(cfg, |_| None).build_rules(ContestMode::Mix);
        c.score(l).sheet.with_coeff
    };
    assert!(score("#K\npower HHHHHH\nnomulti on\n", &l).is_some());
    assert!(score("#K\npower HHHHHH\nnomulti on\ncoeff on\n", &l).is_some());
    assert!(score("#K\npower HHHHHH\nnomulti on\ncoeff off\n", &l).is_none());
    assert!(score("#K\npower HHHHHH\nnomulti on\ncoeff off ; contest\n", &l).is_none());
    l.set_score_coeff(0.0);
    assert!(score("#K\npower HHHHHH\nnomulti on\n", &l).is_none());
}

#[test]
fn cfg_warc_bands_never_available() {
    for warc in ["on", "off"] {
        let c = Config::parse(&format!("#W\npower HHHHHH\nwarc {warc}\n"));
        assert!(!c.band_available(Band::B10));
        assert!(!c.band_available(Band::B18));
        assert!(!c.band_available(Band::B24));
        assert!(c.band_available(Band::B7));
    }
    let mut c = from_cfg("#W\npower HHHHHH\nwarc on\n", |_| None).build_rules(ContestMode::Mix);
    assert!(c.uses_warc());
    let mut l = log(vec![qso("JA1AAA", Band::B10, Mode::Cw, "10")]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    assert!(!sheet.bands.contains_key(&Band::B10));
}

#[test]
fn cfg_d_is_the_time_filled_by_the_logger() {
    let st = zlog_core::Station::default();
    let c = from_cfg("#N\nsendnr 10$D\n", |_| None).build_rules(ContestMode::Mix);
    assert!(c.sent_with_time());
    assert_eq!(sent(&c, Band::B7, &st), "10$D");
    let plain = from_cfg("#N\nsendnr 10\n", |_| None).build_rules(ContestMode::Mix);
    assert!(!plain.sent_with_time());
}

fn on_tx(mut q: Qso, tx: u8) -> Qso {
    q.tx = tx;
    q
}

#[test]
fn cfg_serial_start_only_for_an_empty_log() {
    let st = zlog_core::Station::default();
    for kind in ["all", "band"] {
        let cfg = format!("#S\nserial {kind}\nserialstart 100\nsendnr $S\n");
        let mut c = from_cfg(&cfg, |_| None).build_rules(ContestMode::Mix);
        c.score(&Log::default());
        assert_eq!(c.next_serial_on(Band::B7), Some(100), "{kind}");
        assert_eq!(sent(&c, Band::B7, &st), "100", "{kind}");
        c.score(&log(vec![with_serial("JA1AAA", Band::B7, 100)]));
        assert_eq!(c.next_serial_on(Band::B7), Some(101), "{kind}");
        let other = if kind == "all" { 101 } else { 1 };
        assert_eq!(c.next_serial_on(Band::B14), Some(other), "{kind}");
    }
}

#[test]
fn cfg_serial_zero_and_bad_serialstart() {
    let mut c =
        from_cfg("#S\nserial all\nserialstart 50\n", |_| None).build_rules(ContestMode::Mix);
    c.score(&log(vec![with_serial("JA1AAA", Band::B7, 0)]));
    assert_eq!(c.next_serial_on(Band::B7), Some(1));
    let c = from_cfg("#S\nserial all\nserialstart x\n", |_| None).build_rules(ContestMode::Mix);
    assert_eq!(c.next_serial_on(Band::B7), Some(1));
}

#[test]
fn serials_follow_the_own_tx() {
    let l = log(vec![
        on_tx(with_serial("JA1AAA", Band::B7, 1), 0),
        on_tx(with_serial("JA1BBB", Band::B7, 1), 1),
        on_tx(with_serial("JA1CCC", Band::B7, 2), 1),
        on_tx(with_serial("JA1DDD", Band::B14, 2), 0),
    ]);
    let mut st = zlog_core::Station::default();
    let mut all = from_cfg("#A\nserial all\nsendnr $S\n", |_| None).build_rules(ContestMode::Mix);
    all.score(&l);
    assert_eq!(all.next_serial_of_tx(Band::B7, 0), Some(3));
    assert_eq!(all.next_serial_of_tx(Band::B7, 1), Some(3));
    assert_eq!(
        all.next_serial_of_tx(Band::B7, 2),
        Some(1),
        "TX without QSOs"
    );
    assert_eq!(all.next_serial_on(Band::B7), Some(3));
    let mut band = from_cfg("#B\nserial band\nsendnr $S\n", |_| None).build_rules(ContestMode::Mix);
    band.score(&l);
    assert_eq!(band.next_serial_of_tx(Band::B7, 0), Some(2));
    assert_eq!(band.next_serial_of_tx(Band::B7, 1), Some(3));
    assert_eq!(band.next_serial_of_tx(Band::B14, 1), Some(1));
    st.tx_nr = 1;
    assert_eq!(sent(&band, Band::B7, &st), "003");
    st.tx_nr = 0;
    assert_eq!(sent(&band, Band::B7, &st), "002");
    let mut ja0 = Ja0Contest::new(false, ContestMode::Mix);
    ja0.score(&log(vec![
        on_tx(with_serial("JA0AAA", Band::B7, 7), 0),
        on_tx(with_serial("JA0BBB", Band::B7, 3), 1),
    ]));
    assert_eq!(ja0.next_serial_of_tx(Band::B7, 0), Some(8));
    assert_eq!(ja0.next_serial_of_tx(Band::B7, 1), Some(4));
    st.tx_nr = 1;
    assert_eq!(sent(&ja0, Band::B7, &st), "004");
}

#[test]
fn all_mode_other_scores_points_but_no_multiplier() {
    let mut c = local().build_rules(ContestMode::All);
    let mut l = log(vec![
        qso("JA8AAA", Band::B7, Mode::Other, "204H"),
        qso("JA8BBB", Band::B7, Mode::Ft8, "204H"),
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    assert!(!l.qsos[0].new_multi1);
    assert_eq!(l.qsos[0].points, 1);
    assert!(l.qsos[1].new_multi1);
    assert_eq!(sheet.total.qsos, 2);
    assert_eq!(sheet.total.multi, 1);
    let mut ja0 = Ja0Contest::new(false, ContestMode::All);
    let mut l = log(vec![qso("JA0ABC", Band::B7, Mode::Other, "001")]);
    let sheet = rescore(&mut l, &mut ja0).unwrap();
    assert!(!l.qsos[0].new_multi1);
    assert_eq!(l.qsos[0].points, 3);
    assert_eq!(sheet.total.qsos, 1);
}

#[test]
fn count_higher_points_merges_10g_under_single10g() {
    let cfg = |single: &str| {
        format!(
            "#G\npower HHHHHHHHHHHHHHHHHHH\nmode on\ncounthigh on\nexpt10g 1,3\n\
             expt104 1,3\nundefmulti on\nsingle10g {single}\n"
        )
    };
    let qsos = || {
        log(vec![
            qso("JA1AAA", Band::B10G, Mode::Cw, "10"),
            qso("JA1AAA", Band::B10_4G, Mode::Ssb, "10"),
        ])
    };
    let mut c = from_cfg(&cfg("on"), |_| None).build_rules(ContestMode::Mix);
    let mut l = qsos();
    let sheet = rescore(&mut l, &mut c).unwrap();
    let pts: Vec<u8> = l.qsos.iter().map(|q| q.points).collect();
    assert_eq!(pts, vec![0, 3]);
    assert_eq!(sheet.total.points, 3);
    assert!(!l.qsos[1].dupe);
    let mut c = from_cfg(&cfg("off"), |_| None).build_rules(ContestMode::Mix);
    let mut l = qsos();
    rescore(&mut l, &mut c).unwrap();
    let pts: Vec<u8> = l.qsos.iter().map(|q| q.points).collect();
    assert_eq!(pts, vec![1, 3]);
}

#[test]
fn cfg_power_longer_than_the_bands() {
    let c = Config::parse("#P\npower HHHHHHHHHHHHHHHHHHMLP\n");
    assert_eq!(c.power.chars().count(), 19);
    assert_eq!(c.power_char(Band::B248G), 'P');
    assert_eq!(c.power_char(Band::B135G), 'H');
    let c = Config::parse("#P\npower HHHHHHHHHHHHHHHHHHL\n");
    assert_eq!(c.power_char(Band::B248G), 'L');
    let c = Config::parse("#P\npower hm\n");
    assert_eq!(
        (c.power_char(Band::B1_9), c.power_char(Band::B3_5)),
        ('H', 'M')
    );
    assert_eq!(c.power_char(Band::B7), '-');
    let c = Config::parse("#P\npower\n");
    assert!(c.power.chars().all(|ch| ch == '-'));
    assert_eq!(c.power.chars().count(), 19);
}

#[test]
fn cfg_function_keys_need_the_exact_name() {
    let c = Config::parse(
        "#F\nf1_a\tONE\nf8_b\tEIGHT\nf01_a\tBAD\nf9_a\tNINE\nf0_b\tZERO\nf+2_a\tPLUS\n",
    );
    assert_eq!(c.cw_message_a[0], "ONE");
    assert_eq!(c.cw_message_b[7], "EIGHT");
    assert_eq!(c.cw_message_a[1], "", "F+2_A is not F2_A");
    assert!(c.cw_message_a.iter().all(|m| m != "BAD" && m != "NINE"));
    assert!(c.cw_message_b.iter().all(|m| m != "ZERO"));
}

#[test]
fn numeric_compare_follows_str_to_int() {
    use zlog_cfg::user::delphi_str_to_int as int;
    assert_eq!(int("12"), Some(12));
    assert_eq!(int("  12"), Some(12));
    assert_eq!(int("12 "), None);
    assert_eq!(int("-5"), Some(-5));
    assert_eq!(int("+7"), Some(7));
    assert_eq!(int("$1F"), Some(31));
    assert_eq!(int("0x10"), Some(16));
    assert_eq!(int("x10"), Some(16));
    assert_eq!(int(""), None);
    assert_eq!(int("-"), None);
    assert_eq!(int("$"), None);
    assert_eq!(int("1A"), None);
    assert_eq!(int("99999999999"), None);
    let dat = "X\n10 Ten\n-5 Minus\n-1 MinusOne\n31 Hex\nABC Text\n";
    let def = from_cfg("#N\ndat x\nnrnumcompare on\n", |_| Some(dat.into()));
    let mut c = def.build_rules(ContestMode::Mix);
    c.score(&Log::default());
    let name = |nr: &str| match c.check_multi(&qso("JA1AAA", Band::B7, Mode::Cw, nr)) {
        MultiCheck::Known { name, .. } => name,
        MultiCheck::Invalid => String::new(),
    };
    assert_eq!(name("010"), "Ten");
    assert_eq!(name("$A"), "Ten");
    assert_eq!(name("-05"), "Minus");
    assert_eq!(name("$1F"), "Hex");
    assert_eq!(name("-1"), "MinusOne");
    assert_eq!(name("-01"), "", "-1 by value falls back to text");
    assert_eq!(name("ABC"), "Text");
    assert_eq!(name("11"), "");
}

#[test]
fn check_multi_invalid_paths() {
    let dat = "X\n10 Ten\n";
    let c = from_cfg("#U\ndat x\n", |_| Some(dat.into())).build_rules(ContestMode::Mix);
    let at = |nr: &str| c.check_multi(&qso("JA1AAA", Band::B7, Mode::Cw, nr));
    assert_eq!(at(""), MultiCheck::Invalid);
    assert_eq!(at("11"), MultiCheck::Invalid);
    assert!(matches!(at("10"), MultiCheck::Known { needed: true, .. }));
    // UNDEFMULTI: zLog writes "<nr> : Not worked on any band"; reported as Invalid.
    let c =
        from_cfg("#U\ndat x\nundefmulti on\n", |_| Some(dat.into())).build_rules(ContestMode::Mix);
    assert_eq!(
        c.check_multi(&qso("JA1AAA", Band::B7, Mode::Cw, "11")),
        MultiCheck::Invalid
    );
}

#[test]
fn user_multi_table_follows_the_dat_file() {
    let mut c = local().build_rules(ContestMode::Mix);
    c.score(&Log::default());
    let t = c.multi_table();
    assert_eq!(t.len(), 3);
    assert_eq!(t[2].0, "101", "first code of an entry");
    assert!(t.iter().all(|r| r.2.is_empty()));
    let mut l = log(vec![qso("JA8AAA", Band::B21, Mode::Cw, "102H")]);
    rescore(&mut l, &mut c).unwrap();
    assert_eq!(c.multi_table()[2].2, vec![Band::B21]);
    let c = from_cfg("#N\nnomulti on\n", |_| None).build_rules(ContestMode::Mix);
    assert!(c.multi_table().is_empty());
}
