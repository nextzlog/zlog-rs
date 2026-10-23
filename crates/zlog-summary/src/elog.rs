//! The JARL E-Log (`TformELogJarlEx`): a summary sheet, R1.0 or R2.1, followed by
//! the log sheet (`WriteLogSheetStd`, `ZLOG_REIWA`).
//!
//! The summary sheet is written as zLog writes it, line for line. The QSO lines of
//! the log sheet are the QxSL `jarl` format as zlog-qxsl writes it
//! ([`zlog_qxsl::encode`]); departure: zLog's own lines (`TQSO.FormatELogStd`,
//! with the TX number column and the points zeroed on bands that do not count)
//! are not written. The summary's Japanese fixed
//! texts (the power type, the oaths, the date formats) are in `lang/ja.lang` under
//! `doc.`, since the document is in Japanese whatever the user interface language.
//!
//! References: <https://www.jarl.org/Japanese/1_Tanoshimo/1-1_Contest/e-log_R1.0.htm>
//! and the R2.0 tag list quoted in `UELogJarlEx.pas`.

use std::collections::BTreeMap;

use zlog_core::{Band, BandScore, Contest, Log, Mode, Qso, ScoreSheet};

use crate::delphi::{comma_text, float_to_str, memo_text, CRLF};
use crate::{texts, without_out_of_period};

/// The summary sheet version (`TabControl1`); zLog starts at R1.0
/// (`ReadInteger('SummaryInfo', 'Version', 0)`, `UELogJarlEx.pas`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Version {
    #[default]
    R1,
    R2,
}

/// The order of the log sheet (`comboOutputOrder`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Order {
    #[default]
    AsEntered,
    Time,
    BandThenTime,
}

/// The fields of the summary sheet, as entered (`SummaryInfo` of `zlog.ini`).
/// Multi-line fields (address, equipment, comments, oath, operators) hold lines
/// separated by `\n`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Form {
    pub version: Version,
    pub contest_name: String,
    pub category_code: String,
    /// R1.0 only.
    pub category_name: String,
    pub callsign: String,
    pub op_callsign: String,
    pub address: String,
    pub tel: String,
    pub name: String,
    pub email: String,
    /// R1.0 only.
    pub license: String,
    pub power: String,
    /// R1.0 only: the rated (false) or measured (true) power.
    pub power_measured: bool,
    pub qth: String,
    pub power_supply: String,
    /// R1.0 only.
    pub equipment: String,
    /// R2.1, newcomer categories only: (year, month, day).
    pub license_date: Option<(i32, u32, u32)>,
    /// R2.1, senior and junior categories only.
    pub age: String,
    pub comments: String,
    pub multi_op_list: String,
    pub club_id: String,
    /// R1.0 only.
    pub club_name: String,
    pub oath: String,
    pub date: String,
    pub signature: String,
    pub order: Order,
    /// Categories that need the licence date (`ELOG/NewComer`), comma separated.
    pub newcomer_categories: String,
    /// Categories that need the age (`ELOG/SeniorJunior`), comma separated.
    pub senior_junior_categories: String,
}

/// zLog's defaults of the category lists.
pub const NEWCOMER: &str = "PN";
pub const SENIOR_JUNIOR: &str = "XS,CS,SOSV,SOJR";

fn listed(list: &str, category: &str) -> bool {
    list.split(',').any(|c| c == category)
}

impl Form {
    /// Fill the fields zLog fills when the form opens and the user has not
    /// (`TformELogJarlEx.FormShow`): the contest name, the own callsign
    /// (`Categories/MyCall`) and the multi-operator list (`dmZlogGlobal.OpList`).
    /// Fields already entered are kept.
    ///
    /// Departure: zLog lists its operator list; the operators here are those of
    /// the log's QSOs and the current one, in the order they first appear.
    pub fn fill_defaults(&mut self, contest: &str, callsign: &str, operators: &[&str]) {
        if self.contest_name.is_empty() {
            self.contest_name = contest.into();
        }
        if self.callsign.is_empty() {
            self.callsign = callsign.into();
        }
        if self.multi_op_list.is_empty() {
            let mut list: Vec<&str> = Vec::new();
            for op in operators.iter().map(|o| o.trim()) {
                if !op.is_empty() && !list.contains(&op) {
                    list.push(op);
                }
            }
            self.multi_op_list = list.join("\n");
        }
    }

