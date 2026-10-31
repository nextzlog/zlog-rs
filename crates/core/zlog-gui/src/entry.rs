use zlog_core::{Band, Contest, ContestMode, DelphiTime, Mode, Power, Qso, Station};

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub callsign: String,
    pub rst_sent: String,
    pub rst_rcvd: String,
    pub nr_rcvd: String,
    pub band: Band,
    pub mode: Mode,
    pub power: Power,
    pub memo: String,
    pub cq: bool,
    pub date: String,
    pub time: String,
}

impl Entry {
    pub fn new(band: Band, mode: Mode, power: Power) -> Self {
        let mut e = Entry {
            callsign: String::new(),
            rst_sent: String::new(),
            rst_rcvd: String::new(),
            nr_rcvd: String::new(),
            band,
            mode,
            power,
            memo: String::new(),
            cq: false,
            date: String::new(),
            time: String::new(),
        };
        e.reset_rst();
        e
    }

    fn reset_rst(&mut self) {
        let rst = default_rst(self.mode).to_string();
        self.rst_sent = rst.clone();
        self.rst_rcvd = rst;
    }

    pub fn wipe(&mut self) {
        let cq = self.cq;
        *self = Entry::new(self.band, self.mode, self.power);
        self.cq = cq;
        let rst = logged_rst(self.mode).to_string();
        self.rst_sent = rst.clone();
        self.rst_rcvd = rst;
    }

    pub fn clear_call_and_number(&mut self) {
        self.callsign.clear();
        self.nr_rcvd.clear();
        self.memo.clear();
    }

    pub fn bands(contest: &dyn Contest) -> Vec<Band> {
        let (lo, hi) = contest.band_range();
        Band::range(lo, hi)
            .filter(|b| contest.uses_warc() || !b.is_warc())
            .collect()
    }

    pub fn modes(&self, contest_mode: ContestMode, all_modes: bool) -> Vec<Mode> {
        use Mode::*;
        let max = if all_modes {
            Mode::Dv
        } else {
            max_mode(self.band, contest_mode)
        };
        [Cw, Ssb, Fm, Am, Rtty, Ft4, Ft8, Dv, Other]
            .into_iter()
            .filter(|m| *m <= max)
            .collect()
    }

    pub fn cycle_band(&mut self, contest: &dyn Contest, station: &Station, back: bool) {
        self.band = cycle(&Self::bands(contest), self.band, back);
        self.power = station.power_for(self.band);
    }

    pub fn cycle_mode(&mut self, contest_mode: ContestMode, all_modes: bool, back: bool) {
        use Mode::*;
        let max = if all_modes {
            Mode::Dv
        } else {
            max_mode(self.band, contest_mode)
        };
        let next = if back {
            match self.mode {
                Cw => Other,
                Ssb => Cw,
                Fm => Ssb,
                Am => Fm,
                Rtty => Am,
                Ft4 => Rtty,
                Ft8 => Ft4,
                Other => Dv,
                Dv => Ft8,
            }
        } else {
            match self.mode {
                Cw => Ssb,
                Ssb => Fm,
                Fm => Am,
                Am => Rtty,
                Rtty => Ft4,
                Ft4 => Ft8,
                Ft8 => Dv,
                Other => Cw,
                Dv => Other,
            }
        };
        self.mode = match (next > max, back) {
            (true, false) => Cw,
            (true, true) => max,
            (false, _) => next,
        };
        self.reset_rst();
    }

    pub fn set_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            self.reset_rst();
        }
    }

    pub fn cycle_power(&mut self, back: bool) {
        self.power = if back {
            self.power.prev_code()
        } else {
            self.power.next_code()
        };
    }

    pub fn set_rst_rcvd(&mut self, rst: u16) -> bool {
        let ok = valid_rst(self.mode, rst);
        if ok {
            self.rst_rcvd = rst.to_string();
        }
        ok
    }

    pub fn bump_s(rst: &mut String, up: bool) {
        let Ok(n) = rst.trim().parse::<u16>() else {
            return;
        };
        let place = if n < 100 { 1 } else { 10 };
        let s = n / place % 10;
        let s2 = match (up, s) {
            (true, s) if s >= 9 => 1,
            (true, s) => s + 1,
            (false, s) if s <= 1 => 9,
            (false, s) => s - 1,
        };
        let (n, s, s2, place) = (u32::from(n), u32::from(s), u32::from(s2), u32::from(place));
        *rst = (n - s * place + s2 * place).to_string();
    }

    pub fn to_qso(
        &self,
        contest: &dyn Contest,
        station: &Station,
        time: DelphiTime,
        id: i32,
    ) -> Qso {
        Qso {
            time,
            callsign: self.callsign.trim().to_uppercase(),
            nr_sent: contest.sent_number(self.band, self.power, station),
            nr_rcvd: self.nr_rcvd.trim().to_uppercase(),
            rst_sent: self.rst_sent.parse().unwrap_or(default_rst(self.mode)),
            rst_rcvd: self.rst_rcvd.parse().unwrap_or(default_rst(self.mode)),
            band: self.band,
            mode: self.mode,
            power: self.power,
            operator: station.operator.clone(),
            memo: self.memo.clone(),
            cq: self.cq,
            tx: station.tx_nr,
            reserve3: id,
            serial: contest
                .next_serial_of_tx(self.band, station.tx_nr)
                .unwrap_or(0),
            ..Qso::default()
        }
    }
}

