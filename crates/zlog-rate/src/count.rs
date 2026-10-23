//! What the rate window shows, computed from the log alone.
//!
//! Times are Delphi day numbers ([`DelphiTime`]) in the zone of the log, as zLog
//! keeps them. zLog's `UseContestPeriod` (the contest's start time and period) is
//! not known to a panel, so every function here behaves as zLog does without a
//! contest period: the origin is the first QSO. As in zLog, the last-10/last-100
//! rates count only the QSOs of the own transmitter (`TXNr`).

use zlog_core::{Band, DelphiTime, Log, Qso};

/// Hours a contest can last (`MAX_HOURS`).
pub const MAX_HOURS: usize = 48;

/// Number of real bands (1.9 MHz to 248 GHz).
pub const BANDS: usize = Band::HIGHEST as usize + 1;

const HOUR: f64 = 1.0 / 24.0;

/// Real bands in order.
pub fn bands() -> impl Iterator<Item = Band> {
    Band::range(Band::B1_9, Band::HIGHEST)
}

/// The time rounded to the second, in seconds since the Delphi epoch.
fn seconds(t: f64) -> i64 {
    (t * 86400.0).round() as i64
}

/// The time cut to the full hour (`Int(t) + EncodeTime(H, 0, 0, 0)`).
pub fn trunc_hour(t: f64) -> f64 {
    seconds(t).div_euclid(3600) as f64 * HOUR
}

/// Whole hours from `origin` to `t` (`DecodeTime` of the difference plus the days).
pub fn hours_between(t: f64, origin: f64) -> i64 {
    (seconds(t) - seconds(origin)).div_euclid(3600)
}

/// Hour of the day, 0 to 23 (`GetHour`).
pub fn hour_of(t: f64) -> u32 {
    DelphiTime(t).to_ymd_hms().3
}

/// The current time in the zone of a log (`CurrentTime`): UTC shifted by the
/// offset, `None` for UTC.
pub fn now(offset_minutes: Option<i32>) -> f64 {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    // 1970-01-01 is day 25569 in Delphi.
    25569.0 + (unix + offset_minutes.unwrap_or(0) as f64 * 60.0) / 86400.0
}

/// QSOs per hour of the last `n` scored QSOs of transmitter `tx`
/// (`TContestTarget.UpdateLastRate`, `aQSO.TX = mytx`): `n` divided by the hours
/// from the n-th last such QSO with points to `now`.
pub fn last_rate(qsos: &[Qso], tx: u8, n: usize, now: f64) -> Option<f64> {
    let q = qsos
        .iter()
        .rev()
        .filter(|q| q.points > 0 && q.tx == tx)
        .nth(n.checked_sub(1)?)?;
    let hours = (now - q.time.0) * 24.0;
    (hours > 0.0).then(|| n as f64 / hours)
}

/// Operating time in minutes (`TLog.GetOperatingTime`): the gaps between
/// consecutive QSOs shorter than an hour, summed, from the first QSO to `end`.
pub fn operating_minutes(qsos: &[Qso], end: f64) -> i64 {
    let minute = |t: f64| (t * 1440.0).trunc() / 1440.0;
    let Some(first) = qsos.first() else {
        return 0;
    };
    let start = minute(first.time.0);
    let mut total = 0;
    for (i, q) in qsos.iter().enumerate() {
        let t = minute(q.time.0);
        if t < start || t > end {
            continue;
        }
        if let Some(next) = qsos.get(i + 1) {
            let m = ((minute(next.time.0) - t) * 1440.0).round() as i64;
            // As zLog: negative gaps (an unsorted log) are added too.
            if m < 60 {
                total += m;
            }
        }
    }
    total
}

/// `H:MM` as the window shows the operating time.
pub fn hours_minutes(minutes: i64) -> String {
    format!("{}:{:02}", minutes / 60, minutes % 60)
}

/// A table of numbers per band and contest hour (`THourTarget` for each band).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hours(pub Vec<[u32; MAX_HOURS]>);

impl Default for Hours {
    fn default() -> Self {
        Hours(vec![[0; MAX_HOURS]; BANDS])
    }
}

