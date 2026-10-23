//! The entry row at the bottom of the main window, without any drawing.

use zlog_core::{Band, Contest, ContestMode, DelphiTime, Mode, Power, Qso, Station};

/// Contents of the entry row (`CurrentQSO` and its edit boxes).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub callsign: String,
    pub rst_sent: String,
    pub rst_rcvd: String,
    pub nr_rcvd: String,
    pub band: Band,
    pub mode: Mode,
    /// Power of this QSO; follows the station's power for the band (`SetOpPower`).
    pub power: Power,
    pub memo: String,
    /// CQ (run) rather than S&P (`CurrentQSO.CQ`); toggled with Shift+O
    /// (`actionToggleCqSp`), kept when the row is cleared, stored in `Qso::cq`.
    pub cq: bool,
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
        };
        e.reset_rst();
        e
    }

    fn reset_rst(&mut self) {
        let rst = default_rst(self.mode).to_string();
        self.rst_sent = rst.clone();
        self.rst_rcvd = rst;
    }

    /// Clear the row after logging, keeping band, mode and power. The RSTs restart
    /// from the post-log rule (`TMainForm` after logging, main.pas: 599 for CW and
    /// RTTY, 59 otherwise), which differs from a mode change for FT4/FT8/Other.
    pub fn wipe(&mut self) {
        let cq = self.cq;
        *self = Entry::new(self.band, self.mode, self.power);
        self.cq = cq;
        let rst = logged_rst(self.mode).to_string();
        self.rst_sent = rst.clone();
        self.rst_rcvd = rst;
    }

    /// Clear callsign, number and memo, keeping the RSTs (`actionClearCallAndRpt`, Ctrl+K).
    pub fn clear_call_and_number(&mut self) {
        self.callsign.clear();
        self.nr_rcvd.clear();
        self.memo.clear();
    }

    /// Next band of the contest (Shift+B); the power follows the station's for the band.
    pub fn cycle_band(&mut self, contest: &dyn Contest, station: &Station, back: bool) {
        let (lo, hi) = contest.band_range();
        let bands: Vec<Band> = Band::range(lo, hi)
            .filter(|b| contest.uses_warc() || !b.is_warc())
            .collect();
        self.band = cycle(&bands, self.band, back);
        self.power = station.power_for(self.band);
    }

    /// Next mode (Shift+M), within the highest mode for the band (`SetQSOMode`):
    /// forward past the limit wraps to CW, backward stops at the limit.
    /// `all_modes` lifts the limit to DV (`LastMode`), as zLog does for Pedition.
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

    /// Set the mode, with the RSTs it starts from (59 or 599), as a mode change does.
    pub fn set_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            self.reset_rst();
        }
    }

    /// Next power code (Shift+P): H, P, L, M.
    pub fn cycle_power(&mut self) {
        self.power = self.power.next_code();
    }

    /// Raise or lower the S figure of an RST (Shift+Up / Shift+Down; `IncreaseS`).
    pub fn bump_s(rst: &mut String, up: bool) {
        let Ok(n) = rst.trim().parse::<u16>() else {
            return;
        };
        // 59: S is the units; 599: S is the tens.
        let place = if n < 100 { 1 } else { 10 };
        let s = n / place % 10;
        let s2 = match (up, s) {
            (true, s) if s >= 9 => 1,
            (true, s) => s + 1,
            (false, s) if s <= 1 => 9,
            (false, s) => s - 1,
        };
        // In u32: 65535 with a raised S would not fit in u16.
        let (n, s, s2, place) = (u32::from(n), u32::from(s), u32::from(s2), u32::from(place));
        *rst = (n - s * place + s2 * place).to_string();
    }

    /// A QSO record built from the row, stamped with `time`.
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
            // main.pas:5964 `Q.Serial := Log.GetCurrentSerial`.
            serial: contest
                .next_serial_of_tx(self.band, station.tx_nr)
                .unwrap_or(0),
            ..Qso::default()
        }
    }
}

