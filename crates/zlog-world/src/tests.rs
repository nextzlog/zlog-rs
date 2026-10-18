//! Scoring of small logs; expected values follow the Delphi code cited per test.

use super::*;
use crate::cty::tests::FIXTURE;
use zlog_core::rescore;

fn rules(kind: WorldContest, mode: ContestMode) -> WorldRules {
    WorldRules::new(kind, mode, Arc::new(Cty::parse(FIXTURE)))
}

fn q(call: &str, band: Band, mode: Mode, nr: &str) -> Qso {
    Qso {
        callsign: call.into(),
        band,
        mode,
        nr_rcvd: nr.into(),
        ..Qso::default()
    }
}

fn log(me: &str, qsos: Vec<Qso>) -> Log {
    let mut l = Log::default();
    l.header.callsign = me.into();
    l.qsos = qsos;
    l
}

fn run(kind: WorldContest, mode: ContestMode, l: &mut Log) -> (WorldRules, ScoreSheet) {
    let mut c = rules(kind, mode);
    let sheet = rescore(l, &mut c).unwrap();
    (c, sheet)
}

fn totals(s: &ScoreSheet) -> (u32, u32, u32, u64) {
    (s.total.qsos, s.total.points, s.total.multi, s.score)
}

#[test]
fn texts_complete() {
    assert!(texts().missing().is_empty());
    assert_eq!(Cqww.caption("en"), "CQ WW");
    assert_eq!(Cqww.group("en"), "World");
    assert!(WorldContest::ALL
        .iter()
        .all(|c| c.uses_utc() == (*c != Pedi)));
}

#[test]
fn cqww() {
    // UWWMulti.pas TWWMulti.AddNoUpdate: JA-JA 0, AS-AS 1, other continent 3;
    // zones and countries per band; UWWScore.pas: points x (zones + countries).
    let mut l = log(
        "JA1ZLO",
        vec![
            q("JA2AAA", Band::B14, Mode::Cw, "25"), // own country: 0, zone 25 + JA
            q("BY1AA", Band::B14, Mode::Cw, "24"),  // AS: 1, zone 24 + BY
            q("W1AW", Band::B14, Mode::Cw, "5"),    // NA: 3, zone 5 + K
            q("W1AW", Band::B14, Mode::Cw, "5"),    // dupe
            q("K6XX", Band::B21, Mode::Cw, "3"),    // 3, zone 3 + K on 21
            q("DL1AA", Band::B14, Mode::Ssb, "14"), // phone in a CW log: no score
        ],
    );
    let (_, s) = run(Cqww, ContestMode::Cw, &mut l);
    assert_eq!(totals(&s), (5, 7, 8, 56));
    assert!(l.qsos[3].dupe);
    assert_eq!(l.qsos[2].multi2, "K");
}

#[test]
fn cqww_rtty_states() {
    // UWWMulti.pas AddNoUpdate (RTTY points) and UARRLDXMulti.pas AddMulti3NoUpdate.
    let mut l = log(
        "JA1ZLO",
        vec![
            q("W1AW", Band::B14, Mode::Rtty, "5CT"),
            q("K1AR", Band::B14, Mode::Rtty, "5MA"),
            q("JA2AAA", Band::B14, Mode::Rtty, "25"),
        ],
    );
    let (c, s) = run(Cqww, ContestMode::Rtty, &mut l);
    // 3 + 3 + 1 points; zones 5, 25; K, JA; states CT, MA.
    assert_eq!(totals(&s), (3, 7, 6, 42));
    assert_eq!(l.qsos[1].multi3, "MA");
    assert!(c.valid_number(&q("W1AW", Band::B14, Mode::Rtty, "5CT")));
    assert!(!c.valid_number(&q("W1AW", Band::B14, Mode::Rtty, "5")));
    assert!(c.valid_number(&q("DL1AA", Band::B14, Mode::Rtty, "14")));
}

