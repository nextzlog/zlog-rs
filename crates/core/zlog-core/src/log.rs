use std::collections::HashSet;
use std::path::Path;

use crate::qso::{DelphiTime, QslState, Qso};
use crate::types::{Band, Mode};
use crate::zlo::{self, Format, ZloError};

pub const USE_UTC: u16 = 32767;

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Zlo(#[from] ZloError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DupeRule {
    pub accept_different_mode: bool,
    pub all_phone: bool,
    pub single_10g: bool,
}

impl Default for DupeRule {
    fn default() -> Self {
        DupeRule {
            accept_different_mode: false,
            all_phone: true,
            single_10g: false,
        }
    }
}

fn same_mode(a: Mode, b: Mode, all_phone: bool) -> bool {
    use Mode::*;
    match a {
        Cw | Rtty | Other => a == b,
        Ssb | Fm | Am | Dv if all_phone => matches!(b, Ssb | Fm | Am | Dv),
        Ssb | Fm | Am | Dv => a == b,
        Ft4 | Ft8 => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QsyRule {
    pub countdown_minutes: Option<u32>,
    pub max_qsy_per_hour: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Log {
    pub header: Qso,
    pub qsos: Vec<Qso>,
    pub dupe_rule: DupeRule,
    pub qsy_rule: QsyRule,
}

impl Default for Log {
    fn default() -> Self {
        Log {
            header: Qso {
                rst_sent: 0,
                rst_rcvd: 100,
                ..Qso::default()
            },
            qsos: Vec::new(),
            dupe_rule: DupeRule::default(),
            qsy_rule: QsyRule::default(),
        }
    }
}

impl Log {
    pub fn from_bytes(bytes: &[u8]) -> Result<(Self, Format), LogError> {
        Self::from_bytes_counted(bytes).map(|(log, format, _)| (log, format))
    }

    pub fn from_bytes_counted(bytes: &[u8]) -> Result<(Self, Format, usize), LogError> {
        let read = zlo::read_records(bytes)?;
        let mut records = read.records;
        let qsos = records.split_off(1);
        let file = records.pop().expect("at least one record");
        let mut log = Log {
            qsos,
            ..Log::default()
        };
        log.header.rst_sent = file.rst_sent;
        log.header.reserve2 = file.reserve2;
        log.header.reserve3 = file.reserve3;
        Ok((log, read.format, read.bad_bytes))
    }

    pub fn has_old_other(&self) -> bool {
        self.qsos.iter().any(|q| q.mode == Mode::Ft4)
    }

    pub fn convert_old_other(&mut self) {
        for q in &mut self.qsos {
            if q.mode == Mode::Ft4 {
                q.mode = Mode::Other;
            }
        }
    }

    pub fn to_bytes(&self, format: Format) -> Vec<u8> {
        self.to_bytes_with(format, QslState::None)
    }

    pub fn to_bytes_with(&self, format: Format, qsl_default: QslState) -> Vec<u8> {
        let mut records = Vec::with_capacity(self.qsos.len() + 1);
        records.push(self.header.clone());
        records.extend(self.qsos.iter().cloned());
        zlo::write_records_with(&records, format, qsl_default)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<(Self, Format), LogError> {
        Self::from_bytes(&std::fs::read(path)?)
    }

    pub fn save(&self, path: impl AsRef<Path>, format: Format) -> Result<(), LogError> {
        self.save_with(path, format, QslState::None)
    }

    pub fn save_with(
        &self,
        path: impl AsRef<Path>,
        format: Format,
        qsl_default: QslState,
    ) -> Result<(), LogError> {
        crate::file::write_atomic(path.as_ref(), &self.to_bytes_with(format, qsl_default))?;
        Ok(())
    }

    pub fn utc_offset_minutes(&self) -> Option<i32> {
        match self.header.rst_sent {
            USE_UTC => None,
            n => Some(-(n as i16 as i32)),
        }
    }

    pub fn score_coeff(&self) -> f64 {
        self.header.rst_rcvd as f64 / 100.0
    }

    pub fn set_score_coeff(&mut self, coeff: f64) {
        self.header.rst_rcvd = (coeff * 100.0).round().clamp(0.0, u16::MAX as f64) as u16;
    }

    pub fn start_time(&self) -> DelphiTime {
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&self.header.reserve2.to_le_bytes());
        b[4..].copy_from_slice(&self.header.reserve3.to_le_bytes());
        DelphiTime(f64::from_le_bytes(b))
    }

    pub fn set_start_time(&mut self, t: DelphiTime) {
        let b = t.0.to_le_bytes();
        self.header.reserve2 = i32::from_le_bytes(b[..4].try_into().unwrap());
        self.header.reserve3 = i32::from_le_bytes(b[4..].try_into().unwrap());
    }

    pub fn out_of_period(&self, q: &Qso, hours: u32) -> bool {
        if hours == 0 {
            return false;
        }
        self.outside(q, |start| start + hours as i64 * 3600)
    }

    pub fn out_of_open_period(&self, q: &Qso, now: DelphiTime) -> bool {
        self.outside(q, |start| start.max(secs(now)))
    }

    fn outside(&self, q: &Qso, end: impl Fn(i64) -> i64) -> bool {
        if self.start_time().0 == 0.0 {
            return false;
        }
        let (t, start) = (secs(q.time), secs(self.start_time()));
        t < start || t > end(start)
    }

    fn dupe_key(&self, q: &Qso) -> String {
        let mut key = format!("{}\t{}", q.core_call(), q.band.label());
        if self.dupe_rule.accept_different_mode {
            key.push('\t');
            key.push_str(if self.dupe_rule.all_phone {
                q.mode_group()
            } else {
                q.mode.label()
            });
        }
        key
    }

    pub fn dupe_flags(&self) -> Vec<bool> {
        let mut seen = HashSet::new();
        self.qsos
            .iter()
            .map(|q| {
                if q.invalid {
                    q.dupe
                } else {
                    !seen.insert(self.dupe_key(q))
                }
            })
            .collect()
    }

    pub fn set_dupe_flags(&mut self) {
        let flags = self.dupe_flags();
        let qsy = self.qsy_violations(self.qsy_rule);
        for ((q, d), v) in self.qsos.iter_mut().zip(flags).zip(qsy) {
            if q.invalid {
                continue;
            }
            q.dupe = d;
            if d {
                q.points = 0;
            } else if q.memo.contains(zlo::MEMO_DUPE) {
                q.memo = q.memo.replace(zlo::MEMO_DUPE, "").trim().to_string();
            }
            q.qsy_violation = v;
        }
    }

    pub fn qsy_violations(&self, rule: QsyRule) -> Vec<bool> {
        let all: Vec<&Qso> = self.qsos.iter().collect();
        qsy_flags(&all, rule)
    }

    pub fn qsy_violation_of(&self, q: &Qso, rule: QsyRule) -> bool {
        let mut all: Vec<&Qso> = self.qsos.iter().collect();
        all.push(q);
        qsy_flags(&all, rule).pop().unwrap_or(false)
    }

    pub fn qsy_count_at(&self, tx: u8, now: DelphiTime) -> usize {
        let Some(last) = self.qsos.iter().rev().find(|q| q.tx == tx) else {
            return 0;
        };
        let probe = Qso {
            time: now,
            ..last.clone()
        };
        let mut all: Vec<&Qso> = self.qsos.iter().collect();
        all.push(&probe);
        qsy_count(&all, all.len() - 1)
    }

    pub fn find_dupe(&self, q: &Qso) -> Option<usize> {
        let rule = self.dupe_rule;
        let band = |b: Band| {
            if rule.single_10g && b == Band::B10_4G {
                Band::B10G
            } else {
                b
            }
        };
        let call = q.core_call();
        self.qsos.iter().position(|p| {
            !p.invalid
                && band(p.band) == band(q.band)
                && p.core_call() == call
                && (!rule.accept_different_mode || same_mode(q.mode, p.mode, rule.all_phone))
        })
    }
}

fn secs(t: DelphiTime) -> i64 {
    (t.0 * 86400.0).round() as i64
}

fn hour(t: DelphiTime) -> i64 {
    secs(t).div_euclid(3600)
}

fn qsy_flags(qsos: &[&Qso], rule: QsyRule) -> Vec<bool> {
    let mut out: Vec<bool> = Vec::with_capacity(qsos.len());
    let mut v: Option<usize> = None;
    for (i, q) in qsos.iter().enumerate() {
        if q.invalid {
            out.push(q.qsy_violation);
            continue;
        }
        let mut violation = false;
        if let Some(minutes) = rule.countdown_minutes {
            match v {
                None => v = Some(i),
                Some(j) => {
                    let p = qsos[j];
                    let short = (secs(q.time) - secs(p.time)).abs() < minutes as i64 * 60;
                    if p.tx == q.tx && p.band != q.band {
                        if short {
                            violation = true;
                        } else {
                            v = Some(i);
                        }
                    } else if short {
                        violation = out[j];
                    }
                }
            }
        }
        if let Some(max) = rule.max_qsy_per_hour {
            if qsy_count(qsos, i) > max as usize {
                violation = true;
            }
        }
        out.push(violation);
    }
    out
}

fn qsy_count(qsos: &[&Qso], i: usize) -> usize {
    let (base, tx) = (hour(qsos[i].time), qsos[i].tx);
    let mut start = i;
    for j in (0..=i).rev() {
        if qsos[j].tx != tx {
            continue;
        }
        start = j;
        if hour(qsos[j].time) != base {
            break;
        }
    }
    let mut prev = qsos[start];
    let mut count = 0;
    for &q in &qsos[start + 1..] {
        if q.tx != tx {
            continue;
        }
        if hour(q.time) != base {
            break;
        }
        if q.band != prev.band {
            count += 1;
        }
        prev = q;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Power;

    fn at(call: &str, band: Band, h: u32, m: u32) -> Qso {
        Qso {
            time: DelphiTime::from_ymd_hms(2026, 6, 6, h, m, 0),
            ..qso(call, band, Mode::Cw)
        }
    }

    fn countdown(minutes: u32) -> QsyRule {
        QsyRule {
            countdown_minutes: Some(minutes),
            ..QsyRule::default()
        }
    }

    #[test]
    fn countdown_flags_a_band_change_too_soon() {
        let mut log = Log {
            qsos: vec![
                at("JA1AAA", Band::B7, 21, 0),
                at("JA1BBB", Band::B14, 21, 5),
                at("JA1CCC", Band::B14, 21, 8),
                at("JA1DDD", Band::B14, 21, 12),
                at("JA1EEE", Band::B21, 21, 25),
            ],
            qsy_rule: countdown(10),
            ..Log::default()
        };
        log.set_dupe_flags();
        let flags: Vec<_> = log.qsos.iter().map(|q| q.qsy_violation).collect();
        assert_eq!(flags, vec![false, true, true, false, false]);
    }

    #[test]
    fn countdown_boundary_is_exclusive() {
        let log = Log {
            qsos: vec![at("A", Band::B7, 21, 0), at("B", Band::B14, 21, 10)],
            ..Log::default()
        };
        assert_eq!(log.qsy_violations(countdown(10)), vec![false, false]);
        assert_eq!(log.qsy_violations(countdown(11)), vec![false, true]);
        assert_eq!(log.qsy_violations(countdown(0)), vec![false, false]);
    }

    #[test]
    fn countdown_is_per_tx() {
        let mut other = at("B", Band::B14, 21, 1);
        other.tx = 1;
        let log = Log {
            qsos: vec![at("A", Band::B7, 21, 0), other],
            ..Log::default()
        };
        assert_eq!(log.qsy_violations(countdown(10)), vec![false, false]);
    }

    #[test]
    fn rules_off_clear_violations_but_invalid_keep_them() {
        let mut log = Log {
            qsos: vec![at("A", Band::B7, 21, 0), at("B", Band::B14, 21, 1)],
            ..Log::default()
        };
        log.qsos[0].qsy_violation = true;
        log.qsos[1].qsy_violation = true;
        log.qsos[1].invalid = true;
        log.set_dupe_flags();
        assert!(!log.qsos[0].qsy_violation);
        assert!(log.qsos[1].qsy_violation);
        assert!(Log::default().qsy_violations(countdown(10)).is_empty());
    }

    #[test]
    fn qsy_count_per_clock_hour() {
        let rule = QsyRule {
            max_qsy_per_hour: Some(2),
            ..QsyRule::default()
        };
        let log = Log {
            qsos: vec![
                at("A", Band::B7, 20, 50),
                at("B", Band::B14, 21, 0),
                at("C", Band::B7, 21, 10),
                at("D", Band::B14, 21, 20),
                at("E", Band::B7, 22, 0),
            ],
            ..Log::default()
        };
        assert_eq!(
            log.qsy_violations(rule),
            vec![false, true, true, true, false]
        );
        let loose = QsyRule {
            max_qsy_per_hour: Some(3),
            ..rule
        };
        assert_eq!(log.qsy_violations(loose), vec![false; 5]);
    }

    #[test]
    fn qsy_count_in_the_hour_of_now() {
        let mut log = Log {
            qsos: vec![
                at("A", Band::B7, 20, 50),
                at("B", Band::B14, 21, 0),
                at("C", Band::B7, 21, 10),
            ],
            ..Log::default()
        };
        let now = |h, m| DelphiTime::from_ymd_hms(2026, 6, 6, h, m, 0);
        assert_eq!(log.qsy_count_at(0, now(21, 30)), 2);
        assert_eq!(log.qsy_count_at(0, now(22, 0)), 0);
        assert_eq!(log.qsy_count_at(1, now(21, 30)), 0);
        log.qsos[2].tx = 1;
        assert_eq!(log.qsy_count_at(0, now(21, 30)), 1);
        assert_eq!(Log::default().qsy_count_at(0, now(21, 30)), 0);
    }

    #[test]
    fn logging_checks_the_new_qso() {
        let log = Log {
            qsos: vec![at("A", Band::B7, 21, 0)],
            ..Log::default()
        };
        let rule = countdown(10);
        assert!(log.qsy_violation_of(&at("B", Band::B14, 21, 5), rule));
        assert!(!log.qsy_violation_of(&at("B", Band::B7, 21, 5), rule));
        assert!(!log.qsy_violation_of(&at("B", Band::B14, 21, 5), QsyRule::default()));
        assert!(!Log::default().qsy_violation_of(&at("B", Band::B14, 21, 5), rule));
    }

    #[test]
    fn dupe_flags_zero_points_and_strip_the_dupe_memo() {
        let mut log = Log {
            qsos: vec![
                qso("JA1ZLO", Band::B7, Mode::Cw),
                qso("JA1ZLO", Band::B7, Mode::Cw),
            ],
            ..Log::default()
        };
        log.qsos[0].memo = "-DUPE- QSB".into();
        log.qsos[0].points = 1;
        log.qsos[1].points = 1;
        log.qsos[1].memo = "-DUPE-".into();
        log.set_dupe_flags();
        assert_eq!((log.qsos[0].points, log.qsos[0].memo.as_str()), (1, "QSB"));
        assert_eq!(
            (log.qsos[1].points, log.qsos[1].memo.as_str()),
            (0, "-DUPE-")
        );
        let mut log2 = log.clone();
        log2.qsos[1].invalid = true;
        log2.qsos[1].points = 3;
        log2.set_dupe_flags();
        assert_eq!(log2.qsos[1].points, 3);
    }

    #[test]
    fn open_period_ends_now() {
        let mut log = Log::default();
        let start = DelphiTime::from_ymd_hms(2026, 4, 25, 21, 0, 0);
        let now = DelphiTime::from_ymd_hms(2026, 4, 26, 3, 0, 0);
        let q = |h, d| Qso {
            time: DelphiTime::from_ymd_hms(2026, 4, d, h, 0, 0),
            ..Qso::default()
        };
        assert!(!log.out_of_open_period(&q(20, 25), now), "no start time");
        log.set_start_time(start);
        assert!(log.out_of_open_period(&q(20, 25), now));
        assert!(!log.out_of_open_period(&q(21, 25), now));
        assert!(!log.out_of_open_period(&q(3, 26), now));
        assert!(log.out_of_open_period(&q(4, 26), now));
        let early = DelphiTime::from_ymd_hms(2026, 4, 25, 12, 0, 0);
        assert!(!log.out_of_open_period(&q(21, 25), early));
        assert!(log.out_of_open_period(&q(22, 25), early));
    }

    fn qso(call: &str, band: Band, mode: Mode) -> Qso {
        Qso {
            callsign: call.into(),
            band,
            mode,
            ..Qso::default()
        }
    }

    #[test]
    fn dupe_by_band_only_by_default() {
        let mut log = Log {
            qsos: vec![
                qso("JA1ZLO", Band::B7, Mode::Cw),
                qso("JA1ZLO/1", Band::B7, Mode::Ssb),
                qso("JA1ZLO", Band::B14, Mode::Cw),
            ],
            ..Log::default()
        };
        log.set_dupe_flags();
        let flags: Vec<_> = log.qsos.iter().map(|q| q.dupe).collect();
        assert_eq!(flags, vec![false, true, false]);
    }

    #[test]
    fn different_mode_allowed() {
        let mut log = Log::default();
        log.dupe_rule.accept_different_mode = true;
        log.qsos = vec![
            qso("JA1ZLO", Band::B7, Mode::Cw),
            qso("JA1ZLO", Band::B7, Mode::Ssb),
            qso("JA1ZLO", Band::B7, Mode::Fm),
        ];
        log.set_dupe_flags();
        let flags: Vec<_> = log.qsos.iter().map(|q| q.dupe).collect();
        assert_eq!(flags, vec![false, false, true]);
    }

    #[test]
    fn a_new_log_has_zlogs_header() {
        let log = Log::default();
        assert_eq!(log.score_coeff(), 1.0);
        assert_eq!(log.utc_offset_minutes(), Some(0));
    }

    #[test]
    fn mode_groups_follow_zlog() {
        let mut log = Log::default();
        log.dupe_rule.accept_different_mode = true;
        log.qsos = [Mode::Ssb, Mode::Dv, Mode::Ft4, Mode::Ft8, Mode::Fm]
            .into_iter()
            .map(|m| qso("JA1ZLO", Band::B7, m))
            .collect();
        assert_eq!(log.dupe_flags(), vec![false, false, false, true, true]);
    }

    #[test]
    fn contest_period() {
        let mut log = Log::default();
        let start = DelphiTime::from_ymd_hms(2026, 4, 25, 21, 0, 0);
        log.set_start_time(start);
        assert_eq!(log.start_time(), start);
        let at = |h, m| Qso {
            time: DelphiTime::from_ymd_hms(2026, 4, 26, h, m, 0),
            ..Qso::default()
        };
        let before = Qso {
            time: DelphiTime::from_ymd_hms(2026, 4, 25, 20, 59, 0),
            ..Qso::default()
        };
        assert!(log.out_of_period(&before, 24));
        assert!(!log.out_of_period(&at(21, 0), 24));
        assert!(log.out_of_period(&at(21, 1), 24));
        assert!(!log.out_of_period(&before, 0));
        let (back, _) = Log::from_bytes(&log.to_bytes(Format::Zlox)).unwrap();
        assert_eq!(back.start_time(), start);
    }

    #[test]
    fn score_coeff_rounds_to_hundredths() {
        let mut log = Log::default();
        for (c, h) in [
            (1.15, 115),
            (0.57, 57),
            (2.01, 201),
            (1.13, 113),
            (0.29, 29),
        ] {
            log.set_score_coeff(c);
            assert_eq!(log.header.rst_rcvd, h, "{c}");
        }
        log.set_score_coeff(1e9);
        assert_eq!(log.header.rst_rcvd, u16::MAX);
        log.set_score_coeff(-1.0);
        assert_eq!(log.header.rst_rcvd, 0);
    }

    #[test]
    fn save_replaces_the_file_atomically() {
        let dir = std::env::temp_dir().join(format!("zlog-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.zlox");
        std::fs::write(&path, b"old").unwrap();
        let old = std::env::temp_dir().join(format!("zlog-save-old-{}", std::process::id()));
        let _ = std::fs::remove_file(&old);
        std::fs::hard_link(&path, &old).unwrap();
        let log = Log::default();
        log.save_with(&path, Format::Zlox, QslState::default())
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), log.to_bytes(Format::Zlox));
        assert_eq!(std::fs::read(&old).unwrap(), b"old");
        std::fs::remove_file(&old).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        assert!(log
            .save_with(&sub, Format::Zlox, QslState::default())
            .is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn header_fields() {
        let mut log = Log::default();
        log.header.rst_sent = (-540i16) as u16;
        assert_eq!(log.utc_offset_minutes(), Some(540));
        log.set_score_coeff(1.5);
        assert_eq!(log.score_coeff(), 1.5);
    }

    #[test]
    fn only_offset_and_start_time_are_read_from_the_header() {
        let mut log = Log::default();
        log.header.rst_sent = (-540i16) as u16;
        log.set_score_coeff(1.5);
        log.header.callsign = "JA1ZLO".into();
        log.header.memo = "x".into();
        log.header.power = Power::P1000;
        let start = DelphiTime::from_ymd_hms(2026, 4, 25, 21, 0, 0);
        log.set_start_time(start);
        for format in [Format::Zlo, Format::Zlox] {
            let (back, _) = Log::from_bytes(&log.to_bytes(format)).unwrap();
            assert_eq!(back.utc_offset_minutes(), Some(540));
            assert_eq!(back.start_time(), start);
            assert_eq!(back.score_coeff(), 1.0);
            let expected = Qso {
                rst_sent: log.header.rst_sent,
                reserve2: log.header.reserve2,
                reserve3: log.header.reserve3,
                ..Log::default().header
            };
            assert_eq!(back.header, expected);
        }
        let mut utc = Log::default();
        utc.header.rst_sent = USE_UTC;
        let (back, _) = Log::from_bytes(&utc.to_bytes(Format::Zlo)).unwrap();
        assert_eq!(back.utc_offset_minutes(), None);
    }

    #[test]
    fn bad_bytes_are_counted() {
        let log = Log {
            qsos: vec![qso("JA1ZLO", Band::B7, Mode::Cw)],
            ..Log::default()
        };
        let mut bytes = log.to_bytes(Format::Zlo);
        bytes[crate::zlo::ZLO_RECORD + 92] = 9;
        let (back, _, bad) = Log::from_bytes_counted(&bytes).unwrap();
        assert_eq!(bad, 1);
        assert_eq!(back.qsos[0].mode, Mode::Other);
        let (_, _, bad) = Log::from_bytes_counted(&log.to_bytes(Format::Zlo)).unwrap();
        assert_eq!(bad, 0);
        assert!(Log::from_bytes(&[]).is_err());
    }

    #[test]
    fn old_other_is_converted() {
        let mut log = Log {
            qsos: vec![
                qso("JA1ZLO", Band::B7, Mode::Cw),
                qso("JA1YAA", Band::B7, Mode::Ft4),
            ],
            ..Log::default()
        };
        let bytes = log.to_bytes(Format::Zlo);
        assert_eq!(bytes[2 * crate::zlo::ZLO_RECORD + 92], 5);
        assert!(log.has_old_other());
        log.convert_old_other();
        assert!(!log.has_old_other());
        let modes: Vec<_> = log.qsos.iter().map(|q| q.mode).collect();
        assert_eq!(modes, vec![Mode::Cw, Mode::Other]);
        assert!(!Log::default().has_old_other());
    }

    #[test]
    fn invalid_qsos_keep_their_dupe_flag() {
        let mut log = Log {
            qsos: vec![
                qso("JA1ZLO", Band::B7, Mode::Cw),
                qso("JA1ZLO", Band::B7, Mode::Cw),
                qso("JA1ZLO", Band::B7, Mode::Cw),
            ],
            ..Log::default()
        };
        log.qsos[0].invalid = true;
        log.qsos[0].dupe = true;
        log.qsos[2].invalid = true;
        log.set_dupe_flags();
        let flags: Vec<_> = log.qsos.iter().map(|q| q.dupe).collect();
        assert_eq!(flags, vec![true, false, false]);
        assert!(Log::default().dupe_flags().is_empty());
    }

    fn find(rule: DupeRule, old: (Band, Mode), new: (Band, Mode)) -> Option<usize> {
        let log = Log {
            qsos: vec![qso("JA1ZLO", old.0, old.1)],
            dupe_rule: rule,
            ..Log::default()
        };
        log.find_dupe(&qso("JA1ZLO/1", new.0, new.1))
    }

    #[test]
    fn find_dupe_follows_same_mode() {
        let by_mode = DupeRule {
            accept_different_mode: true,
            ..DupeRule::default()
        };
        let b = Band::B7;
        assert_eq!(find(by_mode, (b, Mode::Ssb), (b, Mode::Dv)), Some(0));
        assert_eq!(find(by_mode, (b, Mode::Dv), (b, Mode::Fm)), Some(0));
        let exact = DupeRule {
            all_phone: false,
            ..by_mode
        };
        assert_eq!(find(exact, (b, Mode::Ssb), (b, Mode::Dv)), None);
        assert_eq!(find(exact, (b, Mode::Dv), (b, Mode::Dv)), Some(0));
        assert_eq!(find(by_mode, (b, Mode::Ft8), (b, Mode::Ft8)), None);
        assert_eq!(find(by_mode, (b, Mode::Ft4), (b, Mode::Ft8)), None);
        assert_eq!(find(by_mode, (b, Mode::Cw), (b, Mode::Ssb)), None);
        assert_eq!(find(by_mode, (b, Mode::Other), (b, Mode::Other)), Some(0));
        assert_eq!(find(by_mode, (b, Mode::Rtty), (b, Mode::Rtty)), Some(0));
        let band_only = DupeRule::default();
        assert_eq!(find(band_only, (b, Mode::Ft8), (b, Mode::Cw)), Some(0));
        assert_eq!(find(band_only, (b, Mode::Cw), (Band::B14, Mode::Cw)), None);
    }

    #[test]
    fn find_dupe_single_10g() {
        let single = DupeRule {
            single_10g: true,
            ..DupeRule::default()
        };
        let cw = Mode::Cw;
        assert_eq!(find(single, (Band::B10G, cw), (Band::B10_4G, cw)), Some(0));
        assert_eq!(find(single, (Band::B10_4G, cw), (Band::B10G, cw)), Some(0));
        assert_eq!(
            find(DupeRule::default(), (Band::B10G, cw), (Band::B10_4G, cw)),
            None
        );
        assert_eq!(find(single, (Band::B5600, cw), (Band::B10G, cw)), None);
    }

    #[test]
    fn find_dupe_skips_invalid_and_empty() {
        let mut log = Log {
            qsos: vec![
                qso("JA1ZLO", Band::B7, Mode::Cw),
                qso("JA1ZLO", Band::B7, Mode::Cw),
            ],
            ..Log::default()
        };
        log.qsos[0].invalid = true;
        assert_eq!(log.find_dupe(&qso("JA1ZLO", Band::B7, Mode::Cw)), Some(1));
        assert_eq!(
            Log::default().find_dupe(&qso("JA1ZLO", Band::B7, Mode::Cw)),
            None
        );
    }
}
