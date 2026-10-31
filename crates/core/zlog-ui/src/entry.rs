use zlog_core::{Band, DelphiTime, Mode, Power};

use crate::{EntryField, Suggestion};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotLogged {
    NoCallsign,
    InvalidNumber,
    InvalidTime,
    OutOfPeriod,
    Refused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    #[default]
    Warn,
    Refuse,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LogOptions {
    pub time: Option<DelphiTime>,
    pub period: Period,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EntryCaps {
    pub rig: bool,
    pub keyer: bool,
    pub time: bool,
    pub period_refuses: bool,
}

pub trait EntryFields {
    fn text(&self, field: EntryField) -> &str;

    fn set_text(&mut self, field: EntryField, text: String);

    fn suggestion(&self, _field: EntryField) -> Option<&Suggestion> {
        None
    }

    fn accept_suggestion(&mut self, field: EntryField) -> bool {
        let text = match self.suggestion(field) {
            Some(s) if self.text(field).trim().is_empty() => s.text.clone(),
            _ => return false,
        };
        self.set_text(field, text);
        true
    }

    fn band(&self) -> Band;

    fn mode(&self) -> Mode;

    fn power(&self) -> Power;

    fn bands(&self) -> Vec<Band> {
        Vec::new()
    }

    fn modes(&self) -> Vec<Mode> {
        Vec::new()
    }

    fn powers(&self) -> Vec<Power> {
        [Power::P001, Power::P002, Power::P005, Power::P010].to_vec()
    }

    fn cq(&self) -> bool;

    fn operator(&self) -> &str;

    fn sent(&self) -> String;

    fn dupe(&self) -> bool {
        false
    }

    fn new_multi(&self) -> Option<String> {
        None
    }
}

pub trait EntryOps: EntryFields {
    fn cycle_band(&mut self, back: bool);

    fn cycle_mode(&mut self, back: bool);

    fn set_band(&mut self, band: Band);

    fn set_mode(&mut self, mode: Mode);

    fn qsy(&mut self, hz: u64) {
        if let Some(b) = zlog_core::band_of(hz) {
            self.set_band(b);
        }
    }

    fn cycle_power(&mut self, back: bool);

    fn toggle_cq(&mut self);

    fn bump_s(&mut self, field: EntryField, up: bool);

    fn clear(&mut self);

    fn complete_call(&mut self) -> bool;

    fn log_with(&mut self, options: LogOptions) -> Result<(), NotLogged>;

    fn log(&mut self) -> Result<(), NotLogged> {
        self.log_with(LogOptions::default())
    }

    fn check(&mut self) -> Result<(), NotLogged> {
        Ok(())
    }

    fn log_forced(&mut self) -> Result<(), NotLogged> {
        self.log()
    }

    fn set_power(&mut self, power: Power) {
        for _ in 0..16 {
            if self.power() == power {
                return;
            }
            self.cycle_power(false);
        }
    }
}

pub trait EntryEnv {
    fn caps(&self) -> EntryCaps {
        EntryCaps::default()
    }

    fn now(&self) -> DelphiTime;

    fn qso_count(&self) -> usize;

    fn columns(&self) -> &[f32] {
        &[]
    }

    fn toggle_rig(&mut self) {}

    fn enter_log(&mut self) {}

    fn leave_log(&mut self) {}

    fn send(&mut self, _n: u8) {}

    fn tab_sent(&mut self) {}

    fn run_role(&mut self, _role: &str) {}

    fn key_messages(&self) -> Vec<String> {
        Vec::new()
    }

    fn language(&self) -> &str {
        "en"
    }

    fn exchange_label(&self) -> Option<String> {
        None
    }
}

pub trait EntryState: EntryFields + EntryOps + EntryEnv {}

impl<T: EntryFields + EntryOps + EntryEnv + ?Sized> EntryState for T {}

pub trait EntryStyle {
    fn id(&self) -> &str;

    fn title(&self, language: &str) -> String;

    fn keys(&mut self, ctx: &egui::Context, entry: &mut dyn EntryState);

    fn ui(&mut self, ui: &mut egui::Ui, entry: &mut dyn EntryState);

    fn focused(&self, ctx: &egui::Context) -> Option<EntryField>;

    fn focus(&mut self, field: EntryField);

    fn reserved_keys(&self) -> Vec<egui::KeyboardShortcut> {
        Vec::new()
    }

    fn load(&mut self, _section: &crate::Section) {}

    fn save(&self, _section: &mut crate::Section) {}

    fn uses_main_row(&self) -> bool {
        true
    }

    fn entry_changed(&mut self) {}

    fn caret_to_end(&mut self, _field: EntryField) {}

    fn messages_sent(&mut self, _entry: &mut dyn EntryState) {}

    fn own_window(&self) -> Option<&str> {
        None
    }

    fn close_window(&mut self) {}

    fn docked_ui(&mut self, _ui: &mut egui::Ui, _entry: &mut dyn EntryState) {}

    fn floating_ui(&mut self, ui: &mut egui::Ui, entry: &mut dyn EntryState, _wrap: Wrap<'_>) {
        self.ui(ui, entry)
    }
}

pub type Wrap<'a> = &'a mut dyn FnMut(&mut egui::Ui, &mut dyn FnMut(&mut egui::Ui));

#[cfg(test)]
pub(crate) mod fake {
    use super::*;

    #[derive(Default)]
    pub struct Fake {
        pub call: String,
        pub number: String,
        pub time: String,
        pub cq: bool,
        pub dupe: bool,
        pub sent: Vec<u8>,
        pub logged: usize,
        pub options: Option<LogOptions>,
        pub refuse: Option<NotLogged>,
        pub band: Option<Band>,
        pub mode: Option<Mode>,
        pub suggest: Option<Suggestion>,
        pub forced: usize,
        pub tabbed: usize,
    }

    impl EntryFields for Fake {
        fn text(&self, field: EntryField) -> &str {
            match field {
                EntryField::Call => &self.call,
                EntryField::Number => &self.number,
                EntryField::Time => &self.time,
                _ => "",
            }
        }
        fn set_text(&mut self, field: EntryField, text: String) {
            match field {
                EntryField::Call => self.call = text,
                EntryField::Number => self.number = text,
                EntryField::Time => self.time = text,
                _ => {}
            }
        }
        fn suggestion(&self, field: EntryField) -> Option<&Suggestion> {
            self.suggest
                .as_ref()
                .filter(|s| s.field == field && s.is_for(&self.call))
        }
        fn band(&self) -> Band {
            self.band.unwrap_or(Band::B7)
        }
        fn mode(&self) -> Mode {
            self.mode.unwrap_or(Mode::Cw)
        }
        fn power(&self) -> Power {
            Power::P010
        }
        fn cq(&self) -> bool {
            self.cq
        }
        fn operator(&self) -> &str {
            ""
        }
        fn sent(&self) -> String {
            String::new()
        }
        fn dupe(&self) -> bool {
            self.dupe
        }
    }

    impl EntryOps for Fake {
        fn cycle_band(&mut self, _back: bool) {}
        fn cycle_mode(&mut self, _back: bool) {}
        fn set_band(&mut self, band: Band) {
            self.band = Some(band);
        }
        fn set_mode(&mut self, mode: Mode) {
            self.mode = Some(mode);
        }
        fn cycle_power(&mut self, _back: bool) {}
        fn toggle_cq(&mut self) {
            self.cq = !self.cq;
        }
        fn bump_s(&mut self, _field: EntryField, _up: bool) {}
        fn clear(&mut self) {
            self.call.clear();
            self.number.clear();
        }
        fn complete_call(&mut self) -> bool {
            self.dupe
        }
        fn check(&mut self) -> Result<(), NotLogged> {
            self.refuse.map_or(Ok(()), Err)
        }
        fn log_with(&mut self, options: LogOptions) -> Result<(), NotLogged> {
            self.check()?;
            if self.call.is_empty() {
                return Err(NotLogged::NoCallsign);
            }
            self.options = Some(options);
            self.logged += 1;
            self.clear();
            Ok(())
        }
        fn log_forced(&mut self) -> Result<(), NotLogged> {
            if self.call.is_empty() {
                return Err(NotLogged::NoCallsign);
            }
            self.forced += 1;
            self.logged += 1;
            self.clear();
            Ok(())
        }
    }

    impl EntryEnv for Fake {
        fn now(&self) -> DelphiTime {
            DelphiTime::default()
        }
        fn qso_count(&self) -> usize {
            self.logged
        }
        fn send(&mut self, n: u8) {
            self.sent.push(n);
        }
        fn tab_sent(&mut self) {
            self.tabbed += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::Fake;
    use super::*;

    #[test]
    fn any_entry_with_the_three_parts_is_an_entry_state() {
        fn as_state(e: &mut dyn EntryState) -> &mut dyn EntryState {
            e
        }
        let mut f = Fake::default();
        let e = as_state(&mut f);
        assert_eq!(
            e.caps(),
            EntryCaps::default(),
            "nothing promised by default"
        );
        assert!(e.columns().is_empty());
        assert_eq!(e.language(), "en");
        e.set_band(Band::B14);
        e.set_mode(Mode::Ssb);
        assert_eq!((e.band(), e.mode()), (Band::B14, Mode::Ssb));
        let fields: &dyn EntryFields = e;
        assert!(!fields.dupe());
        assert_eq!(fields.new_multi(), None);
    }

    #[test]
    fn log_is_log_with_the_default_options() {
        let mut f = Fake {
            call: "JA1ZLO".into(),
            ..Default::default()
        };
        assert_eq!(f.log(), Ok(()));
        assert_eq!(f.options, Some(LogOptions::default()));
        assert_eq!(LogOptions::default().period, Period::Warn);
        f.call = "JA1ZLO".into();
        let at = LogOptions {
            time: Some(DelphiTime::from_ymd_hms(2026, 6, 6, 21, 5, 0)),
            period: Period::Refuse,
        };
        assert_eq!(f.log_with(at), Ok(()));
        assert_eq!(f.options, Some(at));
    }

    #[test]
    fn a_suggestion_is_taken_only_into_an_empty_field() {
        let mut f = Fake {
            call: "JA1ZLO".into(),
            suggest: Some(Suggestion::new(EntryField::Number, "10M", "JA1ZLO", "")),
            ..Default::default()
        };
        assert!(f.accept_suggestion(EntryField::Number));
        assert_eq!(f.number, "10M");
        f.number = "11M".into();
        assert!(!f.accept_suggestion(EntryField::Number));
        assert!(!f.accept_suggestion(EntryField::Memo));
    }
}