#[test]
fn wpx() {
    // UWPXMulti.pas AddNoUpdate: other continent 6 on 40 m, 3 on 20 m; same
    // continent other country 2/1; own country 1. Prefixes once per contest.
    let mut l = log(
        "JA1ZLO",
        vec![
            q("W1AW", Band::B7, Mode::Cw, "001"),
            q("W1AW", Band::B14, Mode::Cw, "002"),
            q("BY1AA", Band::B7, Mode::Cw, "003"),
            q("JA2AAA", Band::B14, Mode::Cw, "004"),
            q("JA1BBB", Band::B21, Mode::Cw, "005"),
        ],
    );
    l.qsos[4].nr_sent = "005".into();
    let (c, s) = run(Wpx, ContestMode::Cw, &mut l);
    // 6 + 3 + 2 + 1 + 1 = 13; prefixes W1, BY1, JA2, JA1.
    assert_eq!(totals(&s), (5, 13, 4, 52));
    assert!(!l.qsos[1].new_multi1);
    assert_eq!(c.next_serial(), 6);
    let st = Station::default();
    assert_eq!(c.sent_number(Band::B7, Power::P100, &st), "006");
    assert_eq!(c.exchange_from(Band::B7, &l.qsos[0]), None);
}

#[test]
fn iaru() {
    // UIARUMulti.pas AddNoUpdate: own zone or HQ 1, same continent 3, else 5.
    let mut l = log(
        "JA1ZLO",
        vec![
            q("JA2AAA", Band::B14, Mode::Cw, "45"),
            q("JA2AAA", Band::B14, Mode::Ssb, "45"), // other mode: not a dupe
            q("BY1AA", Band::B14, Mode::Cw, "44"),
            q("W1AW", Band::B14, Mode::Cw, "ARRL"),
            q("DL1AA", Band::B21, Mode::Cw, "28"),
        ],
    );
    let (_, s) = run(Iaru, ContestMode::Mix, &mut l);
    // 1 + 1 + 3 + 1 + 5 = 11; 45, 44, ARRL on 14, 28 on 21.
    assert_eq!(totals(&s), (5, 11, 4, 44));
    assert!(!l.qsos[1].dupe);
}

#[test]
fn arrl_dx() {
    // UARRLDXMulti.pas (states per band), UARRLDXScore.pas (3 points).
    let mut l = log(
        "JA1ZLO",
        vec![
            q("W1AW", Band::B14, Mode::Cw, "CT"),
            q("K1AR", Band::B14, Mode::Cw, "MA"),
            q("K1XX", Band::B14, Mode::Cw, "CT"),
            q("W1AW", Band::B21, Mode::Cw, "CT"),
        ],
    );
    let (c, s) = run(ArrlDxDx, ContestMode::Cw, &mut l);
    assert_eq!(totals(&s), (4, 12, 3, 36));
    assert!(!c.valid_number(&q("W1AW", Band::B14, Mode::Cw, "XX")));
    // UARRLWMulti.pas (countries per band) for the W/VE side.
    let mut l = log(
        "W1AW",
        vec![
            q("JA1ZLO", Band::B14, Mode::Cw, "100"),
            q("JA2AAA", Band::B14, Mode::Cw, "KW"),
            q("IT9AAA", Band::B14, Mode::Cw, "100"), // no WAE list: Italy
            q("KH6AA", Band::B14, Mode::Cw, "100"),  // Hawaii is DX
            q("VE3AA", Band::B14, Mode::Cw, "ON"),   // rules 5.2.2: no multiplier
        ],
    );
    let (_, s) = run(ArrlDxW, ContestMode::Cw, &mut l);
    assert_eq!(totals(&s), (5, 15, 3, 45));
    assert_eq!(l.qsos[2].multi1, "I");
}

#[test]
fn waedc() {
    // UWAEMulti.pas (Europe only), UWAEScore.pas: (QSOs + QTCs) x factor x multis.
    let mut tagged = q("DL1AA", Band::B7, Mode::Cw, "001");
    tagged.memo = "[QTC 10]".into();
    let mut l = log(
        "JA1ZLO",
        vec![
            q("DL1AA", Band::B3_5, Mode::Cw, "001"), // 80 m factor 4
            tagged,                                  // 40 m factor 3, one QTC
            q("DJ1BB", Band::B7, Mode::Cw, "002"),
            q("W1AW", Band::B7, Mode::Cw, "003"),   // not Europe
            q("IT9AA", Band::B14, Mode::Cw, "004"), // *IT9, factor 2
        ],
    );
    let (_, s) = run(Wae, ContestMode::Cw, &mut l);
    // points 4, QTCs 1; multis 4 + 3 + 2 = 9.
    assert_eq!(totals(&s), (5, 4, 9, 45));
    assert_eq!(l.qsos[3].multi1, "Non-EU");
}

