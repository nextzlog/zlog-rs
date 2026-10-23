//! Helpers shared by the contests of this crate.

use std::collections::{BTreeMap, HashSet};

use zlog_core::{Band, BandScore, Log, Mode, Power, Qso, Station};

use crate::user::DatEntry;

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

/// Serial numbers of the next QSO, for all bands and per band, as zLog's
/// `TLog.GetCurrentSerial` and `TLog.GetLastSerial` (UzLogQSO.pas) on `TQSO.Serial`.
///
/// An empty log starts at the start number (CFG `SERIALSTART`, else 1). Otherwise
/// the next serial is that of the last QSO of the same TX (`Q.TX = txnr`), on the
/// same band for `SERIAL BAND`, plus one; with no such QSO it is 1 (zLog's
/// `GetLastSerial` returns 0), and a QSO stored with serial 0 is followed by 1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Serials {
    start: i32,
    /// Whether the log has any QSO (`Log.TotalQSO = 0`).
    any: bool,
    /// Last serial per TX, for all bands.
    all: BTreeMap<u8, i32>,
    /// Last serial per band and TX.
    band: BTreeMap<(Band, u8), i32>,
}

impl Serials {
    /// Counters before any QSO, starting at `start` (CFG `SERIALSTART`, else 1).
    pub fn new(start: i32) -> Self {
        Serials {
            start,
            ..Serials::default()
        }
    }

    /// Counters after the QSOs of a log.
    pub fn scan(log: &Log, start: i32) -> Self {
        let mut s = Serials::new(start);
        for q in &log.qsos {
            s.any = true;
            s.all.insert(q.tx, q.serial);
            s.band.insert((q.band, q.tx), q.serial);
        }
        s
    }

    /// The next serial of TX `tx` on a band, or on all bands with `None`.
    pub fn next(&self, band: Option<Band>, tx: u8) -> i32 {
        if !self.any {
            return self.start;
        }
        let last = match band {
            Some(b) => self.band.get(&(b, tx)),
            None => self.all.get(&tx),
        };
        last.copied().unwrap_or(0).saturating_add(1)
    }
}

/// What a sent-number template may refer to.
pub struct SentContext<'a> {
    pub station: &'a Station,
    pub prov: &'a str,
    pub qth: &'a str,
    pub power: Power,
    pub serial: Option<i32>,
    /// The handle for the QSO's mode ([`Station::handle_cw`] or
    /// [`Station::handle_ph`]).
    pub handle: &'a str,
}

/// Expand a sent-number template as `TMainForm.GetInitNrSent` (main.pas:16195)
/// after the checks of `TContest.SetNrSent` (UzLogContest.pas:532).
///
/// `$Q` QTH, `$V` prefecture, `$Z` CQ zone, `$I` IARU zone, `$A` age, `$T` IOTA,
/// `$H` handle, `$N` power, `$S` serial, `$O` operator, `$M` own call. `$P` becomes
/// empty: zLog shows the power code beside the number (`TQSO.GetNrSentStr`). Other
/// letters such as `$R` and `$C` stay as written, as in `GetInitNrSent`. `None` when
/// the template needs a value that is not known; zLog then leaves the number as it is.
pub fn expand_sent(template: &str, cx: &SentContext) -> Option<String> {
    let t = template.to_uppercase();
    let st = cx.station;
    let vars = [
        ("$Q", cx.qth),
        ("$V", cx.prov),
        ("$Z", st.cq_zone.as_str()),
        ("$I", st.iaru_zone.as_str()),
        ("$A", st.age.as_str()),
        ("$T", st.iota.as_str()),
        ("$H", cx.handle),
    ];
    if vars.iter().any(|(v, x)| t.contains(v) && x.is_empty())
        || (t.contains("$P") && cx.power.code().is_none())
        || (t.contains("$S") && cx.serial.is_none())
    {
        return None;
    }
    let mut s = t;
    for tag in ["[AR]", "[SK]", "[VA]", "[KN]", "[BK]", "[BT]"] {
        s = s.replace(tag, "");
    }
    for (v, x) in vars {
        s = s.replace(v, &x.to_uppercase());
    }
    // Departure: zLog's `$N` (`TQSO.GetPowerStr`) is the user's text for the power
    // code (`_powerH` and so on); this program has no such setting and sends the
    // power class.
    s = s
        .replace("$O", &st.operator.to_uppercase())
        .replace("$P", "")
        .replace("$N", &cx.power.label().to_uppercase())
        .replace("$M", &st.callsign.to_uppercase());
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
    let valid = mode.counts(q.mode);
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

/// A multiplier table of a built-in local contest (`CitiesCallBack`, UzLogExtension.pas:393): every line,
/// the title too, split at blanks into code and name (its first word).
pub fn rule_table(text: &str) -> Vec<DatEntry> {
    text.lines()
        .map(|l| {
            let mut words = l.split_whitespace();
            DatEntry {
                code: words.next().unwrap_or("").to_string(),
                name: words.next().unwrap_or("").to_string(),
            }
        })
        .collect()
}

/// The default checks of a local contest: not a dupe, band and mode allowed.
///
/// The ranges are bytes of [`Band`] and [`Mode`] (1.9 MHz = 0 to 10 GHz = 15; CW,
/// SSB, FM, AM, RTTY, OTHER), as the original rules give them.
pub fn allowed(q: &Qso, dupe: bool, bands: (u8, u8), modes: (u8, u8)) -> bool {
    let (b, m) = (q.band as u8, q.mode as u8);
    !dupe && (bands.0..=bands.1).contains(&b) && (modes.0..=modes.1).contains(&m)
}

/// `^\d{n,}` followed by one of `tails` (or nothing if `tails` is empty) to the end:
/// the digits and the tail.
pub fn digits_then<'a>(s: &'a str, n: usize, tails: &[&str]) -> Option<(&'a str, &'a str)> {
    let k = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if k < n {
        return None;
    }
    let (d, rest) = s.split_at(k);
    (tails.is_empty() && rest.is_empty() || tails.contains(&rest)).then_some((d, rest))
}

/// `^\w{3,}` on the callsign.
pub fn call_ok(call: &str) -> bool {
    call.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .count()
        >= 3
}

/// Modes CW to AM, as bytes.
pub const CW_TO_AM: (u8, u8) = (Mode::Cw as u8, Mode::Am as u8);