    /// `IsNewcomer`.
    pub fn is_newcomer(&self) -> bool {
        listed(&self.newcomer_categories, &self.category_code)
    }

    /// `IsSeniorJunior`.
    pub fn is_senior_junior(&self) -> bool {
        listed(&self.senior_junior_categories, &self.category_code)
    }

    /// What must be filled in before the document is made
    /// (`buttonCreateLogClick`): the text keys of the missing fields.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.signature.is_empty() {
            out.push("missing.signature");
        }
        if self.version == Version::R2 {
            // zLog's date picker starts at 2000-01-01, and that date counts as not
            // entered (`buttonCreateLogClick`, `UELogJarlEx.pas`).
            if self.is_newcomer() && matches!(self.license_date, None | Some((2000, 1, 1))) {
                out.push("missing.license_date");
            }
            if self.is_senior_junior() && self.age.is_empty() {
                out.push("missing.age");
            }
        }
        out
    }
}

/// The JARL oath for a JARL contest or for another organizer (`radioOrganizer*`).
pub fn oath(jarl: bool) -> &'static str {
    texts().get(
        "ja",
        if jarl {
            "doc.oath_jarl"
        } else {
            "doc.oath_other"
        },
    )
}

/// Today's date as zLog fills it in (`yyyy"年"m"月"d"日"`).
pub fn date_text(y: i32, m: u32, d: u32) -> String {
    texts().format("ja", "doc.date", &[&y, &m, &d])
}

/// The score part of the summary: the bands that count and their totals.
#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub bands: Vec<(Band, BandScore)>,
    pub total: BandScore,
    /// The coefficient (Field Day), when the contest has one.
    pub coeff: Option<f64>,
    pub total_score: f64,
}

