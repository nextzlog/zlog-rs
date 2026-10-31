use std::sync::Arc;

use zlog_cfg::UserDefinedDef;
use zlog_core::{rescore, Band, Contest, ContestMode, Log, Mode, Qso};
use zlog_rule_dx::cty::Cty;

const CTY: &str = "\
Japan:                    25:  45:  AS:   36.40:  -138.38:    -9.0:  JA:
    JA,JE,JR,7K;
United States:            05:  08:  NA:   37.53:    91.67:     5.0:  K:
    K,N,W,AA;
Canada:                   05:  09:  NA:   44.35:    78.75:     5.0:  VE:
    VE,VA;
Fed. Rep. of Germany:     14:  28:  EU:   51.00:   -10.00:    -1.0:  DL:
    DL,DJ;
Republic of Korea:        25:  44:  AS:   37.53:  -127.00:    -9.0:  HL:
    HL,DS;
ITU HQ:                   14:  28:  EU:   46.17:    -6.05:    -1.0:  *4U1I:
    =4U1ITU;
Switzerland:              14:  28:  EU:   46.87:    -8.12:    -1.0:  HB:
    HB,4U1I;
";

const NORTH160: &str = "\
#Sample North 160m Contest for K/VE stations
time\tutc;
power\tH------------;\t\t\t1.9 MHz only
dat\tnorthsec;\tsection table
cty\t;\t\tcountries from cty.dat
noctymulti\tk,ve;\tsections for k/ve
localcty\tk,ve;\tlocal points for k/ve
defaultpt\t5,0,0,0,0,0,0,0,0,0,0,0; others 5 pts
localpt\t\t2,0,0,0,0,0,0,0,0,0,0,0; k/ve 2 pts
cut\t0;
sendnr\tQTH;
";

const NORTHSEC: &str = "\
northsec.dat  sample sections
NX,NQ,NQX        North X(K)
VQS                         Valley Q South(VE)
";

fn q(call: &str, band: Band, mode: Mode, nr: &str) -> Qso {
    Qso {
        callsign: call.into(),
        band,
        mode,
        nr_rcvd: nr.into(),
        ..Qso::default()
    }
}

fn contest(cfg: &str, dat: &'static str, mode: ContestMode) -> Box<dyn Contest> {
    let def =
        UserDefinedDef::from_cfg(cfg, |_| Some(dat.into())).with_cty(Arc::new(Cty::parse(CTY)));
    Box::new(def.build_rules(mode))
}

fn log(me: &str, qsos: Vec<Qso>) -> Log {
    let mut l = Log {
        qsos,
        ..Log::default()
    };
    l.header.callsign = me.into();
    l
}

#[test]
fn sections_for_k_ve_and_countries_for_others() {
    let mut c = contest(NORTH160, NORTHSEC, ContestMode::Mix);
    let b = Band::B1_9;
    let mut l = log(
        "W1AW",
        vec![
            q("K1AAA", b, Mode::Cw, "NX"),
            q("VE3XX", b, Mode::Cw, "VQS"),
            q("DL1ABC", b, Mode::Cw, "599"),
            q("JA1XYZ", b, Mode::Cw, "599"),
            q("K2BBB", b, Mode::Cw, "NQ"),
            q("DJ2CC", b, Mode::Cw, "599"),
        ],
    );
    let s = rescore(&mut l, &mut *c).unwrap();
    assert_eq!(s.total.points, 2 + 2 + 5 + 5 + 2 + 5);
    assert_eq!(s.total.multi, 4);
    let multis: Vec<_> = l.qsos.iter().map(|q| q.multi1.as_str()).collect();
    assert_eq!(multis, ["NX", "VQS", "DL", "JA", "NQ", "DL"]);
    let new: Vec<_> = l.qsos.iter().map(|q| q.new_multi1).collect();
    assert_eq!(new, [true, true, true, true, false, false]);
    assert!(c.valid_number(&l.qsos[2]));
    let table = c.multi_table();
    assert!(table.iter().any(|(code, _, w)| code == "DL" && w == &[b]));
}

