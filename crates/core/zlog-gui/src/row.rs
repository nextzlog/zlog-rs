use zlog_core::{Band, DelphiTime, Mode, Power};
use zlog_ui::{EntryCaps, EntryEnv, EntryField, EntryFields, EntryOps, LogOptions, NotLogged};

use crate::entry::Entry;
use crate::main_window::View;
use crate::session::{self, Session};

pub struct Row<'a> {
    pub session: &'a mut Session,
    pub view: &'a mut View,
    pub hz: Option<u64>,
}

impl EntryFields for Row<'_> {
    fn text(&self, field: EntryField) -> &str {
        let e = &self.session.entry;
        match field {
            EntryField::Call => &e.callsign,
            EntryField::RstSent => &e.rst_sent,
            EntryField::RstRcvd => &e.rst_rcvd,
            EntryField::Number => &e.nr_rcvd,
            EntryField::Memo => &e.memo,
            EntryField::Time => &e.time,
            EntryField::Date => &e.date,
        }
    }

    fn set_text(&mut self, field: EntryField, text: String) {
        let e = &mut self.session.entry;
        match field {
            EntryField::Call => e.callsign = text.to_uppercase(),
            EntryField::RstSent => e.rst_sent = text,
            EntryField::RstRcvd => e.rst_rcvd = text,
            EntryField::Number => {
                e.nr_rcvd = text.to_uppercase();
                self.session.check_number();
            }
            EntryField::Memo => e.memo = text,
            EntryField::Time => e.time = text,
            EntryField::Date => e.date = text,
        }
    }

    fn suggestion(&self, field: EntryField) -> Option<&zlog_ui::Suggestion> {
        let call = &self.session.entry.callsign;
        zlog_ui::suggest::find(&self.view.suggestions, field, call)
    }

    fn band(&self) -> Band {
        self.session.entry.band
    }

    fn mode(&self) -> Mode {
        self.session.entry.mode
    }

    fn power(&self) -> Power {
        self.session.entry.power
    }

    fn cq(&self) -> bool {
        self.session.entry.cq
    }

    fn operator(&self) -> &str {
        &self.session.station.operator
    }

    fn sent(&self) -> String {
        self.session.sent_number()
    }

    fn dupe(&self) -> bool {
        let e = &self.session.entry;
        let probe = zlog_core::Qso {
            callsign: e.callsign.trim().to_uppercase(),
            band: e.band,
            mode: e.mode,
            ..Default::default()
        };
        !probe.callsign.is_empty() && self.session.log().find_dupe(&probe).is_some()
    }

    fn bands(&self) -> Vec<Band> {
        self.session.bands()
    }

    fn modes(&self) -> Vec<Mode> {
        self.session.modes()
    }

    fn new_multi(&self) -> Option<String> {
        let e = &self.session.entry;
        let probe = zlog_core::Qso {
            nr_rcvd: e.nr_rcvd.to_uppercase(),
            band: e.band,
            ..Default::default()
        };
        match self.session.rules().check_multi(&probe) {
            zlog_core::MultiCheck::Known {
                code, needed: true, ..
            } if !probe.nr_rcvd.is_empty() => Some(code),
            _ => None,
        }
    }
}

impl EntryOps for Row<'_> {
    fn cycle_band(&mut self, back: bool) {
        self.session.cycle_band(back);
    }

    fn cycle_mode(&mut self, back: bool) {
        self.session.cycle_mode(back);
    }

    fn set_band(&mut self, band: Band) {
        let e = &mut self.session.entry;
        e.band = band;
        e.power = self.session.station.power_for(band);
    }

    fn set_mode(&mut self, mode: Mode) {
        self.session.entry.set_mode(mode);
    }

    fn qsy(&mut self, hz: u64) {
        self.view.qsy = Some(hz);
    }

    fn cycle_power(&mut self, back: bool) {
        self.session.entry.cycle_power(back);
    }

    fn toggle_cq(&mut self) {
        self.session.entry.cq = !self.session.entry.cq;
    }

    fn bump_s(&mut self, field: EntryField, up: bool) {
        let e = &mut self.session.entry;
        match field {
            EntryField::RstSent => Entry::bump_s(&mut e.rst_sent, up),
            EntryField::RstRcvd => Entry::bump_s(&mut e.rst_rcvd, up),
            _ => {}
        }
    }

    fn clear(&mut self) {
        self.session.entry.clear_call_and_number();
        self.session.tab = zlog_ui::TabState::Normal;
        self.session.status = Default::default();
    }

    fn complete_call(&mut self) -> bool {
        let dupe = self.session.check_call();
        self.session.fill_exchange();
        if !dupe {
            self.session.check_number();
        }
        dupe
    }

    fn check(&mut self) -> Result<(), NotLogged> {
        let now = self.typed_time()?;
        self.session.entry_qso(now, self.hz).map(|_| ())
    }

    fn log_with(&mut self, options: LogOptions) -> Result<(), NotLogged> {
        let time = match options.time {
            Some(t) => t,
            None => self.typed_time()?,
        };
        self.session.log_entry_with(time, self.hz, options.period)
    }

    fn log_forced(&mut self) -> Result<(), NotLogged> {
        let time = self.typed_time()?;
        self.session.log_entry_forced(time, self.hz)
    }

    fn set_power(&mut self, power: Power) {
        self.session.entry.power = power;
    }
}

impl Row<'_> {
    fn typed_time(&mut self) -> Result<DelphiTime, NotLogged> {
        let e = &self.session.entry;
        let now = session::now_for(self.session.log());
        zlog_ui::entry_time(&e.date, &e.time, now).ok_or_else(|| {
            self.session.status = session::Status::warning(crate::i18n::tr("status.invalid_time"));
            NotLogged::InvalidTime
        })
    }
}

impl EntryEnv for Row<'_> {
    fn caps(&self) -> EntryCaps {
        EntryCaps {
            rig: self.view.has_rig,
            keyer: true,
            time: true,
            period_refuses: true,
        }
    }

    fn now(&self) -> DelphiTime {
        session::now_for(self.session.log())
    }

    fn qso_count(&self) -> usize {
        self.session.log().qsos.len()
    }

    fn columns(&self) -> &[f32] {
        &self.view.widths
    }

    fn toggle_rig(&mut self) {
        self.view.toggle_rig = true;
    }

    fn enter_log(&mut self) {
        let n = self.session.log().qsos.len();
        self.view.selected = n.checked_sub(1);
    }

    fn leave_log(&mut self) {
        self.view.selected = None;
    }

    fn send(&mut self, n: u8) {
        self.view.sends.push(n);
    }

    fn tab_sent(&mut self) {
        let call = self.session.entry.callsign.trim().to_string();
        self.session.tab = zlog_ui::TabState::Sent(call);
    }

    fn run_role(&mut self, role: &str) {
        self.view.run_role = Some(role.to_string());
    }

    fn key_messages(&self) -> Vec<String> {
        self.view.messages.clone()
    }

    fn language(&self) -> &str {
        crate::i18n::current()
    }

    fn exchange_label(&self) -> Option<String> {
        self.session.rules().exchange_label(crate::i18n::current())
    }
}