impl Score {
    /// The summary's score as zLog's form computes it.
    ///
    /// A band counts when the contest scored points on it (`SetBandUsed`) and
    /// lies in the contest's band `range`; zLog also asks whether the band is
    /// visible in its band menu, which zlog-rs does not have, so the contest's
    /// band range stands for it. The WARC bands have no row in zLog's form.
    ///
    /// The rows are counted again from the log (`CalcAll`): every QSO not marked
    /// invalid (dupes included), its points, and its new multipliers
    /// (`NewMulti1` plus `NewMulti2`). The total score is multipliers times
    /// points times the coefficient (1 when the contest has none), as `CalcAll`
    /// writes it; zLog's ZyLO plug-in total (`zyloRequestTotal`) is not ported.
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

fn tag(out: &mut String, name: &str, value: &str) {
    out.push_str(&format!("<{name}>{value}</{name}>{CRLF}"));
}

fn multi_op_list(form: &Form) -> Option<String> {
    let lines: Vec<&str> = form.multi_op_list.lines().collect();
    (!form.multi_op_list.is_empty()).then(|| comma_text(&lines))
}

/// `SCORE BAND=` label: `7MHz`, `10.1GHz`.
fn score_band(b: Band) -> String {
    if b >= Band::B10G {
        format!("{}Hz", b.label())
    } else {
        format!("{}MHz", b.label())
    }
}

/// The R1.0 summary sheet (`WriteSummarySheetR1`).
pub fn summary_r1(form: &Form, score: &Score) -> String {
    let mut out = format!("<SUMMARYSHEET VERSION=R1.0>{CRLF}");
    tag(&mut out, "CONTESTNAME", &form.contest_name);
    tag(&mut out, "CATEGORYCODE", &form.category_code);
    tag(&mut out, "CATEGORYNAME", &form.category_name);
    tag(&mut out, "CALLSIGN", &form.callsign);
    tag(&mut out, "OPCALLSIGN", &form.op_callsign);
    let row = |s: &BandScore| format!("{},{},{}", s.qsos, s.points, s.multi);
    for (b, s) in &score.bands {
        out.push_str(&format!(
            "<SCORE BAND={}>{}</SCORE>{CRLF}",
            score_band(*b),
            row(s)
        ));
    }
    out.push_str(&format!(
        "<SCORE BAND=TOTAL>{}</SCORE>{CRLF}",
        row(&score.total)
    ));
    if let Some(c) = score.coeff {
        tag(&mut out, "FDCOEFF", &float_to_str(c));
    }
    tag(&mut out, "TOTALSCORE", &float_to_str(score.total_score));
    tag(&mut out, "ADDRESS", &memo_text(&form.address));
    tag(&mut out, "TEL", &form.tel);
    tag(&mut out, "NAME", &form.name);
    tag(&mut out, "EMAIL", &form.email);
    tag(&mut out, "LICENSECLASS", &form.license);
    tag(&mut out, "POWER", &form.power);
    let power_type = if form.power_measured {
        "doc.measured"
    } else {
        "doc.rated"
    };
    tag(&mut out, "POWERTYPE", texts().get("ja", power_type));
    tag(&mut out, "OPPLACE", &form.qth);
    tag(&mut out, "POWERSUPPLY", &form.power_supply);
    tag(&mut out, "EQUIPMENT", &memo_text(&form.equipment));
    tag(&mut out, "COMMENTS", &memo_text(&form.comments));
    if let Some(list) = multi_op_list(form) {
        tag(&mut out, "MULTIOPLIST", &list);
    }
    tag(&mut out, "REGCLUBNUMBER", &form.club_id);
    tag(&mut out, "REGCLUBNAME", &form.club_name);
    tag(&mut out, "OATH", &memo_text(&form.oath));
    tag(&mut out, "DATE", &form.date);
    tag(&mut out, "SIGNATURE", &form.signature);
    out.push_str(&format!("</SUMMARYSHEET>{CRLF}"));
    out
}

/// The R2.1 summary sheet (`WriteSummarySheetR2`).
pub fn summary_r2(form: &Form, score: &Score) -> String {
    let mut out = format!("<SUMMARYSHEET VERSION=R2.1>{CRLF}");
    tag(&mut out, "CONTESTNAME", &form.contest_name);
    tag(&mut out, "CATEGORYCODE", &form.category_code);
    tag(&mut out, "CALLSIGN", &form.callsign);
    tag(&mut out, "OPCALLSIGN", &form.op_callsign);
    tag(&mut out, "TOTALSCORE", &float_to_str(score.total_score));
    tag(&mut out, "ADDRESS", &memo_text(&form.address));
    tag(&mut out, "NAME", &form.name);
    tag(&mut out, "TEL", &form.tel);
    tag(&mut out, "EMAIL", &form.email);
    tag(&mut out, "POWER", &form.power);
    if let Some(c) = score.coeff {
        tag(&mut out, "FDCOEFF", &float_to_str(c));
    }
    tag(&mut out, "OPPLACE", &form.qth);
    tag(&mut out, "POWERSUPPLY", &form.power_supply);
    if form.is_newcomer() {
        let (y, m, d) = form.license_date.unwrap_or((2000, 1, 1));
        let date = texts().format(
            "ja",
            "doc.license_date",
            &[&y, &format!("{m:02}"), &format!("{d:02}")],
        );
        tag(&mut out, "LICENSEDATE", &date);
    }
    if form.is_senior_junior() {
        tag(&mut out, "AGE", &form.age);
    }
    tag(&mut out, "COMMENTS", &memo_text(&form.comments));
    if let Some(list) = multi_op_list(form) {
        tag(&mut out, "MULTIOPLIST", &list);
    }
    tag(&mut out, "REGCLUBNUMBER", &form.club_id);
    tag(&mut out, "OATH", &memo_text(&form.oath));
    tag(&mut out, "DATE", &form.date);
    tag(&mut out, "SIGNATURE", &form.signature);
    out.push_str(&format!("</SUMMARYSHEET>{CRLF}"));
    out
}

/// The QSOs in the chosen order (`SortBy(soTime)`, then `SortBy(soBand)`).
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

/// The log sheet (`WriteLogSheetStd`): the QSOs in `order` in QxSL's `jarl`
/// format, in Shift_JIS. QSOs outside the contest period of `period_hours`
/// ([`Log::out_of_period`]) are left out, as zLog does with its default
/// `_output_outofperiod` off.
///
/// The times are written as logged, in the log's own zone, and a UTC log says so
/// in the head of the columns (`WriteLogSheetStd`: `DATE (UTC)` when the log
/// records UTC, else `DATE (JST)`).
pub fn log_sheet(
    log: &Log,
    contest: &dyn Contest,
    order: Order,
    period_hours: u32,
) -> Result<Vec<u8>, String> {
    let mut sheet = without_out_of_period(log, period_hours);
    sheet.qsos = ordered(&sheet.qsos, order).into_iter().cloned().collect();
    let mut out = format!("<LOGSHEET TYPE=\"ZLOG_REIWA\">{CRLF}").into_bytes();
    use zlog_core::LogFormat;
    let offset = log.utc_offset_minutes();
    let jarl = qxsl::formats::jarl::Jarl {
        offset: offset.unwrap_or(0),
    };
    let lines = zlog_qxsl::QxslFormat::new(Box::new(jarl)).export(&sheet, contest)?;
    if offset.is_none() {
        // The sheet is Shift_JIS: the head is replaced on the bytes (it is ASCII).
        let head = qxsl::formats::jarl::HEADER.as_bytes();
        let utc = qxsl::formats::jarl::HEADER.replacen("DATE (JST)", "DATE (UTC)", 1);
        match lines.windows(head.len()).position(|w| w == head) {
            Some(p) => {
                out.extend(&lines[..p]);
                out.extend(utc.bytes());
                out.extend(&lines[p + head.len()..]);
            }
            None => out.extend(lines),
        }
    } else {
        out.extend(lines);
    }
    out.extend(format!("</LOGSHEET>{CRLF}").bytes());
    Ok(out)
}

/// The summary sheet of the form's version.
pub fn summary(form: &Form, score: &Score) -> String {
    match form.version {
        Version::R1 => summary_r1(form, score),
        Version::R2 => summary_r2(form, score),
    }
}

/// The whole E-Log file in Shift_JIS (`CreateELogR1`, `CreateELogR2`);
/// `period_hours` as for [`log_sheet`].
pub fn elog(
    form: &Form,
    score: &Score,
    log: &Log,
    contest: &dyn Contest,
    period_hours: u32,
) -> Result<Vec<u8>, String> {
    let mut out = qxsl::text::encode_sjis(&summary(form, score));
    out.extend(log_sheet(log, contest, form.order, period_hours)?);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Plain;
    use zlog_core::DelphiTime;

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
        Form {
            version: Version::R2,
            contest_name: "ALL JA".into(),
            category_code: "C7".into(),
            category_name: "Single 7".into(),
            callsign: "JA1ZLO".into(),
            address: "Tokyo\nBunkyo\n\n".into(),
            name: "Nextzlog".into(),
            power: "10".into(),
            multi_op_list: "JA1ZLO\nJA1 YAA".into(),
            oath: "I swear.".into(),
            date: "2026-04-26".into(),
            signature: "Nextzlog".into(),
            newcomer_categories: NEWCOMER.into(),
            senior_junior_categories: SENIOR_JUNIOR.into(),
            ..Form::default()
        }
    }