/// The highest mode Shift+M reaches on a band (`SetQSOMode`). ALL and RTTY contests
/// go up to RTTY on every band; others stop at what the band plan carries.
pub fn max_mode(band: Band, contest_mode: ContestMode) -> Mode {
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

/// The RSTs a mode change starts from (`TMainForm.UpdateMode`): 59 for the phone
/// modes, 599 for all others (CW, RTTY, FT4, FT8, Other).
pub fn default_rst(mode: Mode) -> u16 {
    match mode {
        Mode::Ssb | Mode::Fm | Mode::Am | Mode::Dv => 59,
        _ => 599,
    }
}

/// The RSTs the row restarts from after logging (main.pas, the reset of
/// `CurrentQSO` after a QSO is logged): 599 for CW and RTTY, 59 for all others.
pub fn logged_rst(mode: Mode) -> u16 {
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
    use zlog_rule_ja::JarlContest;

    fn entry(band: Band, mode: Mode) -> Entry {
        Entry::new(band, mode, Station::default_power(band))
    }

    #[test]
    fn band_cycle_skips_warc_wraps_and_sets_power() {
        let c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        let st = Station::default();
        let mut e = entry(Band::B7, Mode::Cw);
        e.cycle_band(&c, &st, false);
        assert_eq!(e.band, Band::B14);
        e.band = Band::B50;
        e.cycle_band(&c, &st, false);
        assert_eq!(e.band, Band::B1_9);
        e.cycle_band(&c, &st, true);
        assert_eq!(e.band, Band::B50);
        let six = JarlContest::SixDown.build_rules(ContestMode::Mix);
        e.band = Band::B430;
        e.cycle_band(&six, &st, false);
        assert_eq!((e.band, e.power), (Band::B1200, Power::P002));
    }

    #[test]
    fn mode_cycle_respects_the_band() {
        let mut e = entry(Band::B7, Mode::Cw);
        e.cycle_mode(ContestMode::Mix, false, false);
        assert_eq!((e.mode, e.rst_sent.as_str()), (Mode::Ssb, "59"));
        e.cycle_mode(ContestMode::Mix, false, false);
        assert_eq!(e.mode, Mode::Cw, "SSB is the top on 7 MHz");
        e.band = Band::B28;
        e.mode = Mode::Ssb;
        e.cycle_mode(ContestMode::Mix, false, false);
        assert_eq!(e.mode, Mode::Fm);
        e.cycle_mode(ContestMode::All, false, false);
        assert_eq!(e.mode, Mode::Am);
    }

    #[test]
    fn mode_change_rst_follows_update_mode() {
        for (m, rst) in [
            (Mode::Cw, 599),
            (Mode::Ssb, 59),
            (Mode::Fm, 59),
            (Mode::Am, 59),
            (Mode::Dv, 59),
            (Mode::Rtty, 599),
            (Mode::Ft4, 599),
            (Mode::Ft8, 599),
            (Mode::Other, 599),
        ] {
            assert_eq!(default_rst(m), rst, "{m:?}");
            let mut e = entry(Band::B7, Mode::Cw);
            e.set_mode(Mode::Ssb);
            e.set_mode(m);
            assert_eq!(e.rst_sent, rst.to_string(), "{m:?}");
            assert_eq!(e.rst_rcvd, rst.to_string(), "{m:?}");
        }
    }

    #[test]
    fn logged_rst_follows_the_post_log_reset() {
        for (m, rst) in [
            (Mode::Cw, "599"),
            (Mode::Rtty, "599"),
            (Mode::Ft8, "59"),
            (Mode::Ft4, "59"),
            (Mode::Other, "59"),
            (Mode::Ssb, "59"),
        ] {
            let mut e = entry(Band::B7, m);
            e.callsign = "JA1ZLO".into();
            e.wipe();
            assert_eq!(
                (e.rst_sent.as_str(), e.rst_rcvd.as_str()),
                (rst, rst),
                "{m:?}"
            );
            assert_eq!(e.callsign, "");
        }
    }

    #[test]
    fn set_mode_to_the_same_mode_keeps_typed_rst() {
        let mut e = entry(Band::B7, Mode::Cw);
        e.rst_rcvd = "579".into();
        e.set_mode(Mode::Cw);
        assert_eq!(e.rst_rcvd, "579");
    }

    #[test]
    fn all_modes_reach_dv_and_wrap() {
        let mut e = entry(Band::B7, Mode::Ft8);
        e.cycle_mode(ContestMode::Mix, true, false);
        assert_eq!(e.mode, Mode::Dv, "Pedition goes up to DV");
        e.cycle_mode(ContestMode::Mix, true, false);
        assert_eq!(e.mode, Mode::Other);
        e.cycle_mode(ContestMode::Mix, true, false);
        assert_eq!(e.mode, Mode::Cw);
        e.cycle_mode(ContestMode::Mix, true, true);
        assert_eq!(e.mode, Mode::Other);
        e.cycle_mode(ContestMode::Mix, true, true);
        assert_eq!(e.mode, Mode::Dv);
        // Without the exception, 7 MHz stops at SSB; backward from CW lands there.
        let mut e = entry(Band::B7, Mode::Cw);
        e.cycle_mode(ContestMode::Mix, false, true);
        assert_eq!(e.mode, Mode::Ssb);
    }

    #[test]
    fn power_cycles_h_p_l_m() {
        let mut e = entry(Band::B7, Mode::Cw);
        let codes: Vec<char> = (0..4)
            .map(|_| {
                e.cycle_power();
                e.power.code().unwrap()
            })
            .collect();
        assert_eq!(codes, vec!['P', 'L', 'M', 'H']);
    }

    #[test]
    fn s_figure_goes_round() {
        let mut r = "599".to_string();
        Entry::bump_s(&mut r, true);
        assert_eq!(r, "519");
        Entry::bump_s(&mut r, false);
        assert_eq!(r, "599");
        let mut r = "51".to_string();
        Entry::bump_s(&mut r, false);
        assert_eq!(r, "59");
        let mut r = "65535".to_string();
        Entry::bump_s(&mut r, true);
        assert_eq!(r, "65545");
    }

    #[test]
    fn clear_keeps_rst() {
        let mut e = entry(Band::B7, Mode::Cw);
        e.callsign = "JA1ZLO".into();
        e.rst_rcvd = "579".into();
        e.clear_call_and_number();
        assert_eq!((e.callsign.as_str(), e.rst_rcvd.as_str()), ("", "579"));
    }

    #[test]
    fn to_qso_fills_sent_number_and_power() {
        let c = JarlContest::AllJa.build_rules(ContestMode::Mix);
        let st = Station {
            prov: "10".into(),
            operator: "OP".into(),
            tx_nr: 2,
            ..Station::default()
        };
        let mut e = entry(Band::B7, Mode::Cw);
        e.power = Power::P005;
        e.cq = true;
        e.callsign = "ja1zlo ".into();
        e.nr_rcvd = "25h".into();
        let q = e.to_qso(&c, &st, DelphiTime(1.0), 7);
        assert_eq!(
            (q.callsign.as_str(), q.nr_sent.as_str(), q.nr_rcvd.as_str()),
            ("JA1ZLO", "10M", "25H")
        );
        assert_eq!(
            (q.rst_sent, q.power, q.operator.as_str(), q.reserve3),
            (599, Power::P005, "OP", 7)
        );
        assert_eq!((q.cq, q.tx), (true, 2));
        assert_eq!(q.serial, 0, "ALL JA has no serial numbers");
        e.wipe();
        assert!(e.cq, "CQ mode survives clearing the row");
    }
}
