//! The entry row as the entry styles work it ([`zlog_ui::EntryState`]): its fields
//! in [`Session::entry`], and the operations every style shares, carried out by
//! the session.

use zlog_core::{Band, DelphiTime, Mode, Power};
use zlog_ui::{EntryField, EntryState, NotLogged};

use crate::entry::Entry;
use crate::main_window::View;
use crate::session::{self, Session};

/// The open session and the main window, for one call of a style.
pub struct Row<'a> {
    pub session: &'a mut Session,
    pub view: &'a mut View,
    /// The rig frequency, recorded in a QSO logged now.
    pub hz: Option<u64>,
}

impl EntryState for Row<'_> {
    fn text(&self, field: EntryField) -> &str {
        let e = &self.session.entry;
        match field {
            EntryField::Call => &e.callsign,
            EntryField::RstSent => &e.rst_sent,
            EntryField::RstRcvd => &e.rst_rcvd,
            EntryField::Number => &e.nr_rcvd,
            EntryField::Memo => &e.memo,
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
        }
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

    fn now(&self) -> DelphiTime {
        session::now_for(self.session.log())
    }

    fn qso_count(&self) -> usize {
        self.session.log().qsos.len()
    }

    fn columns(&self) -> &[f32] {
        &self.view.widths
    }

    fn cycle_band(&mut self, back: bool) {
        self.session.cycle_band(back);
    }

    fn cycle_mode(&mut self, back: bool) {
        self.session.cycle_mode(back);
    }

    fn cycle_power(&mut self) {
        self.session.entry.cycle_power();
    }

    fn toggle_cq(&mut self) {
        self.session.entry.cq = !self.session.entry.cq;
    }

    fn toggle_rig(&mut self) {
        self.view.toggle_rig = true;
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

    fn log(&mut self) -> Result<(), NotLogged> {
        let now = session::now_for(self.session.log());
        self.session.log_entry_on(now, self.hz)
    }

    fn enter_log(&mut self) {
        let n = self.session.log().qsos.len();
        self.view.selected = n.checked_sub(1);
    }

    fn leave_log(&mut self) {
        self.view.selected = None;
    }
}