    #[test]
    fn r2_document() {
        let score = Score::new(&sheet(), &log(), ALL);
        let bytes = elog(&form(), &score, &log(), &Plain, 0).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let summary = [
            "<SUMMARYSHEET VERSION=R2.1>",
            "<CONTESTNAME>ALL JA</CONTESTNAME>",
            "<CATEGORYCODE>C7</CATEGORYCODE>",
            "<CALLSIGN>JA1ZLO</CALLSIGN>",
            "<OPCALLSIGN></OPCALLSIGN>",
            "<TOTALSCORE>2</TOTALSCORE>",
            "<ADDRESS>Tokyo",
            "Bunkyo",
            "</ADDRESS>",
            "<NAME>Nextzlog</NAME>",
            "<TEL></TEL>",
            "<EMAIL></EMAIL>",
            "<POWER>10</POWER>",
            "<OPPLACE></OPPLACE>",
            "<POWERSUPPLY></POWERSUPPLY>",
            "<COMMENTS></COMMENTS>",
            "<MULTIOPLIST>JA1ZLO,\"JA1 YAA\"</MULTIOPLIST>",
            "<REGCLUBNUMBER></REGCLUBNUMBER>",
            "<OATH>I swear.",
            "</OATH>",
            "<DATE>2026-04-26</DATE>",
            "<SIGNATURE>Nextzlog</SIGNATURE>",
            "</SUMMARYSHEET>",
            "<LOGSHEET TYPE=\"ZLOG_REIWA\">",
            "",
        ];
        let body =
            String::from_utf8(zlog_qxsl::encode("jarl", &log(), &Plain, "").unwrap()).unwrap();
        assert_eq!(text, summary.join(CRLF) + &body + "</LOGSHEET>\r\n");
    }