impl Hours {
    pub fn get(&self, band: Band, hour: usize) -> u32 {
        self.0[band as usize][hour]
    }

    pub fn set(&mut self, band: Band, hour: usize, n: u32) {
        self.0[band as usize][hour] = n;
    }

    /// Sum over the bands of one hour.
    pub fn hour_total(&self, hour: usize) -> u32 {
        self.0.iter().map(|h| h[hour]).sum()
    }

    /// Sum over the hours of one band.
    pub fn band_total(&self, band: Band) -> u32 {
        self.0[band as usize].iter().sum()
    }

    pub fn total(&self) -> u32 {
        self.0.iter().flatten().sum()
    }

    /// Round every target above `n` up to a multiple of `n`
    /// (`TContestTarget.Adjust`, the "5/10 per step" buttons). As zLog, a value
    /// that already is a multiple still goes up one step.
    pub fn adjust(&mut self, n: u32) {
        for t in self.0.iter_mut().flatten() {
            if *t > n {
                *t = (*t / n + 1) * n;
            }
        }
    }

    /// The 48 hourly targets of one band, separated by commas, as kept in the
    /// settings (one key per band). Departure: zLog writes `zlog_target.txt`
    /// (`TContestTarget.SaveToFile`), one line per band led by the band's ordinal;
    /// this text is that line without the ordinal, and no such file is written.
    pub fn band_text(&self, band: Band) -> String {
        let v: Vec<String> = self.0[band as usize].iter().map(u32::to_string).collect();
        v.join(",")
    }

    /// Read one band's text back; missing or malformed hours are 0.
    pub fn set_band_text(&mut self, band: Band, text: &str) {
        let row = &mut self.0[band as usize];
        *row = [0; MAX_HOURS];
        for (h, v) in text.split(',').take(MAX_HOURS).enumerate() {
            row[h] = v.trim().parse().unwrap_or(0);
        }
    }
}

/// Origin of the hour table: the first QSO, cut to the hour.
pub fn origin(qsos: &[Qso]) -> Option<f64> {
    qsos.first().map(|q| trunc_hour(q.time.0))
}

/// Actual QSOs per band and contest hour from `origin`
/// (`TContestTarget.UpdateActualQSOs`); QSOs without points, invalid ones and
/// those before the origin or after hour 48 are left out.
pub fn actual(qsos: &[Qso], origin: f64) -> Hours {
    let mut out = Hours::default();
    for q in counted(qsos) {
        if q.time.0 < origin || q.band > Band::HIGHEST {
            continue;
        }
        let h = hours_between(q.time.0, origin);
        if (0..MAX_HOURS as i64).contains(&h) {
            out.0[q.band as usize][h as usize] += 1;
        }
    }
    out
}

fn counted(qsos: &[Qso]) -> impl Iterator<Item = &Qso> {
    qsos.iter().filter(|q| q.points > 0 && !q.invalid)
}

/// Targets taken from a past log (`TTargetEditor.buttonLoadZLOClick`): its QSOs
/// with points per band and hour. The origin is the past log's start time when
/// the open contest checks a period (`UseContestPeriod`, here `period_hours` > 0),
/// otherwise its first QSO, cut to the hour; the hours counted are the contest's
/// period (`MyContest.Period`), 24 when it has none, at most 48.
///
/// Departure: zLog takes the period from `MyContest.Period` even when
/// `UseContestPeriod` is off; a panel only knows `View::period_hours`, which is
/// 0 then, so such contests count 24 hours.
pub fn targets_from(log: &Log, period_hours: u32) -> Hours {
    let mut out = Hours::default();
    let qsos = &log.qsos;
    let start = log.start_time().0;
    let origin = if period_hours > 0 && start != 0.0 {
        trunc_hour(start)
    } else {
        match origin(qsos) {
            Some(o) => o,
            None => return out,
        }
    };
    let period = match period_hours as usize {
        0 => 24,
        p => p.min(MAX_HOURS),
    };
    for q in qsos
        .iter()
        .filter(|q| q.points > 0 && q.band <= Band::HIGHEST)
    {
        let h = hours_between(q.time.0, origin);
        if (0..period as i64).contains(&h) {
            out.0[q.band as usize][h as usize] += 1;
        }
    }
    out
}