#[test]
fn iota() {
    // UIOTAMulti.pas AddNoUpdate: island 15, no island or own island 3; islands
    // by band and mode.
    let mut first = q("JA2AAA", Band::B14, Mode::Cw, "001AS007");
    first.nr_sent = "001AS007".into();
    let mut l = log(
        "JA1ZLO",
        vec![
            first,
            q("DL1AA", Band::B14, Mode::Cw, "005"),
            q("G4AAA", Band::B14, Mode::Cw, "010EU005"),
            q("G4AAA", Band::B14, Mode::Ssb, "011EU005"),
        ],
    );
    let (c, s) = run(Iota, ContestMode::Mix, &mut l);
    // IOTA rules 2023/2024, island side: own reference 5, world 5, island 15:
    // 5 + 5 + 15 + 15; AS007, EU005 CW, EU005 SSB.
    assert_eq!(totals(&s), (4, 40, 3, 120));
    assert!(matches!(
        c.check_multi(&q("X", Band::B21, Mode::Cw, "EU005")),
        MultiCheck::Known { needed: true, .. }
    ));
}

#[test]
fn arrl10() {
    // UARRL10Multi.pas (mode slots), UARRL10Score.pas (CW 4, /N 8, phone 2).
    let mut l = log(
        "JA1ZLO",
        vec![
            q("W1AW", Band::B28, Mode::Cw, "CT"),
            q("W1AW", Band::B28, Mode::Ssb, "CT"),
            q("K1AA/N", Band::B28, Mode::Cw, "MA"),
            q("DL1AA", Band::B28, Mode::Cw, "123"),
            q("XE1AA", Band::B28, Mode::Ssb, "TLAX"),
        ],
    );
    let (c, s) = run(Arrl10, ContestMode::Mix, &mut l);
    // ARRL 10 m rules 2026 v2.0 5.1: no novice bonus (zLog gives /N 8 points):
    // 4 + 2 + 4 + 4 + 2 = 16; CT cw, CT ph, MA cw, DL cw, TLX ph.
    assert_eq!(totals(&s), (5, 16, 5, 80));
    assert!(c.valid_number(&q("X/MM", Band::B28, Mode::Cw, "2")));
    assert!(!c.valid_number(&q("DL1AA", Band::B28, Mode::Cw, "CT")));
    let me = Station {
        callsign: "W1AW".into(),
        prov: "CT".into(),
        ..Station::default()
    };
    assert_eq!(c.sent_number(Band::B28, Power::P100, &me), "CT");
}

#[test]
fn all_asian() {
    // UARRLWMulti.pas AddNoUpdate with ALLASIANFLAG.
    let mut l = log(
        "JA1ZLO",
        vec![
            q("JA2AAA", Band::B14, Mode::Cw, "50"),
            q("BY1AA", Band::B1_9, Mode::Cw, "40"),
            q("BY1AA", Band::B28, Mode::Cw, "40"),
            q("DL1AA", Band::B7, Mode::Cw, "30"),
            q("W1AW", Band::B3_5, Mode::Cw, "30"),
        ],
    );
    let (_, s) = run(AllAsian, ContestMode::Cw, &mut l);
    // 0 + 3 + 2 + 3 + 6 = 14; JA, BY, BY, DL, K.
    assert_eq!(totals(&s), (5, 14, 5, 70));
}

