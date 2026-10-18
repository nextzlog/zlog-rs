//! Helpers shared by the contests of this crate.

use std::collections::{BTreeMap, HashSet};

use zlog_core::{Band, BandScore, Log, Mode, Power, Qso, Station};

/// Dupe flags with a contest's own rule (`TLog.SetDupeFlags`, UzLogQSO.pas:4119):
/// core callsign and band, plus the mode (or the mode group with all-phone) when
/// different modes are allowed. Invalid QSOs are never dupes.
pub fn dupe_flags(log: &Log, different_mode: bool, all_phone: bool) -> Vec<bool> {
    let mut seen = HashSet::new();
    log.qsos
        .iter()
        .map(|q| !q.invalid && !seen.insert(dupe_key(q, different_mode, all_phone)))
        .collect()
}

pub fn dupe_key(q: &Qso, different_mode: bool, all_phone: bool) -> String {
    let mut key = format!("{}\t{}", q.core_call(), q.band.label());
    if different_mode {
        key.push('\t');
        key.push_str(if all_phone {
            q.mode_group()
        } else {
            q.mode.label()
        });
    }
    key
}

/// Whether a callsign is Japanese (`IsDomestic`, UzLogGlobal.pas:4777).
pub fn is_domestic(call: &str) -> bool {
    let c: Vec<char> = call.chars().collect();
    if c.is_empty() {
        return true;
    }
    if c.len() < 3 {
        return false;
    }
    let (s1, s2, s3) = (c[0], c[1], c[2]);
    (s1 == 'J' && ('A'..='S').contains(&s2))
        || (s1 == '7' && s2 == 'J')
        || (s1 == '7' && ('K'..='N').contains(&s2) && ('1'..='4').contains(&s3))
        || (s1 == '8' && ('J'..='N').contains(&s2))
}

/// zLog's three-digit serial text (`TQSO.GetSerialStr`, UzLogQSO.pas:781).
pub fn serial_str(serial: i32) -> String {
    if serial < 1000 {
        let s = format!("0000{serial}");
        s[s.len() - 3..].to_string()
    } else {
        serial.to_string()
    }
}

/// What a sent-number template may refer to.
pub struct SentContext<'a> {
    pub station: &'a Station,
    pub prov: &'a str,
    pub qth: &'a str,
    pub power: Power,
    pub serial: Option<i32>,
}

/// Expand a sent-number template as `TContest.SetNrSent` and `SetStrNoAbbrev`
/// (UzLogContest.pas:532, UzLogCW.pas:202). `None` when the template needs a value
/// that is not known; zLog then leaves the number as it is.
pub fn expand_sent(template: &str, cx: &SentContext) -> Option<String> {
    let t = template.to_uppercase();
    let code = cx.power.code().map(String::from).unwrap_or_default();
    let needs = |v: &str| t.contains(v);
    if (needs("$Q") && cx.qth.is_empty())
        || (needs("$V") && cx.prov.is_empty())
        || (needs("$P") && code.is_empty())
        || (needs("$S") && cx.serial.is_none())
    {
        return None;
    }
    // Variables whose values this program does not keep: zone, IARU zone, age, IOTA,
    // the number already sent.
    if ["$Z", "$I", "$A", "$T", "$H"].iter().any(|v| needs(v)) {
        return None;
    }
    let mut s = t;
    for tag in ["[AR]", "[SK]", "[VA]", "[KN]", "[BK]", "[BT]"] {
        s = s.replace(tag, "");
    }
    s = s
        .replace("$Q", cx.qth)
        .replace("$V", cx.prov)
        .replace("$O", &cx.station.operator)
        .replace("$P", &code)
        .replace("$M", &cx.station.callsign.to_uppercase());
    if let Some(n) = cx.serial {
        s = s.replace("$S", &serial_str(n));
    }
    Some(s.replace('_', ""))
}

/// Score sheet totals from rows.
pub fn sum(rows: impl Iterator<Item = BandScore>) -> BandScore {
    rows.fold(BandScore::default(), |a, b| BandScore {
        qsos: a.qsos + b.qsos,
        cw_qsos: a.cw_qsos + b.cw_qsos,
        points: a.points + b.points,
        multi: a.multi + b.multi,
    })
}

/// Rows of the score window for the given bands, zeros included.
pub fn empty_rows(bands: impl Iterator<Item = Band>) -> BTreeMap<Band, BandScore> {
    bands.map(|b| (b, BandScore::default())).collect()
}

/// `TBasicScore.AddNoUpdate` (UBasicScore.pas:134): count the QSO when its mode
/// belongs to the contest mode; returns whether it did (`FValidQso`).
pub fn count_qso(row: &mut BandScore, mode: zlog_core::ContestMode, q: &Qso) -> bool {
    let valid = mode.accepts(q.mode);
    if valid {
        row.qsos += 1;
        if q.mode == Mode::Cw {
            row.cw_qsos += 1;
        }
    }
    valid
}

/// Number of multipliers in a multiplier text (`SL.CommaText := Multi1; SL.Count`).
pub fn multi_count(text: &str) -> u32 {
    if text.is_empty() {
        0
    } else {
        text.split(',').count() as u32
    }
}

/// zLog's band membership of a QSO in `RenewScoreAndMulti` (UzLogContest.pas:735-742).
pub fn in_contest(band: Band, (lo, hi): (Band, Band), warc: bool) -> bool {
    band >= lo && band <= hi && (warc || !band.is_warc())
}