#[test]
fn countries_count_per_band_or_once() {
    let base = "#T\nCTY ;\nPOWER 1111111111\n";
    let qsos = || {
        vec![
            q("K1AAA", Band::B3_5, Mode::Cw, "599"),
            q("K1AAA", Band::B7, Mode::Cw, "599"),
            q("ZZ9ZZ", Band::B7, Mode::Cw, "599"),
            q("DL1ABC", Band::B7, Mode::Ssb, "59"),
        ]
    };
    let mut l = log("JA1ZLO", qsos());
    let s = rescore(&mut l, &mut *contest(base, "", ContestMode::Mix)).unwrap();
    assert_eq!(s.total.multi, 3);
    assert_eq!(l.qsos[2].multi1, "599");
    assert!(!l.qsos[2].new_multi1);
    let once = format!("{base}COUNTMULTIONCE ON\n");
    let mut l = log("JA1ZLO", qsos());
    let s = rescore(&mut l, &mut *contest(&once, "", ContestMode::Mix)).unwrap();
    assert_eq!(s.total.multi, 2);
    let mut l = log("JA1ZLO", qsos());
    let s = rescore(&mut l, &mut *contest(base, "", ContestMode::Cw)).unwrap();
    assert_eq!(s.total.multi, 2);
    assert_eq!(l.qsos[3].multi1, "DL");
}

#[test]
fn dupes_and_waedc_entities() {
    let base = "#T\nCTY ;\nPOWER 1111111111\n";
    let qsos = || {
        vec![
            q("4U1ITU", Band::B7, Mode::Cw, "599"),
            q("HB9AA", Band::B7, Mode::Cw, "599"),
            q("HB9AA", Band::B7, Mode::Cw, "599"),
        ]
    };
    let mut l = log("JA1ZLO", qsos());
    let s = rescore(&mut l, &mut *contest(base, "", ContestMode::Mix)).unwrap();
    assert_eq!(s.total.multi, 2);
    assert_eq!(l.qsos[0].multi1, "*4U1I");
    assert!(l.qsos[2].dupe);
    assert_eq!(l.qsos[2].multi1, "");
    let only = format!("{base}CTYENTITYONLY ON\n");
    let mut l = log("JA1ZLO", qsos());
    let s = rescore(&mut l, &mut *contest(&only, "", ContestMode::Mix)).unwrap();
    assert_eq!(s.total.multi, 1);
    assert_eq!(l.qsos[0].multi1, "");
}

#[test]
fn same_country_and_continent_points_then_local() {
    let cfg = "#T\nCTY ;\nPOWER 1111111111\nDEFAULTPT 3\nSAMECTYPT 1\nSAMECONTPT 2\n";
    let qsos = || {
        vec![
            q("JA2AAA", Band::B1_9, Mode::Cw, "599"),
            q("HL1AA", Band::B1_9, Mode::Cw, "599"),
            q("DL1ABC", Band::B1_9, Mode::Cw, "599"),
        ]
    };
    let points = |cfg: &str, me: &str| {
        let mut l = log(me, qsos());
        rescore(&mut l, &mut *contest(cfg, "", ContestMode::Mix)).unwrap();
        l.qsos.iter().map(|q| q.points).collect::<Vec<_>>()
    };
    assert_eq!(points(cfg, "JA1ZLO"), [1, 2, 3]);
    assert_eq!(points(cfg, ""), [1, 2, 3]);
    assert_eq!(points(cfg, "DL1ZLO"), [3, 3, 1]);
    let cont = format!("{cfg}LOCALCONT eu\nLOCALPT 7\n");
    assert_eq!(points(&cont, "JA1ZLO"), [1, 2, 7]);
    let cty = format!("{cfg}LOCALCTY hl,dl\nLOCALPT 7\n");
    assert_eq!(points(&cty, "JA1ZLO"), [1, 7, 7]);
    let off = "#T\nPOWER 1111111111\nDEFAULTPT 3\nSAMECTYPT 1\nLOCALCTY JA\nLOCALPT 7\n";
    assert_eq!(points(off, "JA1ZLO"), [3, 3, 3]);
}

#[test]
fn no_country_multi_star_falls_back_to_exchange() {
    let cfg = "#T\nCTY ;\nNOCTYMULTI *\nUNDEFMULTI ON\nPOWER 1111111111\nLOCALCTY JA\nLCUT 1\n";
    let mut l = log(
        "JA1ZLO",
        vec![
            q("JA1AAA", Band::B7, Mode::Cw, "10M"),
            q("DL1ABC", Band::B7, Mode::Cw, "14"),
            q("K1AAA", Band::B7, Mode::Cw, "10"),
        ],
    );
    let s = rescore(&mut l, &mut *contest(cfg, "", ContestMode::Mix)).unwrap();
    let multis: Vec<_> = l.qsos.iter().map(|q| q.multi1.as_str()).collect();
    assert_eq!(multis, ["10", "14", "10"]);
    assert_eq!(s.total.multi, 2);
    assert!(!c_has_countries(cfg));
}

fn c_has_countries(cfg: &str) -> bool {
    contest(cfg, "", ContestMode::Mix)
        .multi_table()
        .iter()
        .any(|(code, _, _)| code == "DL")
}
