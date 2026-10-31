use std::collections::BTreeMap;

use rxsl::formats::jarl::Jarl;
use rxsl::sheets::jarl::JarlSheet;
use rxsl::sheets::summary::{self, Missing, Rules, Summary, Version};
use rxsl::{Item, SheetFormat};
use zlog_core::{Band, BandScore, Log, Mode, Qso, ScoreSheet};

use crate::float_to_str;
use crate::without_out_of_period;

pub const TABLE_TYPE: &str = "ZLOG_REIWA";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    #[default]
    AsEntered,
    Time,
    BandThenTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub summary: Summary,
    pub rules: Rules,
    pub license_date: Option<(i32, u32, u32)>,
    pub age: String,
    pub operators: String,
    pub power_measured: bool,
    pub order: Order,
    pub field_extend: bool,
}

impl Default for Form {
    fn default() -> Self {
        Form {
            summary: Summary {
                version: Version::R1,
                ..Summary::default()
            },
            rules: Rules::default(),
            license_date: None,
            age: String::new(),
            operators: String::new(),
            power_measured: false,
            order: Order::AsEntered,
            field_extend: false,
        }
    }
}

impl Form {
    pub fn fill_defaults(&mut self, contest: &str, callsign: &str, operators: &[&str]) {
        let s = &mut self.summary;
        if s.contest.is_empty() {
            s.contest = contest.into();
        }
        if s.callsign.is_empty() {
            s.callsign = callsign.into();
        }
        if self.operators.is_empty() {
            let mut list: Vec<&str> = Vec::new();
            for op in operators.iter().map(|o| o.trim()) {
                if !op.is_empty() && !list.contains(&op) {
                    list.push(op);
                }
            }
            self.operators = list.join("\n");
        }
    }

    pub fn is_newcomer(&self) -> bool {
        self.rules.is_newcomer(&self.summary.category_code)
    }

    fn categorized(&self) -> Summary {
        let mut s = self.summary.clone();
        s.set_category(&self.rules, self.license_date, &self.age);
        s
    }

    pub fn missing(&self) -> Vec<&'static str> {
        let s = self.categorized();
        s.missing(&self.rules)
            .into_iter()
            .map(|m| match m {
                Missing::Signature => "missing.signature",
                Missing::LicenseDate => "missing.license_date",
                Missing::Age => "missing.age",
            })
            .collect()
    }

    pub fn sheet(&self, score: &Score) -> Summary {
        let mut s = self.categorized();
        s.scores = score
            .bands
            .iter()
            .map(|(b, r)| summary::Score {
                band: score_band(*b),
                qsos: r.qsos.into(),
                points: r.points.into(),
                mults: r.multi.into(),
            })
            .collect();
        s.coeff = score.coeff.map(float_to_str);
        s.total_score = float_to_str(score.total_score);
        s.power_type = summary::power_type(self.power_measured).into();
        s.operators.clear();
        if !self.operators.is_empty() {
            s.operators = self.operators.lines().map(String::from).collect();
        }
        s
    }
}