#[test]
fn jidx() {
    // UJIDXMulti.pas (zones, countries), UJIDXScore2.pas (points by band).
    let mut l = log(
        "JA1ZLO",
        vec![
            q("W1AW", Band::B1_9, Mode::Cw, "5"),
            q("K6XX", Band::B1_9, Mode::Cw, "3"),
            q("DL1AA", Band::B7, Mode::Cw, "14"),
            q("JA2AAA", Band::B7, Mode::Cw, "25"), // JIDX rules 2026: JA-JA scores nothing
        ],
    );
    let (_, s) = run(Jidx, ContestMode::Cw, &mut l);
    // 4 + 4 + 1 = 9; zones 5, 3, 14; K, DL.
    assert_eq!(totals(&s), (4, 9, 5, 45));
    // UJIDX_DX_Multi.pas: prefectures 1 to 50 per band.
    let mut l = log(
        "W1AW",
        vec![
            q("JA1ZLO", Band::B28, Mode::Cw, "10"),
            q("JA1BBB", Band::B28, Mode::Cw, "10"),
            q("JD1AA", Band::B14, Mode::Cw, "48"),
        ],
    );
    let (c, s) = run(JidxDx, ContestMode::Cw, &mut l);
    assert_eq!(totals(&s), (3, 5, 2, 10));
    assert!(!c.valid_number(&q("JA1ZLO", Band::B14, Mode::Cw, "51")));
    assert_eq!(c.multi_table()[47].1, "Ogasawara");
}

#[test]
fn ap_sprint() {
    // UAPSprintScore.pas: 1 point; UWPXMulti.pas prefixes once per contest.
    let mut l = log(
        "JA1ZLO",
        vec![
            q("JA2AAA", Band::B7, Mode::Cw, "001"),
            q("JA2AAA", Band::B14, Mode::Cw, "002"),
            q("VK2AA", Band::B21, Mode::Cw, "003"),
        ],
    );
    let (_, s) = run(ApSprint, ContestMode::Cw, &mut l);
    // AP Sprint rules 2026: 20 and 15 m only, so the 40 m QSO is out of the contest.
    assert_eq!(totals(&s), (2, 2, 2, 4));
}

#[test]
fn bartg() {
    // UJarlWorldWideRTTYMulti.pas AddNoUpdate (BARTG rule): countries and call
    // areas per band, continents once; 1 point, /MM 2 points.
    let mut l = log(
        "JA1ZLO",
        vec![
            q("W1AW", Band::B14, Mode::Rtty, "001"),  // K, K1, NA
            q("K1AR", Band::B14, Mode::Rtty, "002"),  // nothing new
            q("W6AA", Band::B14, Mode::Rtty, "003"),  // K6
            q("DL1AA", Band::B21, Mode::Rtty, "004"), // DL, EU
            q("DL1AA/MM", Band::B7, Mode::Rtty, "005"),
        ],
    );
    let (_, s) = run(Bartg, ContestMode::Rtty, &mut l);
    // BARTG rules 2025 v3: 1 point each (no /MM bonus); score = points x
    // (K, K1, K6, DL) x continents (NA, EU) = 5 x 4 x 2.
    assert_eq!(totals(&s), (5, 5, 4, 40));
    assert_eq!(l.qsos[0].multi1, "K,K1");
}

#[test]
fn pedi() {
    // UPediScore.pas: one point per QSO, every mode, no multipliers.
    let mut l = log(
        "JA1ZLO",
        vec![
            q("W1AW", Band::B10, Mode::Cw, ""),
            q("W1AW", Band::B10, Mode::Ft8, ""),
            q("W1AW", Band::B144, Mode::Fm, ""),
        ],
    );
    let (_, s) = run(Pedi, ContestMode::All, &mut l);
    assert_eq!(totals(&s), (3, 3, 0, 3));
}

#[test]
fn without_cty_everything_is_unknown() {
    // GetPrefix returns the unknown country; AnalyzeMyCountry keeps JA/AS.
    let mut c = WorldRules::new(Cqww, ContestMode::Cw, Arc::new(Cty::default()));
    let mut l = log("JA1ZLO", vec![q("W1AW", Band::B14, Mode::Cw, "5")]);
    let s = rescore(&mut l, &mut c).unwrap();
    assert_eq!(totals(&s), (1, 3, 1, 3));
}
