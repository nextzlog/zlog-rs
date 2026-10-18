//! Scoring checks against zLog's Delphi code; expected values are derived by hand
//! from the files and lines cited.

use zlog_core::{rescore, Band, Contest, ContestDef, ContestMode, Log, Mode, MultiCheck, Qso};
use zlog_japan::cfg::{Config, PrefixMulti, SerialType};
use zlog_japan::ja0::Ja0Contest;
use zlog_japan::nyp::NypContest;
use zlog_japan::{from_cfg, JapanContest};

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

/// A local-contest CFG in the style of the zLog_cfgdat files (tabs, comments).
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

fn local() -> zlog_japan::UserDefinedDef {
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
    // POWER --H-HHHMM: 7 MHz to 430 MHz, 14 MHz has '-' (UserDefinedContest.pas:1202-1226).
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
    // PT: SSB digit then CW digit (UserDefinedContest.pas:473-488).
    assert_eq!((at(Band::B7, Mode::Ssb), at(Band::B7, Mode::Cw)), (2, 3));
    assert_eq!((at(Band::B21, Mode::Ssb), at(Band::B21, Mode::Cw)), (5, 10));
    assert_eq!(at(Band::B50, Mode::Dv), 8);
    assert_eq!(at(Band::B50, Mode::Other), 9);
    // DEFAULTPT: CW, phone per band without WARC (SetPointsTable).
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
        qso("JA8AAA", Band::B7, Mode::Ssb, "204H"), // other mode: not a dupe (MODE ON)
        qso("JA8AAA", Band::B7, Mode::Cw, "204H"),  // dupe
        qso("JA8BBB", Band::B21, Mode::Fm, "102M"), // second code of an entry
        qso("JA8CCC", Band::B21, Mode::Cw, "555M"), // not on the list
        qso("JA8DDD", Band::B1200, Mode::Fm, "214L"), // out of the band range
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    // CUT 1 drops the power letter (UGeneralMulti2.pas:262-266); default points 1.
    assert_eq!(l.qsos[0].multi1, "204");
    assert!(l.qsos[0].new_multi1 && !l.qsos[1].new_multi1);
    assert!(l.qsos[2].dupe && l.qsos[2].points == 0);
    assert!(l.qsos[3].new_multi1);
    assert!(!l.qsos[4].new_multi1 && l.qsos[4].points == 1);
    assert_eq!(l.qsos[5].points, 0);
    let t = sheet.total;
    // A dupe still counts as a QSO (TBasicScore.AddNoUpdate, UBasicScore.pas:134).
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
        qso("JA1AAA", Band::B1_9, Mode::Cw, "01H"), // local: LOCALPT cw 3
        qso("JA1BBB", Band::B1_9, Mode::Ssb, "02H"), // default 1
        qso("JA1ZZZ", Band::B1_9, Mode::Fm, "02M"), // special call 10
        qso("JA1CCC", Band::B3_5, Mode::Cw, "1H"),  // JARL: only digit first -> ""
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    let pts: Vec<u8> = l.qsos.iter().map(|q| q.points).collect();
    assert_eq!(pts, vec![3, 1, 10, 1]);
    // UNDEFMULTI adds 01 and 02 on 1.9 MHz and "" on 3.5 MHz (UGeneralMulti2.pas:393).
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
    // NOMULTI: score is the points (UGeneralScore.pas:204).
    assert_eq!((sheet.total.points, sheet.score), (8, 8));
}