fn score_band(b: Band) -> String {
    match b >= Band::B10G {
        true => format!("{}Hz", b.label()),
        false => format!("{}MHz", b.label()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub bands: Vec<(Band, BandScore)>,
    pub total: BandScore,
    pub coeff: Option<f64>,
    pub total_score: f64,
}

impl Score {
    pub fn new(sheet: &ScoreSheet, log: &Log, range: (Band, Band)) -> Self {
        let used = |b: Band| {
            !b.is_warc()
                && b >= range.0
                && b <= range.1
                && b <= Band::HIGHEST
                && sheet.bands.get(&b).is_some_and(|s| s.points > 0)
        };
        let mut rows: BTreeMap<Band, BandScore> = BTreeMap::new();
        for b in sheet.bands.keys().filter(|b| used(**b)) {
            rows.insert(*b, BandScore::default());
        }
        for q in log.qsos.iter().filter(|q| !q.invalid) {
            let Some(s) = rows.get_mut(&q.band) else {
                continue;
            };
            s.qsos += 1;
            if q.mode == Mode::Cw {
                s.cw_qsos += 1;
            }
            s.points += q.points as u32;
            s.multi += q.new_multi1 as u32 + q.new_multi2 as u32;
        }
        let bands: Vec<(Band, BandScore)> = rows.into_iter().collect();
        let mut total = BandScore::default();
        for (_, s) in &bands {
            total.qsos += s.qsos;
            total.cw_qsos += s.cw_qsos;
            total.points += s.points;
            total.multi += s.multi;
        }
        let coeff = sheet.with_coeff.map(|(c, _)| c);
        let total_score = total.multi as f64 * total.points as f64 * coeff.unwrap_or(1.0);
        Score {
            bands,
            total,
            coeff,
            total_score,
        }
    }
}

pub fn ordered(qsos: &[Qso], order: Order) -> Vec<&Qso> {
    let mut v: Vec<&Qso> = qsos.iter().collect();
    match order {
        Order::AsEntered => {}
        Order::Time => v.sort_by(|a, b| a.time.0.total_cmp(&b.time.0)),
        Order::BandThenTime => {
            v.sort_by(|a, b| a.band.cmp(&b.band).then(a.time.0.total_cmp(&b.time.0)));
        }
    }
    v
}

pub fn log_table(
    log: &Log,
    order: Order,
    extend: bool,
    period_hours: u32,
) -> Result<Vec<u8>, String> {
    let sheet = without_out_of_period(log, period_hours);
    let qsos = ordered(&sheet.qsos, order);
    let items: Vec<Item> = qsos.iter().map(|q| zlog_qxsl::to_item(q, 0)).collect();
    let zone = match log.utc_offset_minutes() {
        None => "UTC",
        Some(_) => "JST",
    };
    let mut table = rxsl::sheets::Sheet::default();
    table
        .embed(&Jarl { offset: 0, zone }, &items)
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let rows = table.table.split(|b| *b == b'\n').filter(|r| !r.is_empty());
    for (n, row) in rows.enumerate() {
        out.extend(row.strip_suffix(b"\r").unwrap_or(row));
        match (extend, n) {
            (false, _) => {}
            (true, 0) => out.extend(b" TX#"),
            (true, n) => out.extend(format!(" {}", qsos[n - 1].tx).bytes()),
        }
        out.extend(b"\r\n");
    }
    Ok(out)
}

pub fn elog(form: &Form, score: &Score, log: &Log, period_hours: u32) -> Result<Vec<u8>, String> {
    let mut sheet = form.sheet(score).to_sheet();
    sheet.table_type = Some(TABLE_TYPE.into());
    sheet.table = log_table(log, form.order, form.field_extend, period_hours)?;
    JarlSheet.encode(&sheet).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::DelphiTime;

    const ALL: (Band, Band) = (Band::B1_9, Band::HIGHEST);

    fn qso(h: u32, call: &str, band: Band, mode: Mode, multi: Option<&str>) -> Qso {
        Qso {
            time: DelphiTime::from_ymd_hms(2026, 4, 25, h, 3, 0),
            callsign: call.into(),
            band,
            mode,
            rst_sent: 599,
            rst_rcvd: 599,
            nr_sent: "10H".into(),
            nr_rcvd: "13M".into(),
            multi1: multi.unwrap_or("13").into(),
            new_multi1: multi.is_some(),
            points: 1,
            ..Qso::default()
        }
    }

    fn log() -> Log {
        let mut log = Log::default();
        log.header.rst_sent = (-540i16) as u16;
        log.qsos = vec![
            qso(21, "JA1ZLO", Band::B7, Mode::Cw, Some("13")),
            qso(22, "JR1ZTT/1", Band::B10G, Mode::Ssb, None),
        ];
        log
    }

    fn sheet() -> ScoreSheet {
        let mut bands = BTreeMap::new();
        let s = |q, p, m| BandScore {
            qsos: q,
            cw_qsos: 0,
            points: p,
            multi: m,
        };
        bands.insert(Band::B7, s(1, 1, 1));
        bands.insert(Band::B10G, s(1, 1, 0));
        bands.insert(Band::B10, s(0, 0, 0));
        ScoreSheet {
            bands,
            total: s(2, 2, 1),
            score: 2,
            with_coeff: None,
        }
    }

    fn form() -> Form {
        let mut f = Form::default();
        let s = &mut f.summary;
        s.version = Version::R2;
        s.category_code = "C7".into();
        s.signature = "Nextzlog".into();
        f.operators = "JA1ZLO\nJA1 YAA".into();
        f
    }

    fn table(l: &Log, order: Order, extend: bool, period: u32) -> String {
        let bytes = log_table(l, order, extend, period).unwrap();
        rxsl::text::decode_sjis(&bytes)
    }

    #[test]
    fn r1_scores_and_power_type_come_from_zlog() {
        let mut f = form();
        f.summary.version = Version::R1;
        f.power_measured = true;
        let mut sh = sheet();
        sh.with_coeff = Some((2.0, 4.0));
        let s = f.sheet(&Score::new(&sh, &log(), ALL));
        let bands: Vec<&str> = s.scores.iter().map(|s| s.band.as_str()).collect();
        assert_eq!(bands, ["7MHz", "10.1GHz"]);
        assert_eq!(
            (s.coeff.as_deref(), s.total_score.as_str()),
            (Some("2"), "4")
        );
        assert_eq!(s.power_type, summary::MEASURED);
        assert_eq!(s.operators, ["JA1ZLO", "JA1 YAA"]);
    }

    #[test]
    fn newcomer_and_age() {
        let mut f = form();
        f.summary.category_code = "PN".into();
        assert_eq!(f.missing(), ["missing.license_date"]);
        f.license_date = Some((2000, 1, 1));
        assert_eq!(f.missing(), ["missing.license_date"]);
        f.license_date = Some((2025, 3, 4));
        assert!(f.missing().is_empty());
        let s = f.sheet(&Score::new(&sheet(), &log(), ALL));
        assert_eq!(s.license_date.as_deref(), Some("2025年03月04日"));
        f.summary.category_code = "XS".into();
        f.summary.signature.clear();
        assert_eq!(f.missing(), ["missing.signature", "missing.age"]);
        f.summary.version = Version::R1;
        assert_eq!(f.missing(), ["missing.signature"]);
    }

    #[test]
    fn the_document_wraps_the_log_in_the_sheet() {
        let bytes = elog(&form(), &Score::new(&sheet(), &log(), ALL), &log(), 0).unwrap();
        let text = rxsl::text::decode_sjis(&bytes);
        assert!(text.starts_with("<SUMMARYSHEET VERSION=R2.1>\r\n"));
        assert!(text.contains("<MULTIOPLIST>JA1ZLO,\"JA1 YAA\"</MULTIOPLIST>\r\n"));
        assert!(text.contains("</SUMMARYSHEET>\r\n<LOGSHEET TYPE=\"ZLOG_REIWA\">\r\nDATE (JST)"));
        assert!(text.ends_with("\r\n</LOGSHEET>\r\n"));
    }

    #[test]
    fn order_by_band_then_time() {
        let mut l = log();
        l.qsos.push(qso(20, "JA2AAA", Band::B10G, Mode::Cw, None));
        let t = table(&l, Order::BandThenTime, false, 0);
        let at = |c: &str| t.find(c).unwrap();
        assert!(at("JA1ZLO") < at("JA2AAA") && at("JA2AAA") < at("JR1ZTT/1"));
    }

    #[test]
    fn the_log_skips_out_of_period() {
        let mut l = log();
        l.set_start_time(DelphiTime::from_ymd_hms(2026, 4, 25, 21, 0, 0));
        assert!(!table(&l, Order::AsEntered, false, 1).contains("JR1ZTT/1"));
        assert!(table(&l, Order::AsEntered, false, 0).contains("JR1ZTT/1"));
    }

    #[test]
    fn a_utc_log_keeps_utc_times() {
        let mut l = log();
        l.header.rst_sent = zlog_core::log::USE_UTC;
        let t = table(&l, Order::AsEntered, false, 0);
        assert!(t.starts_with("DATE (UTC) TIME"), "{t}");
        assert!(t.contains("2026-04-25 21:03"), "{t}");
        assert!(table(&log(), Order::AsEntered, false, 0).starts_with("DATE (JST)"));
    }

    #[test]
    fn the_extended_log_adds_the_tx_number() {
        let mut l = log();
        l.qsos.iter_mut().for_each(|q| q.tx = 2);
        let t = table(&l, Order::AsEntered, true, 0);
        assert!(t.contains("Pts TX#\r\n"), "{t}");
        assert!(t.contains(" 13       1 2\r\n"), "{t}");
    }

    #[test]
    fn the_log_keeps_shift_jis_bytes() {
        let mut l = log();
        l.qsos[0].nr_rcvd = "\u{6771}\u{4eac}".into();
        let bytes = log_table(&l, Order::AsEntered, false, 0).unwrap();
        assert!(bytes.windows(4).any(|w| w == [0x93, 0x8c, 0x8b, 0x9e]));
    }

    #[test]
    fn characters_shift_jis_lacks_stay_as_rxsl_writes_them() {
        let mut l = log();
        l.qsos[0].nr_rcvd = "13\u{1F600}".into();
        assert!(table(&l, Order::AsEntered, false, 0).contains(" 13&#128512; "));
    }

    #[test]
    fn rows_recounted_from_the_log() {
        let mut l = log();
        let mut invalid = qso(23, "JA9XXX", Band::B7, Mode::Cw, Some("09"));
        invalid.invalid = true;
        let mut both = qso(23, "JA8YYY", Band::B7, Mode::Ssb, Some("08"));
        both.new_multi2 = true;
        l.qsos.extend([invalid, both]);
        let score = Score::new(&sheet(), &l, ALL);
        let (b, row) = score.bands[0];
        assert_eq!(b, Band::B7);
        assert_eq!((row.qsos, row.cw_qsos, row.points, row.multi), (2, 1, 2, 3));
        assert_eq!(score.total_score, 9.0);
        assert_eq!(
            Score::new(&sheet(), &l, (Band::B1_9, Band::B430))
                .bands
                .len(),
            1
        );
    }

    #[test]
    fn defaults_fill_only_empty_fields() {
        let mut f = Form::default();
        assert_eq!(f.summary.version, Version::R1, "zLog starts at R1.0");
        f.fill_defaults("ALL JA", "JA1ZLO", &["JA1AAA", "", " JA1BBB ", "JA1AAA"]);
        assert_eq!(f.summary.contest, "ALL JA");
        assert_eq!(f.operators, "JA1AAA\nJA1BBB");
        f.fill_defaults("Other", "JA9ZZZ", &["JA1CCC"]);
        assert_eq!(
            (f.summary.contest.as_str(), f.summary.callsign.as_str()),
            ("ALL JA", "JA1ZLO")
        );
    }
}