    #[test]
    fn r1_summary() {
        let mut f = form();
        f.version = Version::R1;
        f.power_measured = true;
        let mut sh = sheet();
        sh.with_coeff = Some((2.0, 4.0));
        let text = summary_r1(&f, &Score::new(&sh, &log(), ALL));
        let lines: Vec<&str> = text.split(CRLF).collect();
        assert_eq!(lines[0], "<SUMMARYSHEET VERSION=R1.0>");
        assert_eq!(lines[3], "<CATEGORYNAME>Single 7</CATEGORYNAME>");
        assert_eq!(lines[6], "<SCORE BAND=7MHz>1,1,1</SCORE>");
        assert_eq!(lines[7], "<SCORE BAND=10.1GHz>1,1,0</SCORE>");
        assert_eq!(lines[8], "<SCORE BAND=TOTAL>2,2,1</SCORE>");
        assert_eq!(lines[9], "<FDCOEFF>2</FDCOEFF>");
        assert_eq!(lines[10], "<TOTALSCORE>4</TOTALSCORE>");
        let power = format!(
            "<POWERTYPE>{}</POWERTYPE>",
            texts().get("ja", "doc.measured")
        );
        assert!(lines.contains(&power.as_str()));
        assert!(lines.contains(&"<EQUIPMENT></EQUIPMENT>"));
        assert!(lines.contains(&"<REGCLUBNAME></REGCLUBNAME>"));
    }

    #[test]
    fn newcomer_and_age() {
        let mut f = form();
        f.category_code = "PN".into();
        assert_eq!(f.missing(), ["missing.license_date"]);
        f.license_date = Some((2025, 3, 4));
        assert!(f.missing().is_empty());
        let text = summary_r2(&f, &Score::new(&sheet(), &log(), ALL));
        let date = texts().format("ja", "doc.license_date", &[&2025, &"03", &"04"]);
        assert!(text.contains(&format!("<LICENSEDATE>{date}</LICENSEDATE>")));
        f.category_code = "XS".into();
        f.signature.clear();
        assert_eq!(f.missing(), ["missing.signature", "missing.age"]);
    }

    #[test]
    fn order_by_band_then_time() {
        let mut l = log();
        l.qsos.push(qso(20, "JA2AAA", Band::B10G, Mode::Cw, None));
        let calls: Vec<&str> = ordered(&l.qsos, Order::BandThenTime)
            .iter()
            .map(|q| q.callsign.as_str())
            .collect();
        assert_eq!(calls, ["JA1ZLO", "JA2AAA", "JR1ZTT/1"]);
    }

    const ALL: (Band, Band) = (Band::B1_9, Band::HIGHEST);

    #[test]
    fn newcomer_default_date_is_missing() {
        // The form's date picker starts at 2000-01-01, which zLog refuses; before
        // the fix drawing the row made the date "entered".
        let mut f = form();
        f.category_code = "PN".into();
        f.license_date = Some((2000, 1, 1));
        assert_eq!(f.missing(), ["missing.license_date"]);
        f.license_date = Some((2000, 1, 2));
        assert!(f.missing().is_empty());
        // R1.0 has no licence date.
        f.license_date = Some((2000, 1, 1));
        f.version = Version::R1;
        assert!(f.missing().is_empty());
        // Other categories do not need one.
        f.version = Version::R2;
        f.category_code = "C7".into();
        assert!(f.missing().is_empty());
    }

