//! The log (`TLog`): header, contacts, dupe check, file load and save.

use std::collections::HashSet;
use std::path::Path;

use crate::qso::{DelphiTime, Qso};
use crate::zlo::{self, Format, ZloError};

/// `RSTSent` value of the header meaning "times are recorded in UTC" (`_USEUTC`).
pub const USE_UTC: u16 = 32767;

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Zlo(#[from] ZloError),
}

/// Dupe rule (`AcceptDifferentMode`, `AllPhone`); zLog defaults to band only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DupeRule {
    pub accept_different_mode: bool,
    pub all_phone: bool,
}

impl Default for DupeRule {
    fn default() -> Self {
        DupeRule {
            accept_different_mode: false,
            all_phone: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Log {
    /// Record 0 of the file; zLog keeps settings in some of its fields.
    pub header: Qso,
    pub qsos: Vec<Qso>,
    pub dupe_rule: DupeRule,
}

impl Default for Log {
    /// An empty log with zLog's new-log header (`TLog.Create`): offset 0, coefficient 1.00.
    fn default() -> Self {
        Log {
            header: Qso {
                rst_sent: 0,
                rst_rcvd: 100,
                ..Qso::default()
            },
            qsos: Vec::new(),
            dupe_rule: DupeRule::default(),
        }
    }
}

impl Log {
    pub fn from_bytes(bytes: &[u8]) -> Result<(Self, Format), LogError> {
        let (format, mut records) = zlo::read_records(bytes)?;
        let qsos = records.split_off(1);
        let header = records.pop().expect("at least one record");
        Ok((
            Log {
                header,
                qsos,
                ..Log::default()
            },
            format,
        ))
    }

    pub fn to_bytes(&self, format: Format) -> Vec<u8> {
        let mut records = Vec::with_capacity(self.qsos.len() + 1);
        records.push(self.header.clone());
        records.extend(self.qsos.iter().cloned());
        zlo::write_records(&records, format)
    }

    /// Load a ZLO or ZLOX file; the format is detected from the content.
    pub fn load(path: impl AsRef<Path>) -> Result<(Self, Format), LogError> {
        Self::from_bytes(&std::fs::read(path)?)
    }

    pub fn save(&self, path: impl AsRef<Path>, format: Format) -> Result<(), LogError> {
        Ok(std::fs::write(path, self.to_bytes(format))?)
    }

    /// Offset of the recorded times from UTC in minutes (JST is 540), or `None` for UTC.
    ///
    /// zLog stores the negated offset as a signed 16-bit value.
    pub fn utc_offset_minutes(&self) -> Option<i32> {
        match self.header.rst_sent {
            USE_UTC => None,
            n => Some(-(n as i16 as i32)),
        }
    }

    /// Score coefficient for Field Day (`ScoreCoeff`), stored as hundredths.
    pub fn score_coeff(&self) -> f64 {
        self.header.rst_rcvd as f64 / 100.0
    }

    pub fn set_score_coeff(&mut self, coeff: f64) {
        self.header.rst_rcvd = (coeff * 100.0).trunc() as u16;
    }

    /// Contest start time, kept in `Reserve2`/`Reserve3` of the header.
    pub fn start_time(&self) -> DelphiTime {
        let mut b = [0u8; 8];
        b[..4].copy_from_slice(&self.header.reserve2.to_le_bytes());
        b[4..].copy_from_slice(&self.header.reserve3.to_le_bytes());
        DelphiTime(f64::from_le_bytes(b))
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

    /// Dupe flag of every QSO, in log order (`SetDupeFlags`); invalid QSOs are never dupes.
    pub fn dupe_flags(&self) -> Vec<bool> {
        let mut seen = HashSet::new();
        self.qsos
            .iter()
            .map(|q| !q.invalid && !seen.insert(self.dupe_key(q)))
            .collect()
    }

    /// Set every dupe flag from [`Log::dupe_flags`].
    pub fn set_dupe_flags(&mut self) {
        let flags = self.dupe_flags();
        for (q, d) in self.qsos.iter_mut().zip(flags) {
            q.dupe = d;
        }
    }

    /// Index of an earlier valid QSO that makes `q` a dupe (`IsDupe`).
    pub fn find_dupe(&self, q: &Qso) -> Option<usize> {
        let key = self.dupe_key(q);
        self.qsos
            .iter()
            .position(|p| !p.invalid && self.dupe_key(p) == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Band, Mode};

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
    fn header_fields() {
        let mut log = Log::default();
        log.header.rst_sent = (-540i16) as u16;
        assert_eq!(log.utc_offset_minutes(), Some(540));
        log.set_score_coeff(1.5);
        assert_eq!(log.score_coeff(), 1.5);
        let (back, _) = Log::from_bytes(&log.to_bytes(Format::Zlox)).unwrap();
        assert_eq!(back.score_coeff(), 1.5);
    }
}