#[test]
fn count_higher_points() {
    let cfg = "#C\npower HHHHHH\nmode on\ncounthigh on\nexpt7 1,3\nundefmulti on\n";
    let mut c = from_cfg(cfg, |_| None).build_rules(ContestMode::Mix);
    let mut l = log(vec![
        qso("JA1AAA", Band::B7, Mode::Cw, "10"),
        qso("JA1AAA", Band::B7, Mode::Ssb, "10"),
        qso("JA1AAA", Band::B7, Mode::Fm, "10"), // dupe of SSB in the phone group
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    // The SSB QSO is worth more, so the CW QSO loses its point (UGeneralScore.pas:310-321).
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
    assert_eq!(c.sent_number(Band::B7, zlog_core::Power::P010, &st), "");
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
    // GetPX: suffix after a slash past the 4th character is cut; a short prefix is dropped.
    assert_eq!(l.qsos[0].multi1, "JA1");
    assert_eq!(l.qsos[1].multi1, "JA1");
    assert!(l.qsos[0].new_multi1 && !l.qsos[1].new_multi1);
    use zlog_japan::user::wpx_prefix;
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
        qso("JR0XYC", Band::B7, Mode::Cw, "002"), // same multiplier A*C? no: R*C
        qso("JA1AAA", Band::B7, Mode::Cw, "003"), // no 0: 1 point, no multiplier
        qso("JA0ABC", Band::B7, Mode::Cw, "004"), // dupe
        qso("JA0ABC", Band::B14, Mode::Cw, "005"), // 14 MHz: no points, not in totals
        qso("JA0ABC", Band::B21, Mode::Ssb, "006"),
    ]);
    let sheet = rescore(&mut l, &mut c).unwrap();
    // UJA0Multi.pas:117-143, UJA0Score.pas:76-101.
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
    assert_eq!(c.sent_number_with_serial(12), "012");
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
    // TPediScore.AddNoUpdate: 1 point and a count for every QSO, the dupe included.
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

/// Real CFG/DAT pairs from github.com/jr8ppg/zLog_cfgdat (not redistributed: no
/// licence). Run with `ZLOG_CFGDAT_DIR=<clone>/plugins/rules` after converting the
/// files to UTF-8 is not needed: the test decodes only ASCII-relevant parts.
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

mod zylo {
    use super::*;
    use zlog_core::DelphiTime;
    use zlog_japan::zylo::ZyloContest;

    fn run(c: ZyloContest, qsos: Vec<Qso>) -> (Log, zlog_core::ScoreSheet) {
        let def = c.def();
        assert!(def.dat_loaded());
        assert_eq!(def.group("ja"), "ローカル");
        let mut contest = def.build_rules(ContestMode::Mix);
        let mut l = log(qsos);
        let sheet = rescore(&mut l, &mut contest).unwrap();
        (l, sheet)
    }

    #[test]
    fn asahikawa() {
        // aktest.go:35-46: member 9, city 0102 9, Hokkaido 6, else 3.
        let (l, sheet) = run(
            ZyloContest::Asahikawa,
            vec![
                qso("JA8AAA", Band::B7, Mode::Cw, "0102"),
                qso("JA8BBB", Band::B7, Mode::Cw, "0103"),
                qso("JA1CCC", Band::B7, Mode::Cw, "100101M"),
                qso("JA1DDD", Band::B7, Mode::Cw, "100102"),
                qso("JA1EEE", Band::B1_9, Mode::Cw, "100103"), // not 3.5-430
            ],
        );
        let pts: Vec<u8> = l.qsos.iter().map(|q| q.points).collect();
        assert_eq!(pts, vec![9, 6, 9, 3, 0]);
        assert_eq!(l.qsos[2].multi1, "100101");
        assert_eq!(sheet.score, 27 * 4);
    }

    #[test]
    fn high_school() {
        // hstest.go:66-83: CW 3, phone 1; HS adds a second multiplier on each QSO
        // (UGeneralMulti2.pas:370) and one per distinct HS station.
        let (l, sheet) = run(
            ZyloContest::HighSchool,
            vec![
                qso("JA1AAA", Band::B7, Mode::Cw, "10HS"),
                qso("JA1AAA", Band::B21, Mode::Ssb, "10HS"),
                qso("JA1BBB", Band::B7, Mode::Cw, "11C"),
            ],
        );
        let pts: Vec<u8> = l.qsos.iter().map(|q| q.points).collect();
        assert_eq!(pts, vec![3, 1, 3]);
        assert!(l.qsos[0].new_multi2 && !l.qsos[2].new_multi2);
        // multipliers: 10/7, 10/21, 11/7 = 3, HS twice = 2, one HS station = 1.
        assert_eq!(sheet.total.multi, 5);
        assert_eq!(sheet.score, 7 * (5 + 1));
    }

    #[test]
    fn tokai_marathon() {
        // tmtest.go:51-87: band points, last letter, times days.
        let mut a = qso("JA2AAA", Band::B1200, Mode::Fm, "001");
        a.time = DelphiTime::from_ymd_hms(2026, 1, 2, 10, 0, 0);
        let mut b = qso("JA2BBB/2", Band::B5600, Mode::Fm, "002");
        b.time = DelphiTime::from_ymd_hms(2026, 1, 3, 10, 0, 0);
        let (l, sheet) = run(ZyloContest::TokaiMarathon, vec![a, b]);
        assert_eq!(l.qsos[1].multi1, "B");
        assert_eq!(sheet.total.points, 12);
        assert_eq!(sheet.score, 12 * 2 * 2);
    }

    #[test]
    fn yl() {
        // yltest.go:42-88: the sent number itself selects the table, so a sent
        // code like 3000 scores as an OM: 5 from a JL, 1 from a YL, 0 from an OM.
        let q = |call: &str, nr: &str| {
            let mut x = qso(call, Band::B7, Mode::Cw, nr);
            x.nr_sent = "3000".into();
            x
        };
        let (l, sheet) = run(
            ZyloContest::Yl,
            vec![
                q("JA1AAA", "6000"),
                q("JA2BBB", "3000"),
                q("JA3CCC", "1000"),
            ],
        );
        let pts: Vec<u8> = l.qsos.iter().map(|q| q.points).collect();
        assert_eq!(pts, vec![5, 1, 0]);
        assert_eq!(l.qsos[0].multi1, "JA1");
        // JA1, JA2, JA3 listed + three second multipliers.
        assert_eq!(sheet.total.multi, 6);
        assert_eq!(sheet.score, 6 * 6);
    }
}
