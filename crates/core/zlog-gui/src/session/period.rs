use super::*;

impl Session {
    pub fn start_time(&self) -> DelphiTime {
        self.log().start_time()
    }

    pub fn set_start_time(&mut self, start: DelphiTime, judge: bool) -> usize {
        let b = start.0.to_le_bytes();
        let header = self.book.header_mut();
        header.reserve2 = i32::from_le_bytes(b[..4].try_into().unwrap());
        header.reserve3 = i32::from_le_bytes(b[4..].try_into().unwrap());
        self.asks_start_time = false;
        let mut changed = 0;
        if judge && self.use_contest_period {
            let updates: Vec<(QsoId, Qso)> = self
                .log()
                .qsos
                .iter()
                .filter(|q| q.invalid != self.out_of_period(q))
                .map(|q| {
                    let mut new = q.clone();
                    new.invalid = !q.invalid;
                    (QsoId::of(q), new)
                })
                .collect();
            self.book.begin_step();
            for (id, q) in updates {
                if self.execute_unsaved(Command::Update(id, q)) {
                    changed += 1;
                }
            }
            self.book.end_step();
        }
        self.save();
        changed
    }

    pub fn change_qsos(&mut self, ids: &[QsoId], change: impl Fn(&mut Qso)) -> usize {
        let mut changed = 0;
        self.book.begin_step();
        for &id in ids {
            let Some(index) = self.find(id) else {
                continue;
            };
            let mut q = self.log().qsos[index].clone();
            change(&mut q);
            if q != self.log().qsos[index] && self.execute_unsaved(Command::Update(id, q)) {
                changed += 1;
            }
        }
        self.book.end_step();
        if changed > 0 {
            self.save();
        }
        changed
    }

    pub fn change_band(&mut self, ids: &[QsoId], band: Band) -> usize {
        self.change_qsos(ids, |q| q.band = band)
    }

    pub fn change_mode(&mut self, ids: &[QsoId], mode: Mode) -> usize {
        self.change_qsos(ids, |q| {
            let phone = |m: Mode| matches!(m, Mode::Ssb | Mode::Am | Mode::Fm | Mode::Dv);
            if phone(mode) && !phone(q.mode) {
                q.rst_sent /= 10;
                q.rst_rcvd /= 10;
            } else if !phone(mode) && phone(q.mode) {
                let gain = |r: u16| {
                    r.checked_mul(10)
                        .and_then(|r| r.checked_add(9))
                        .filter(|_| r < 1000)
                        .unwrap_or(r)
                };
                q.rst_sent = gain(q.rst_sent);
                q.rst_rcvd = gain(q.rst_rcvd);
            }
            q.mode = mode;
        })
    }

    pub fn change_power(&mut self, ids: &[QsoId], power: Power) -> usize {
        self.change_qsos(ids, |q| q.power = power)
    }
}

pub fn with_time(sent: &str, hhmm: &str) -> String {
    if sent.contains("$D") {
        sent.replace("$D", hhmm)
    } else {
        format!("{sent}{hhmm}")
    }
}

pub fn utc_hhmm(time: DelphiTime, offset: Option<i32>) -> String {
    let utc = DelphiTime(time.0 - offset.unwrap_or(0) as f64 / 1440.0);
    let (_, _, _, h, m, _) = utc.to_ymd_hms();
    format!("{h:02}{m:02}")
}

pub fn khz_string(hz: u64) -> String {
    format!("{}.{}", hz / 1000, hz % 1000 / 100)
}

pub fn local_utc_offset_minutes() -> i32 {
    chrono::Local::now().offset().local_minus_utc() / 60
}

pub fn now_for(log: &Log) -> DelphiTime {
    use chrono::{Datelike, Timelike};
    let t = if log.utc_offset_minutes().is_none() {
        chrono::Utc::now().naive_utc()
    } else {
        chrono::Local::now().naive_local()
    };
    DelphiTime::from_ymd_hms(
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
    )
}

pub fn default_start_time(now: DelphiTime, start_hour: Option<u32>) -> DelphiTime {
    let day = now.0.floor() + if start_hour == Some(0) { 1.0 } else { 0.0 };
    let (_, _, _, hour, _, _) = now.to_ymd_hms();
    let h = start_hour.unwrap_or(hour);
    DelphiTime(day + h as f64 / 24.0)
}