pub(crate) fn max_mode(band: Band, contest_mode: ContestMode) -> Mode {
    if matches!(contest_mode, ContestMode::All | ContestMode::Rtty) {
        return Mode::Rtty;
    }
    match band {
        b if b <= Band::B24 => Mode::Ssb,
        Band::B28 => Mode::Fm,
        Band::B50 => Mode::Am,
        _ => Mode::Fm,
    }
}

pub(crate) struct Follow<'a> {
    pub entry: &'a mut Entry,
    pub station: &'a zlog_core::Station,
}

impl zlog_ui::rig::RigEntry for Follow<'_> {
    fn band(&self) -> Band {
        self.entry.band
    }

    fn mode(&self) -> Mode {
        self.entry.mode
    }

    fn set_band(&mut self, band: Band) {
        self.entry.band = band;
        self.entry.power = self.station.power_for(band);
    }

    fn set_mode(&mut self, mode: Mode) {
        self.entry.set_mode(mode);
    }
}

pub fn default_rst(mode: Mode) -> u16 {
    match mode {
        Mode::Ssb | Mode::Fm | Mode::Am | Mode::Dv => 59,
        _ => 599,
    }
}

pub fn valid_rst(mode: Mode, rst: u16) -> bool {
    let digits: Vec<u16> = if default_rst(mode) < 100 {
        if !(11..=59).contains(&rst) {
            return false;
        }
        vec![rst / 10, rst % 10]
    } else {
        if !(111..=599).contains(&rst) {
            return false;
        }
        vec![rst / 100, rst / 10 % 10, rst % 10]
    };
    digits[0] <= 5 && digits.iter().all(|&d| d >= 1)
}

pub(crate) fn logged_rst(mode: Mode) -> u16 {
    match mode {
        Mode::Cw | Mode::Rtty => 599,
        _ => 59,
    }
}

fn cycle<T: Copy + PartialEq>(items: &[T], current: T, back: bool) -> T {
    let Some(i) = items.iter().position(|x| *x == current) else {
        return items[0];
    };
    let n = items.len();
    items[if back { (i + n - 1) % n } else { (i + 1) % n }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rst_rcvd_takes_three_digits_in_cw() {
        let mut e = Entry::new(Band::B7, Mode::Cw, Power::P010);
        e.callsign = "JA1ZLO".into();
        assert!(e.set_rst_rcvd(579));
        assert_eq!(e.rst_rcvd, "579");
        assert!(!e.set_rst_rcvd(59));
        assert_eq!(e.rst_rcvd, "579");
    }

    #[test]
    fn rst_rcvd_takes_two_digits_in_phone() {
        let mut e = Entry::new(Band::B7, Mode::Ssb, Power::P010);
        e.callsign = "JA1YWX".into();
        assert!(e.set_rst_rcvd(57));
        assert_eq!(e.rst_rcvd, "57");
        assert!(!e.set_rst_rcvd(599));
        assert_eq!(e.rst_rcvd, "57");
    }

    #[test]
    fn invalid_rst_rcvd_leaves_the_field() {
        let mut e = Entry::new(Band::B7, Mode::Cw, Power::P010);
        for bad in [0, 609, 590, 509, 99, 1000, 699] {
            assert!(!e.set_rst_rcvd(bad), "{bad}");
            assert_eq!(e.rst_rcvd, "599", "{bad}");
        }
        assert!(e.set_rst_rcvd(111));
    }

    #[test]
    fn modes_are_what_the_cycle_steps_through() {
        for band in [Band::B7, Band::B28, Band::B50, Band::B144] {
            for cm in [
                ContestMode::Mix,
                ContestMode::Phone,
                ContestMode::Rtty,
                ContestMode::Cw,
                ContestMode::All,
            ] {
                for all in [false, true] {
                    let mut e = Entry::new(band, Mode::Cw, Power::P010);
                    let list = e.modes(cm, all);
                    let mut seen = vec![e.mode];
                    for _ in 1..list.len() {
                        e.cycle_mode(cm, all, false);
                        seen.push(e.mode);
                    }
                    e.cycle_mode(cm, all, false);
                    assert_eq!(e.mode, Mode::Cw, "{band:?} {cm:?} {all}");
                    assert_eq!(seen, list, "{band:?} {cm:?} {all}");
                }
            }
        }
    }
}