    #[test]
    fn bands_outside_the_range_have_no_row() {
        let score = Score::new(&sheet(), &log(), (Band::B1_9, Band::B430));
        assert_eq!(score.bands.len(), 1);
    }

    #[test]
    fn rows_recounted_from_the_log() {
        // The sheet says 5 QSOs on 7 MHz; zLog's form counts the log again:
        // dupes count, invalid QSOs do not, multipliers are NewMulti1 + NewMulti2.
        let mut sh = sheet();
        sh.bands.insert(
            Band::B7,
            BandScore {
                qsos: 5,
                cw_qsos: 5,
                points: 9,
                multi: 9,
            },
        );
        let mut l = log();
        let mut dupe = qso(23, "JA1ZLO", Band::B7, Mode::Cw, None);
        dupe.points = 0;
        dupe.dupe = true;
        let mut invalid = qso(23, "JA9XXX", Band::B7, Mode::Cw, Some("09"));
        invalid.invalid = true;
        let mut both = qso(23, "JA8YYY", Band::B7, Mode::Ssb, Some("08"));
        both.new_multi2 = true;
        l.qsos.extend([dupe, invalid, both]);
        let score = Score::new(&sh, &l, ALL);
        let (b, row) = score.bands[0];
        assert_eq!(b, Band::B7);
        assert_eq!((row.qsos, row.cw_qsos, row.points, row.multi), (3, 2, 2, 3));
        assert_eq!(score.total.multi, 3);
        // 3 multipliers times 2 + 1 points (7 MHz and 10.1 GHz).
        assert_eq!(score.total_score, 9.0);
    }

    #[test]
    fn score_edge_cases() {
        let empty = Score::new(&ScoreSheet::default(), &Log::default(), ALL);
        assert!(empty.bands.is_empty());
        assert_eq!(empty.total, BandScore::default());
        assert_eq!(empty.total_score, 0.0);
        // A used band whose QSOs are all invalid keeps an empty row.
        let mut l = log();
        l.qsos.iter_mut().for_each(|q| q.invalid = true);
        let score = Score::new(&sheet(), &l, ALL);
        assert_eq!(score.bands.len(), 2);
        assert_eq!(score.total, BandScore::default());
        // The coefficient multiplies the recounted score.
        let mut sh = sheet();
        sh.with_coeff = Some((1.5, 0.0));
        let score = Score::new(&sh, &log(), ALL);
        assert_eq!(score.total_score, 3.0);
        // An empty log writes the head of the sheet only.
        let text = log_sheet(&Log::default(), &Plain, Order::Time, 24).unwrap();
        assert_eq!(
            text,
            b"<LOGSHEET TYPE=\"ZLOG_REIWA\">\r\nDATE (JST) TIME   BAND MODE  CALLSIGN      SENTNo      RCVDNo      Mlt    Pts\r\n</LOGSHEET>\r\n"
        );
    }

    #[test]
    fn log_sheet_skips_out_of_period() {
        // Before the fix every QSO was written whatever the period.
        let mut l = log();
        l.set_start_time(DelphiTime::from_ymd_hms(2026, 4, 25, 21, 0, 0));
        let sheet = |l: &Log| {
            String::from_utf8(log_sheet(l, &Plain, Order::AsEntered, 1).unwrap()).unwrap()
        };
        // A 1 h period ends at 22:00; the 22:03 QSO is left out.
        let text = sheet(&l);
        assert!(text.contains("JA1ZLO "));
        assert!(!text.contains("JR1ZTT/1"));
        // A QSO exactly at the end of the period is in it.
        l.qsos[1].time = DelphiTime::from_ymd_hms(2026, 4, 25, 22, 0, 0);
        assert!(sheet(&l).contains("JR1ZTT/1"));
        // Before the start it is out.
        l.qsos[0].time = DelphiTime::from_ymd_hms(2026, 4, 25, 20, 59, 0);
        assert!(!sheet(&l).contains("JA1ZLO "));
    }

