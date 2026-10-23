//! What the QSY indicator shows (`TMainForm.Update10MinTimer`, main.pas): the time
//! left of the countdown, or the QSY count of the hour, and whether a band change
//! is allowed now.

use zlog_core::{DelphiTime, Log};

/// The main line of the indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// Both QSY rules are off (`None`, grey).
    Off,
    /// The countdown is over or not running (`QSY OK`).
    QsyOk,
    /// Seconds left of the countdown.
    Left(u32),
    /// Band changes of this hour (`QSY# n`).
    Count(usize),
}

impl Shown {
    /// The countdown as zLog writes it: seconds alone under a minute, else `MM:SS`.
    pub fn left_text(secs: u32) -> String {
        let (m, s) = (secs / 60, secs % 60);
        if m == 0 {
            s.to_string()
        } else {
            format!("{m:02}:{s:02}")
        }
    }
}

/// The indicator: what it shows and whether a band change is allowed now (green;
/// yellow when not).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indicator {
    pub shown: Shown,
    pub ok: bool,
}

/// The indicator of TX `tx` at `now` by the QSY rules of `log`. With both rules on,
/// the QSY count decides, as in zLog, where it is checked last.
pub fn indicator(log: &Log, tx: u8, now: DelphiTime) -> Indicator {
    let rule = log.qsy_rule;
    let mut out = Indicator {
        shown: Shown::Off,
        ok: false,
    };
    // `SecondsBetween` is absolute, so a start ahead of the clock never makes the
    // countdown longer than the rule.
    if let Some(minutes) = rule.countdown_minutes {
        let left = countdown_start(log, tx)
            .map(|start| minutes as i64 * 60 - (secs(now) - secs(start)).abs())
            .unwrap_or(0);
        out = if left > 0 {
            Indicator {
                shown: Shown::Left(left as u32),
                ok: false,
            }
        } else {
            Indicator {
                shown: Shown::QsyOk,
                ok: true,
            }
        };
    }
    if let Some(max) = rule.max_qsy_per_hour {
        let n = log.qsy_count_at(tx, now);
        out = Indicator {
            shown: Shown::Count(n),
            ok: n < max as usize,
        };
    }
    out
}

/// When the countdown of `tx` started (`CountDownStartTime`): the time of its
/// first QSO, or of its last allowed band change; `None` without a QSO. zLog
/// starts it on logging such a QSO (`LogButtonProc`); here it is found in the log,
/// so a log opened later shows the same.
fn countdown_start(log: &Log, tx: u8) -> Option<DelphiTime> {
    let mut start = None;
    let mut band = None;
    for q in log.qsos.iter().filter(|q| q.tx == tx && !q.invalid) {
        if band.is_none() || (band != Some(q.band) && !q.qsy_violation) {
            start = Some(q.time);
        }
        band = Some(q.band);
    }
    start
}

/// A time in whole seconds, as zLog compares `TDateTime` values.
fn secs(t: DelphiTime) -> i64 {
    (t.0 * 86400.0).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::{Band, Qso, QsyRule};

    fn at(h: u32, m: u32, s: u32) -> DelphiTime {
        DelphiTime::from_ymd_hms(2026, 6, 6, h, m, s)
    }

    fn qso(band: Band, h: u32, m: u32) -> Qso {
        Qso {
            band,
            time: at(h, m, 0),
            ..Qso::default()
        }
    }

    fn log(rule: QsyRule, qsos: Vec<Qso>) -> Log {
        Log {
            qsos,
            qsy_rule: rule,
            ..Log::default()
        }
    }

    const COUNTDOWN: QsyRule = QsyRule {
        countdown_minutes: Some(10),
        max_qsy_per_hour: None,
    };

    #[test]
    fn off_without_rules() {
        let l = log(QsyRule::default(), vec![qso(Band::B7, 21, 0)]);
        assert_eq!(indicator(&l, 0, at(21, 1, 0)).shown, Shown::Off);
    }

    #[test]
    fn countdown_runs_from_the_last_band_change() {
        let mut l = log(
            COUNTDOWN,
            vec![
                qso(Band::B7, 21, 0),
                qso(Band::B14, 21, 20),
                qso(Band::B14, 21, 25),
            ],
        );
        let now = at(21, 28, 30);
        let i = indicator(&l, 0, now);
        assert_eq!((i.shown, i.ok), (Shown::Left(90), false));
        assert_eq!(
            indicator(&l, 0, at(21, 30, 0)),
            Indicator {
                shown: Shown::QsyOk,
                ok: true
            }
        );
        // A band change flagged as a violation starts no new countdown.
        l.qsos.push(Qso {
            qsy_violation: true,
            ..qso(Band::B21, 21, 26)
        });
        assert_eq!(indicator(&l, 0, now).shown, Shown::Left(90));
        // Another TX, and a log without QSOs, may change band now.
        assert_eq!(indicator(&l, 1, now).shown, Shown::QsyOk);
        assert_eq!(
            indicator(&log(COUNTDOWN, vec![]), 0, now).shown,
            Shown::QsyOk
        );
    }

    #[test]
    fn countdown_starts_at_the_first_qso() {
        let l = log(COUNTDOWN, vec![qso(Band::B7, 21, 0), qso(Band::B7, 21, 5)]);
        assert_eq!(indicator(&l, 0, at(21, 9, 15)).shown, Shown::Left(45));
    }

    #[test]
    fn qsy_count_decides_when_on() {
        let rule = QsyRule {
            countdown_minutes: Some(10),
            max_qsy_per_hour: Some(2),
        };
        let mut l = log(rule, vec![qso(Band::B7, 21, 0), qso(Band::B14, 21, 5)]);
        let now = at(21, 6, 0);
        assert_eq!(
            indicator(&l, 0, now),
            Indicator {
                shown: Shown::Count(1),
                ok: true
            }
        );
        l.qsos.push(qso(Band::B7, 21, 6));
        assert_eq!(
            indicator(&l, 0, now),
            Indicator {
                shown: Shown::Count(2),
                ok: false
            }
        );
    }

    #[test]
    fn countdown_text() {
        assert_eq!(Shown::left_text(45), "45");
        assert_eq!(Shown::left_text(60), "01:00");
        assert_eq!(Shown::left_text(9 * 60 + 5), "09:05");
    }

    #[test]
    fn a_start_ahead_of_the_clock_never_exceeds_the_rule() {
        let mut log = Log {
            qsy_rule: QsyRule {
                countdown_minutes: Some(10),
                ..QsyRule::default()
            },
            ..Log::default()
        };
        log.qsos.push(qso(Band::B7, 12, 5));
        let i = indicator(&log, 0, at(12, 0, 0));
        assert_eq!(i.shown, Shown::Left(300));
    }
}