/// Where the graph starts (`TQSORateStartPosition`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartPosition {
    FirstQso,
    CurrentTime,
    LastQso,
}

/// How the bars are split (`TQSORateStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphStyle {
    /// One bar per hour.
    Original,
    /// Stacked by band.
    ByBand,
    /// Stacked by HF low, HF high, VHF, UHF and SHF.
    ByRange,
}

/// The frequency range a band is stacked in for [`GraphStyle::ByRange`]
/// (`UpdateGraphByRange`): 0 HF(L), 1 HF(H), 2 VHF, 3 UHF, 4 SHF.
pub fn range_of(band: Band) -> usize {
    match band {
        b if b <= Band::B10 => 0,
        b if b <= Band::B28 => 1,
        Band::B50 | Band::B144 => 2,
        Band::B430 | Band::B1200 | Band::B2400 => 3,
        _ => 4,
    }
}

/// One bar of the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    /// Hour of the day the bar stands for.
    pub hour: u32,
    /// Actual QSOs per band (indexed by `Band as usize`).
    pub actual: Vec<u32>,
    /// Target QSOs per band.
    pub target: Vec<u32>,
    /// Actual and target QSOs up to the end of this hour.
    pub actual_total: u32,
    pub target_total: u32,
}

impl Column {
    pub fn actual_sum(&self) -> u32 {
        self.actual.iter().sum()
    }

    pub fn target_sum(&self) -> u32 {
        self.target.iter().sum()
    }
}