    fn lines(l: &Log) -> String {
        String::from_utf8(log_sheet(l, &Plain, Order::AsEntered, 0).unwrap()).unwrap()
    }

    #[test]
    fn log_sheet_of_a_utc_log_keeps_utc_times() {
        // Before the fix: DATE (JST) and the times moved by 9 hours.
        let mut l = log();
        l.header.rst_sent = zlog_core::log::USE_UTC;
        let text = lines(&l);
        assert!(text.contains("DATE (UTC) TIME"), "{text}");
        assert!(!text.contains("JST"));
        assert!(text.contains("2026-04-25 21:03"), "{text}");
        assert!(!text.contains("2026-04-26 06:03"));
    }

    #[test]
    fn log_sheet_of_a_jst_log_is_unchanged() {
        let text = lines(&log());
        assert!(text.contains("DATE (JST) TIME"));
        assert!(text.contains("2026-04-25 21:03"), "{text}");
    }

    #[test]
    fn log_sheet_of_another_zone_keeps_its_times() {
        // zLog writes the times as logged, whatever the zone.
        let mut l = log();
        l.header.rst_sent = (-120i16) as u16;
        let text = lines(&l);
        assert!(text.contains("DATE (JST) TIME"));
        assert!(text.contains("2026-04-25 21:03"), "{text}");
    }

    #[test]
    fn log_sheet_of_a_utc_log_keeps_shift_jis_bytes() {
        // Before the fix the Shift_JIS bytes of a number became U+FFFD.
        let mut l = log();
        l.header.rst_sent = zlog_core::log::USE_UTC;
        l.qsos[0].nr_rcvd = "\u{6771}\u{4eac}".into();
        let bytes = log_sheet(&l, &Plain, Order::AsEntered, 0).unwrap();
        let want = [0x93, 0x8c, 0x8b, 0x9e];
        assert!(bytes.windows(4).any(|w| w == want), "{bytes:?}");
        assert!(bytes.windows(10).any(|w| w == b"DATE (UTC)"));
    }

    #[test]
    fn log_sheet_of_an_empty_utc_log() {
        let mut l = Log::default();
        l.header.rst_sent = zlog_core::log::USE_UTC;
        assert_eq!(
            lines(&l),
            "<LOGSHEET TYPE=\"ZLOG_REIWA\">\r\nDATE (UTC) TIME   BAND MODE  CALLSIGN      SENTNo      RCVDNo      Mlt    Pts\r\n</LOGSHEET>\r\n"
        );
    }

    #[test]
    fn defaults_fill_only_empty_fields() {
        let mut f = Form::default();
        assert_eq!(f.version, Version::R1, "zLog starts at R1.0");
        f.fill_defaults("ALL JA", "JA1ZLO", &["JA1AAA", "", " JA1BBB ", "JA1AAA"]);
        assert_eq!(f.contest_name, "ALL JA");
        assert_eq!(f.callsign, "JA1ZLO");
        assert_eq!(f.multi_op_list, "JA1AAA\nJA1BBB");
        f.callsign = "JA1YYY".into();
        f.multi_op_list = "JA9ZZZ".into();
        f.fill_defaults("Other", "JA1ZLO", &["JA1CCC"]);
        assert_eq!(
            (
                f.contest_name.as_str(),
                f.callsign.as_str(),
                f.multi_op_list.as_str()
            ),
            ("ALL JA", "JA1YYY", "JA9ZZZ")
        );
        // No operators: the list stays empty.
        let mut g = Form::default();
        g.fill_defaults("", "", &[]);
        assert_eq!(g, Form::default());
    }

    #[test]
    fn log_sheet_in_band_order() {
        let mut l = log();
        l.qsos.push(qso(20, "JA2AAA", Band::B10G, Mode::Cw, None));
        let text =
            String::from_utf8(log_sheet(&l, &Plain, Order::BandThenTime, 0).unwrap()).unwrap();
        let at = |c: &str| text.find(c).unwrap();
        assert!(at("JA1ZLO") < at("JA2AAA") && at("JA2AAA") < at("JR1ZTT/1"));
    }
}