/// The bars of the graph (`TRateDialogEx.UpdateGraph`): `show` hours from the
/// start position, with the cumulative actual and target lines.
pub fn graph(
    qsos: &[Qso],
    targets: &Hours,
    now: f64,
    position: StartPosition,
    show: usize,
) -> Vec<Column> {
    let back = |t: f64| t - (show as f64 - 1.0) * HOUR;
    let (start, origin) = match (qsos.first(), qsos.last()) {
        (Some(first), Some(last)) => {
            let last = last.time.0;
            let start = match position {
                StartPosition::FirstQso => first.time.0,
                StartPosition::LastQso => back(last),
                StartPosition::CurrentTime if last < now => back(last),
                StartPosition::CurrentTime => back(now + (show as f64 / 2.0).floor() * HOUR - HOUR),
            };
            (start, first.time.0)
        }
        _ => (back(now), back(now)),
    };
    let (origin, mut start) = (trunc_hour(origin), trunc_hour(start));
    let offset = if start >= origin {
        let h = hours_between(start, origin) as usize;
        if h >= MAX_HOURS {
            0
        } else {
            h
        }
    } else {
        start = origin;
        0
    };
    let actual = actual(qsos, origin);
    let mut actual_total = counted(qsos)
        .filter(|q| q.time.0 >= origin && q.time.0 < start)
        .filter(|q| hours_between(q.time.0, origin) < MAX_HOURS as i64)
        .count() as u32;
    let mut target_total: u32 = (0..offset).map(|h| targets.hour_total(h)).sum();
    let mut out = Vec::new();
    for i in 0..show {
        let h = offset + i;
        if h >= MAX_HOURS {
            break;
        }
        let a: Vec<u32> = (0..BANDS).map(|b| actual.0[b][h]).collect();
        let t: Vec<u32> = (0..BANDS).map(|b| targets.0[b][h]).collect();
        actual_total += a.iter().sum::<u32>();
        target_total += t.iter().sum::<u32>();
        out.push(Column {
            hour: hour_of(start + i as f64 * HOUR),
            actual: a,
            target: t,
            actual_total,
            target_total,
        });
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use zlog_core::Mode;

    pub fn qso(h: u32, m: u32, band: Band, points: u8) -> Qso {
        let (d, h) = (1 + h / 24, h % 24);
        Qso {
            time: DelphiTime::from_ymd_hms(2026, 6, d, h, m, 0),
            band,
            mode: Mode::Cw,
            points,
            callsign: "JA1ZLO".into(),
            ..Qso::default()
        }
    }

    fn at(h: u32, m: u32) -> f64 {
        DelphiTime::from_ymd_hms(2026, 6, 1 + h / 24, h % 24, m, 0).0
    }

    #[test]
    fn last_ten_rate() {
        // Ten QSOs from 21:00 to 21:27; now is 21:30, so ten in half an hour.
        let qsos: Vec<Qso> = (0..10).map(|i| qso(21, i * 3, Band::B7, 1)).collect();
        let r = last_rate(&qsos, 0, 10, at(21, 30)).unwrap();
        assert!((r - 20.0).abs() < 1e-6);
        assert_eq!(last_rate(&qsos, 0, 100, at(21, 30)), None);
        // QSOs without points do not count.
        let mut q = qsos.clone();
        q[0].points = 0;
        assert_eq!(last_rate(&q, 0, 10, at(21, 30)), None);
        // Nor those of another transmitter.
        let mut q = qsos.clone();
        q[0].tx = 1;
        assert_eq!(last_rate(&q, 0, 10, at(21, 30)), None);
        assert_eq!(last_rate(&qsos, 1, 10, at(21, 30)), None);
    }

    #[test]
    fn operating_time_skips_long_breaks() {
        let qsos = vec![
            qso(21, 0, Band::B7, 1),
            qso(21, 10, Band::B7, 1),
            qso(23, 0, Band::B7, 1),
            qso(23, 5, Band::B7, 1),
        ];
        assert_eq!(operating_minutes(&qsos, at(30, 0)), 15);
        assert_eq!(hours_minutes(75), "1:15");
    }

    #[test]
    fn hours_from_the_origin() {
        let qsos = vec![
            qso(21, 30, Band::B7, 1),
            qso(21, 59, Band::B7, 1),
            qso(22, 0, Band::B14, 1),
            qso(22, 1, Band::B14, 0),
            qso(47 + 21, 0, Band::B14, 1),
            qso(48 + 21, 0, Band::B14, 1),
        ];
        let o = origin(&qsos).unwrap();
        assert_eq!(o, at(21, 0));
        let a = actual(&qsos, o);
        assert_eq!(a.get(Band::B7, 0), 2);
        assert_eq!(a.get(Band::B14, 1), 1);
        assert_eq!(a.get(Band::B14, 47), 1);
        assert_eq!(a.total(), 4);
    }

    #[test]
    fn targets_text_and_adjust() {
        let mut t = Hours::default();
        t.set(Band::B7, 0, 12);
        t.set(Band::B7, 1, 3);
        t.set(Band::B7, 2, 10);
        let text = t.band_text(Band::B7);
        assert!(text.starts_with("12,3,10,0,"));
        assert_eq!(text.split(',').count(), MAX_HOURS);
        let mut u = Hours::default();
        u.set_band_text(Band::B7, &text);
        assert_eq!(u, t);
        t.adjust(5);
        assert_eq!(t.0[Band::B7 as usize][..3], [15, 3, 15]);
    }

    #[test]
    fn targets_from_a_past_log() {
        let qsos = vec![
            qso(9, 10, Band::B7, 1),
            qso(10, 0, Band::B7, 1),
            qso(40, 0, Band::B7, 1),
        ];
        let log = Log {
            qsos,
            ..Log::default()
        };
        let t = targets_from(&log, 0);
        assert_eq!(t.get(Band::B7, 0), 1);
        assert_eq!(t.get(Band::B7, 1), 1);
        assert_eq!(t.total(), 2);
    }

    fn past_log(qsos: Vec<Qso>, start: Option<(u32, u32)>) -> Log {
        let mut log = Log {
            qsos,
            ..Log::default()
        };
        if let Some((h, m)) = start {
            log.set_start_time(qso(h, m, Band::B7, 0).time);
        }
        log
    }

    #[test]
    fn targets_use_the_contest_period() {
        // A 48 h contest counts the QSO 40 h after the first one (zLog
        // `MyContest.Period`); before the fix every log counted 24 h.
        let log = past_log(
            vec![qso(9, 10, Band::B7, 1), qso(40, 0, Band::B7, 1)],
            Some((9, 0)),
        );
        let t = targets_from(&log, 48);
        assert_eq!(t.get(Band::B7, 0), 1);
        assert_eq!(t.get(Band::B7, 31), 1);
        assert_eq!(t.total(), 2);
        // Periods above 48 h are capped.
        assert_eq!(targets_from(&log, 100), t);
        // A 6 h period drops the late QSO.
        assert_eq!(targets_from(&log, 6).total(), 1);
    }

    #[test]
    fn targets_start_at_the_past_log_start_time() {
        // The start time (cut to the hour) is the origin, not the first QSO.
        let log = past_log(
            vec![qso(11, 5, Band::B7, 1), qso(12, 59, Band::B14, 1)],
            Some((9, 30)),
        );
        let t = targets_from(&log, 24);
        assert_eq!(t.get(Band::B7, 2), 1);
        assert_eq!(t.get(Band::B14, 3), 1);
        // QSOs before the start are not counted.
        let early = past_log(vec![qso(8, 0, Band::B7, 1)], Some((9, 0)));
        assert_eq!(targets_from(&early, 24).total(), 0);
        // Without a contest period the first QSO is the origin again.
        assert_eq!(targets_from(&log, 0).get(Band::B7, 0), 1);
        // A log whose start time was never set falls back to its first QSO.
        let unset = past_log(vec![qso(11, 5, Band::B7, 1)], None);
        assert_eq!(targets_from(&unset, 24).get(Band::B7, 0), 1);
    }

    #[test]
    fn targets_from_edge_cases() {
        assert_eq!(targets_from(&Log::default(), 24), Hours::default());
        assert_eq!(targets_from(&Log::default(), 0), Hours::default());
        // QSOs without points are left out.
        let log = past_log(vec![qso(9, 0, Band::B7, 0)], Some((9, 0)));
        assert_eq!(targets_from(&log, 24).total(), 0);
        // The last hour of the period counts, the one after it does not.
        let log = past_log(
            vec![qso(32, 59, Band::B7, 1), qso(33, 0, Band::B7, 1)],
            Some((9, 0)),
        );
        let t = targets_from(&log, 24);
        assert_eq!(t.get(Band::B7, 23), 1);
        assert_eq!(t.total(), 1);
    }

    #[test]
    fn graph_from_the_first_qso() {
        let qsos = vec![
            qso(21, 0, Band::B7, 1),
            qso(21, 30, Band::B14, 1),
            qso(23, 10, Band::B7, 1),
        ];
        let mut targets = Hours::default();
        targets.set(Band::B7, 0, 5);
        let g = graph(&qsos, &targets, at(23, 30), StartPosition::FirstQso, 3);
        assert_eq!(g.len(), 3);
        assert_eq!(g.iter().map(|c| c.hour).collect::<Vec<_>>(), [21, 22, 23]);
        assert_eq!(g[0].actual_sum(), 2);
        assert_eq!(g[0].target_sum(), 5);
        assert_eq!(g[2].actual_total, 3);
        assert_eq!(g[2].target_total, 5);
    }

    #[test]
    fn graph_ending_at_the_last_qso() {
        let qsos: Vec<Qso> = (0..6).map(|h| qso(20 + h, 0, Band::B7, 1)).collect();
        let g = graph(
            &qsos,
            &Hours::default(),
            at(30, 0),
            StartPosition::LastQso,
            3,
        );
        assert_eq!(g.iter().map(|c| c.hour).collect::<Vec<_>>(), [23, 0, 1]);
        // The three hours before the graph count in the cumulative line.
        assert_eq!(g[0].actual_total, 4);
        let c = graph(
            &qsos,
            &Hours::default(),
            at(30, 0),
            StartPosition::CurrentTime,
            3,
        );
        assert_eq!(c, g);
    }
}
