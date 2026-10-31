use std::time::{Duration, Instant};

use zlog_core::{Band, Mode};

use crate::enter::{EnterSends, Message, Step};
use crate::{EntryField, EntryState, NotLogged, Section, Suggestion};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub field: EntryField,
    pub select_all: bool,
}

impl Cursor {
    pub fn to(field: EntryField) -> Self {
        Cursor {
            field,
            select_all: false,
        }
    }
}

type QsoKey = (String, Band, Mode);

fn qso_key(entry: &dyn EntryState) -> QsoKey {
    let call = entry.text(EntryField::Call).trim().to_uppercase();
    (call, entry.band(), entry.mode())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallCommand {
    Qsy(u64),
    Mode(Mode),
}

const HAM_KHZ: &[(u64, u64)] = &[
    (1_800, 2_000),
    (3_500, 4_000),
    (7_000, 7_300),
    (10_100, 10_150),
    (14_000, 14_350),
    (18_068, 18_168),
    (21_000, 21_450),
    (24_890, 24_990),
    (28_000, 29_700),
    (50_000, 54_000),
    (144_000, 148_000),
    (420_000, 450_000),
    (1_240_000, 1_300_000),
    (2_300_000, 2_450_000),
    (5_650_000, 5_925_000),
    (10_000_000, 10_500_000),
];

pub fn call_command(text: &str) -> Option<CallCommand> {
    let text = text.trim().to_ascii_uppercase();
    let mode = match text.as_str() {
        "CW" => Some(Mode::Cw),
        "SSB" | "PH" => Some(Mode::Ssb),
        "FM" => Some(Mode::Fm),
        "AM" => Some(Mode::Am),
        "RTTY" | "RY" | "DIG" => Some(Mode::Rtty),
        "FT8" => Some(Mode::Ft8),
        "FT4" => Some(Mode::Ft4),
        _ => None,
    };
    if let Some(m) = mode {
        return Some(CallCommand::Mode(m));
    }
    let (int, frac) = text.split_once('.').unwrap_or((&text, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if int.is_empty() || int.len() > 8 || frac.len() > 3 || !digits(int) || !digits(frac) {
        return None;
    }
    let milli = format!("{frac:0<3}").parse::<u64>().ok()?;
    let hz = int.parse::<u64>().ok()? * 1000 + milli;
    let khz = hz / 1000;
    let ham = HAM_KHZ.iter().any(|(lo, hi)| (*lo..=*hi).contains(&khz));
    (ham && zlog_core::band_of(hz).is_some()).then_some(CallCommand::Qsy(hz))
}

pub const NR_AGAIN: u8 = 5;

pub const QUICK_ENTER: Duration = Duration::from_millis(500);

const SUGGESTED: [EntryField; 4] = [
    EntryField::RstSent,
    EntryField::RstRcvd,
    EntryField::Number,
    EntryField::Memo,
];

#[derive(Debug, Default)]
pub struct EnterFlow {
    enter: EnterSends,
    pending_log: Option<EntryField>,
    called: Option<QsoKey>,
    dupe_warned: Option<QsoKey>,
    thanked: Option<Instant>,
}

impl EnterFlow {
    pub fn load(&mut self, section: &Section) {
        self.enter = EnterSends::load(section);
    }

    pub fn sends(&self) -> &EnterSends {
        &self.enter
    }

    pub fn enter(&mut self, entry: &mut dyn EntryState, field: EntryField) -> Option<Cursor> {
        self.enter_at(entry, field, Instant::now())
    }

    pub fn enter_at(
        &mut self,
        entry: &mut dyn EntryState,
        field: EntryField,
        now: Instant,
    ) -> Option<Cursor> {
        if field == EntryField::Call {
            if let Some(c) = call_command(entry.text(EntryField::Call)) {
                match c {
                    CallCommand::Qsy(hz) => entry.qsy(hz),
                    CallCommand::Mode(m) => entry.set_mode(m),
                }
                entry.set_text(EntryField::Call, String::new());
                return Some(Cursor::to(EntryField::Call));
            }
        }
        if !self.enter.on {
            let (Ok(c) | Err(c)) = self.log(entry, field);
            return Some(c);
        }
        self.forget_other_qso(entry);
        let call = qso_key(entry);
        let cq = entry.cq();
        let step = self.step(entry);
        let e = self.enter.clone();
        let (message, cursor) = match step {
            Step::Nothing => (None, None),
            Step::Cq => {
                let bounce = self
                    .thanked
                    .is_some_and(|t| now.saturating_duration_since(t) < QUICK_ENTER);
                ((!bounce).then_some(Message::Cq), Some(EntryField::Call))
            }
            Step::Call => {
                let again = self.dupe_warned.is_some();
                let dupe = !again && entry.complete_call();
                if dupe {
                    self.dupe_warned = Some(call);
                } else {
                    self.called = Some(call);
                }
                (e.message(step, cq, dupe), Some(EntryField::Number))
            }
            Step::Log => {
                if let Err(why) = entry.check() {
                    return Some(not_logged(why, field));
                }
                self.pending_log = Some(field);
                (e.message(step, cq, false), None)
            }
        };
        if let Some(m) = message {
            entry.send(e.function_key(m));
        }
        cursor.map(Cursor::to)
    }

    pub fn messages_sent(&mut self, entry: &mut dyn EntryState) -> Option<Cursor> {
        let from = self.pending_log.take()?;
        let thanks = entry.cq();
        let logged = self.log(entry, from);
        if thanks && logged.is_ok() {
            self.thanked = Some(Instant::now());
        }
        let (Ok(c) | Err(c)) = logged;
        Some(c)
    }

    pub fn space(
        &mut self,
        entry: &mut dyn EntryState,
        field: Option<EntryField>,
    ) -> Option<Cursor> {
        match field? {
            EntryField::Memo => None,
            EntryField::Call => {
                entry.complete_call();
                Some(Cursor::to(EntryField::Number))
            }
            _ => Some(Cursor::to(EntryField::Call)),
        }
    }

    pub fn tab(&mut self, entry: &mut dyn EntryState) -> Option<Cursor> {
        let call = entry.text(EntryField::Call).trim().to_string();
        if call.is_empty() {
            return None;
        }
        entry.send(self.enter.exchange);
        if entry.mode() == Mode::Cw && (call.contains('.') || call.contains('?')) {
            return None;
        }
        entry.complete_call();
        self.called = Some(qso_key(entry));
        entry.tab_sent();
        Some(Cursor::to(EntryField::Number))
    }

    pub fn complete(&mut self, entry: &mut dyn EntryState, field: EntryField) -> Option<Cursor> {
        if entry.text(EntryField::Call).trim().is_empty() {
            return None;
        }
        match entry.check() {
            Ok(()) => {
                entry.send(self.enter.thanks);
                self.pending_log = Some(field);
                None
            }
            Err(why) => {
                if why == NotLogged::InvalidNumber {
                    entry.send(NR_AGAIN);
                }
                Some(not_logged(why, field))
            }
        }
    }

    pub fn force_enter(&mut self, entry: &mut dyn EntryState, field: EntryField) -> Option<Cursor> {
        if self.enter.on && field == EntryField::Call {
            return self.enter(entry, field);
        }
        match entry.log_forced() {
            Ok(()) => {
                self.new_qso();
                Some(Cursor::to(EntryField::Call))
            }
            Err(why) => Some(not_logged(why, field)),
        }
    }

    pub fn log_now(&mut self, entry: &mut dyn EntryState, field: EntryField) -> Cursor {
        let (Ok(c) | Err(c)) = self.log(entry, field);
        c
    }

    pub fn space_acts(field: Option<EntryField>) -> bool {
        !matches!(field, None | Some(EntryField::Memo))
    }

    pub fn clear(&mut self, entry: &mut dyn EntryState) -> Cursor {
        entry.clear();
        self.new_qso();
        Cursor::to(EntryField::Call)
    }

    fn step(&self, entry: &dyn EntryState) -> Step {
        let called = self.called.as_ref() == Some(&qso_key(entry));
        self.enter.step(
            entry.cq(),
            entry.text(EntryField::Call),
            entry.text(EntryField::Number),
            called,
        )
    }

    pub fn forget_other_qso(&mut self, entry: &dyn EntryState) {
        let key = qso_key(entry);
        let stale = |k: &Option<QsoKey>| k.as_ref().is_some_and(|k| key.0.is_empty() || *k != key);
        if stale(&self.called) {
            self.called = None;
        }
        if stale(&self.dupe_warned) {
            self.dupe_warned = None;
        }
    }

    pub fn new_qso(&mut self) {
        self.called = None;
        self.dupe_warned = None;
    }

    fn log(&mut self, entry: &mut dyn EntryState, field: EntryField) -> Result<Cursor, Cursor> {
        match entry.log() {
            Ok(()) => {
                self.new_qso();
                Ok(Cursor::to(EntryField::Call))
            }
            Err(why) => Err(not_logged(why, field)),
        }
    }

    pub fn next(&self, entry: &dyn EntryState) -> Option<Message> {
        let e = &self.enter;
        if !e.on {
            return None;
        }
        e.message(self.step(entry), entry.cq(), false)
    }

    pub fn guide(&self, entry: &dyn EntryState) -> Option<(Message, u8)> {
        let m = self
            .enter
            .message(self.step(entry), entry.cq(), entry.dupe())?;
        Some((m, self.enter.function_key(m)))
    }

    pub fn next_message(&self, entry: &dyn EntryState) -> Option<&'static str> {
        self.next(entry).map(|m| m.key())
    }

    pub fn thanked(&self) -> Option<Instant> {
        self.thanked
    }

    pub fn dupe_warned(&self) -> bool {
        self.dupe_warned.is_some()
    }
}

pub fn not_logged(why: NotLogged, field: EntryField) -> Cursor {
    match why {
        NotLogged::NoCallsign => Cursor::to(EntryField::Call),
        NotLogged::InvalidNumber => Cursor {
            field: EntryField::Number,
            select_all: true,
        },
        NotLogged::InvalidTime => Cursor {
            field: EntryField::Time,
            select_all: true,
        },
        NotLogged::OutOfPeriod | NotLogged::Refused => Cursor::to(field),
    }
}

pub fn accept_suggestions(entry: &mut dyn EntryState, field: Option<EntryField>) {
    match field {
        Some(EntryField::Call) => {
            for f in SUGGESTED {
                entry.accept_suggestion(f);
            }
        }
        Some(f) => {
            entry.accept_suggestion(f);
        }
        None => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntryStatus {
    pub dupe: bool,
    pub new_multi: Option<String>,
    pub cq: bool,
    pub next: Option<Message>,
    pub suggestions: Vec<Suggestion>,
}

impl EntryStatus {
    pub fn of(entry: &dyn EntryState, flow: &EnterFlow) -> Self {
        let suggestions = SUGGESTED
            .into_iter()
            .filter(|f| entry.text(*f).trim().is_empty())
            .filter_map(|f| entry.suggestion(f).cloned())
            .collect();
        EntryStatus {
            dupe: entry.dupe(),
            new_multi: entry.new_multi(),
            cq: entry.cq(),
            next: flow.next(entry),
            suggestions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::fake::Fake;

    fn flow(on: bool) -> EnterFlow {
        let mut f = EnterFlow::default();
        let mut s = Section::new();
        s.insert("enter_sends".into(), on.to_string());
        f.load(&s);
        f
    }

    fn enter(r: &mut EnterFlow, e: &mut Fake, field: EntryField) -> Option<Cursor> {
        let a = r.enter(e, field);
        r.messages_sent(e).or(a)
    }

    fn at(field: EntryField) -> Option<Cursor> {
        Some(Cursor::to(field))
    }

    #[test]
    fn frequencies_and_modes_typed_as_a_callsign_are_commands() {
        use CallCommand::{Mode as M, Qsy};
        for (text, want) in [
            ("7025", Some(Qsy(7_025_000))),
            ("14025.5", Some(Qsy(14_025_500))),
            ("3525", Some(Qsy(3_525_000))),
            ("1296100", Some(Qsy(1_296_100_000))),
            ("cw", Some(M(Mode::Cw))),
            ("PH", Some(M(Mode::Ssb))),
            ("RY", Some(M(Mode::Rtty))),
            ("DIG", Some(M(Mode::Rtty))),
            ("FT4", Some(M(Mode::Ft4))),
            ("FT8", Some(M(Mode::Ft8))),
            ("6900", None),
            ("7400", None),
            ("1", None),
            ("7025.0001", None),
            ("7.025", None),
            ("JA1ZLO", None),
            ("K1CW", None),
            ("CW1", None),
            ("", None),
        ] {
            assert_eq!(call_command(text), want, "{text}");
        }
    }

    #[test]
    fn enter_on_a_command_qsys_or_sets_the_mode_and_clears_the_call() {
        for on in [false, true] {
            let mut r = flow(on);
            let mut e = Fake {
                call: "14025.5".into(),
                cq: true,
                ..Default::default()
            };
            assert_eq!(r.enter(&mut e, EntryField::Call), at(EntryField::Call));
            assert_eq!(
                (e.band, e.call.as_str(), e.logged),
                (Some(Band::B14), "", 0)
            );
            e.call = "ssb".into();
            assert_eq!(r.enter(&mut e, EntryField::Call), at(EntryField::Call));
            assert_eq!((e.mode, e.call.as_str()), (Some(Mode::Ssb), ""));
            assert!(e.sent.is_empty(), "nothing sent");
        }
    }

    #[test]
    fn run_mode_calls_cq_sends_the_exchange_then_thanks_and_logs() {
        let mut r = flow(true);
        let mut e = Fake {
            cq: true,
            ..Default::default()
        };
        assert_eq!(r.next_message(&e), Some("cq"));
        assert_eq!(
            enter(&mut r, &mut e, EntryField::Call),
            at(EntryField::Call)
        );
        e.call = "JA1ZLO".into();
        assert_eq!(r.next_message(&e), Some("exchange"));
        assert_eq!(
            enter(&mut r, &mut e, EntryField::Call),
            at(EntryField::Number)
        );
        assert_eq!(e.logged, 0);
        e.number = "10M".into();
        assert_eq!(r.next_message(&e), Some("thanks"));
        assert_eq!(r.enter(&mut e, EntryField::Number), None);
        assert_eq!(e.sent, [1, 2, 3]);
        assert_eq!(e.logged, 0, "the entry is kept while the message starts");
        assert_eq!(r.messages_sent(&mut e), at(EntryField::Call));
        assert_eq!(e.logged, 1);
    }

    #[test]
    fn search_and_pounce_sends_my_call_then_the_exchange_and_logs() {
        let mut r = flow(true);
        let mut e = Fake::default();
        assert_eq!(r.next_message(&e), None);
        enter(&mut r, &mut e, EntryField::Call);
        assert!(e.sent.is_empty());
        e.call = "JA1ZLO".into();
        assert_eq!(r.next_message(&e), Some("my_call"));
        enter(&mut r, &mut e, EntryField::Call);
        e.number = "10M".into();
        enter(&mut r, &mut e, EntryField::Number);
        assert_eq!(e.sent, [7, 2]);
        assert_eq!(e.logged, 1);
    }

    #[test]
    fn a_dupe_warns_or_sends_worked_before() {
        for (dupe_sends, sent) in [(false, vec![]), (true, vec![4])] {
            let mut r = flow(true);
            r.enter.dupe_sends = dupe_sends;
            let mut e = Fake {
                cq: true,
                dupe: true,
                call: "JA1ZLO".into(),
                ..Default::default()
            };
            let c = enter(&mut r, &mut e, EntryField::Call);
            assert_eq!(e.sent, sent);
            assert_eq!(
                c,
                at(EntryField::Number),
                "the number is not typed into the call"
            );
            assert_eq!(e.logged, 0);
            assert!(r.dupe_warned());
        }
    }

    #[test]
    fn tab_sends_f2_and_moves_to_the_number() {
        let mut r = flow(false);
        let mut e = Fake::default();
        assert_eq!(r.tab(&mut e), None, "nothing on an empty call");
        assert!(e.sent.is_empty());
        e.call = "JA1ZLO".into();
        assert_eq!(r.tab(&mut e), at(EntryField::Number));
        assert_eq!((e.sent.as_slice(), e.tabbed), ([2].as_slice(), 1));
        e.call = "JA1Z?".into();
        assert_eq!(r.tab(&mut e), None, "a partial call stays");
        assert_eq!((e.sent.as_slice(), e.tabbed), ([2, 2].as_slice(), 1));
    }

    #[test]
    fn down_thanks_and_logs_or_asks_the_number_again() {
        let mut r = flow(false);
        let mut e = Fake::default();
        assert_eq!(r.complete(&mut e, EntryField::Call), None);
        e.call = "JA1ZLO".into();
        e.refuse = Some(NotLogged::InvalidNumber);
        assert_eq!(
            r.complete(&mut e, EntryField::Call),
            Some(not_logged(NotLogged::InvalidNumber, EntryField::Call))
        );
        assert_eq!(e.sent, [NR_AGAIN]);
        e.refuse = None;
        assert_eq!(r.complete(&mut e, EntryField::Number), None);
        assert_eq!(e.sent, [NR_AGAIN, 3]);
        assert_eq!(e.logged, 0);
        assert_eq!(r.messages_sent(&mut e), at(EntryField::Call));
        assert_eq!(e.logged, 1);
    }

    #[test]
    fn shift_enter_forces_the_log() {
        let mut r = flow(false);
        let mut e = Fake {
            call: "JA1ZLO".into(),
            refuse: Some(NotLogged::InvalidNumber),
            ..Default::default()
        };
        assert_eq!(
            r.force_enter(&mut e, EntryField::Call),
            at(EntryField::Call)
        );
        assert_eq!((e.forced, e.logged), (1, 1));
        let mut r = flow(true);
        e.call = "JA1ZLO".into();
        r.force_enter(&mut e, EntryField::Call);
        assert_eq!(e.forced, 1, "the call field of the enter mode only steps");
        assert_eq!(
            r.force_enter(&mut e, EntryField::Number),
            at(EntryField::Call)
        );
        assert_eq!(e.forced, 2);
    }

    #[test]
    fn off_enter_only_logs() {
        let mut r = flow(false);
        let mut e = Fake {
            cq: true,
            call: "JA1ZLO".into(),
            ..Default::default()
        };
        assert_eq!(r.next_message(&e), None);
        r.enter(&mut e, EntryField::Call);
        assert_eq!(e.logged, 1, "logged at once");
        assert!(e.sent.is_empty());
        r.enter(&mut e, EntryField::Call);
        assert!(e.sent.is_empty(), "no CQ on an empty callsign");
    }

    #[test]
    fn a_number_filled_before_the_call_step_does_not_log() {
        for (cq, call_key, log_key) in [(true, 2, 3), (false, 7, 2)] {
            let mut r = flow(true);
            let mut e = Fake {
                cq,
                call: "JA1ZLO".into(),
                number: "10M".into(),
                ..Default::default()
            };
            enter(&mut r, &mut e, EntryField::Number);
            assert_eq!(e.sent, [call_key], "cq {cq}");
            assert_eq!(e.logged, 0);
            enter(&mut r, &mut e, EntryField::Number);
            assert_eq!(e.sent, [call_key, log_key]);
            assert_eq!(e.logged, 1);
        }
    }

    #[test]
    fn editing_the_callsign_or_ctrl_k_starts_the_call_step_again() {
        let mut r = flow(true);
        let mut e = Fake {
            cq: true,
            call: "JA1ZLO".into(),
            number: "10M".into(),
            ..Default::default()
        };
        enter(&mut r, &mut e, EntryField::Call);
        e.call = "JA1ZLP".into();
        assert_eq!(r.next_message(&e), Some("exchange"));
        enter(&mut r, &mut e, EntryField::Call);
        assert_eq!(e.sent, [2, 2]);
        assert_eq!(e.logged, 0);
        assert_eq!(r.clear(&mut e), Cursor::to(EntryField::Call));
        assert!(e.call.is_empty());
        e.call = "JA1ZLP".into();
        e.number = "10M".into();
        assert_eq!(r.next_message(&e), Some("exchange"));
    }

    #[test]
    fn another_band_an_emptied_call_or_an_outside_change_is_a_new_qso() {
        let mut r = flow(true);
        let mut e = Fake {
            cq: true,
            call: "JA1ZLO".into(),
            number: "10M".into(),
            ..Default::default()
        };
        enter(&mut r, &mut e, EntryField::Call);
        e.band = Some(Band::B14);
        assert_eq!(r.next_message(&e), Some("exchange"), "band changed");
        e.band = None;
        assert_eq!(r.next_message(&e), Some("thanks"));
        r.forget_other_qso(&Fake::default());
        assert_eq!(r.next_message(&e), Some("exchange"), "call emptied");
        enter(&mut r, &mut e, EntryField::Call);
        r.new_qso();
        assert_eq!(r.next_message(&e), Some("exchange"), "outside change");
        e.dupe = true;
        enter(&mut r, &mut e, EntryField::Call);
        e.band = Some(Band::B14);
        r.forget_other_qso(&e);
        assert!(!r.dupe_warned(), "dupe reported again on another band");
    }

    #[test]
    fn a_dupe_is_logged_only_when_the_operator_goes_on() {
        let mut r = flow(true);
        let mut e = Fake {
            cq: true,
            dupe: true,
            call: "JA1ZLO".into(),
            number: "10M".into(),
            ..Default::default()
        };
        assert_eq!(
            enter(&mut r, &mut e, EntryField::Call),
            at(EntryField::Number)
        );
        assert!(e.sent.is_empty());
        assert_eq!(e.logged, 0, "the warning does not log");
        enter(&mut r, &mut e, EntryField::Number);
        assert_eq!(e.sent, [2], "Enter again goes on with the exchange");
        assert_eq!(e.logged, 0);
        enter(&mut r, &mut e, EntryField::Number);
        assert_eq!(e.sent, [2, 3]);
        assert_eq!(e.logged, 1);
    }

    #[test]
    fn nothing_is_sent_for_a_qso_that_would_be_refused() {
        let mut r = flow(true);
        let mut e = Fake {
            cq: true,
            call: "JA1ZLO".into(),
            ..Default::default()
        };
        enter(&mut r, &mut e, EntryField::Call);
        e.number = "XX".into();
        e.refuse = Some(NotLogged::InvalidNumber);
        let c = enter(&mut r, &mut e, EntryField::Number);
        assert_eq!(e.sent, [2], "no thanks for an invalid number");
        assert_eq!(e.logged, 0);
        assert_eq!(
            c,
            Some(Cursor {
                field: EntryField::Number,
                select_all: true
            })
        );
    }

    #[test]
    fn a_quick_second_enter_does_not_cut_the_thanks_off() {
        let mut r = flow(true);
        let mut e = Fake {
            cq: true,
            call: "JA1ZLO".into(),
            ..Default::default()
        };
        enter(&mut r, &mut e, EntryField::Call);
        e.number = "10M".into();
        enter(&mut r, &mut e, EntryField::Number);
        assert_eq!((e.sent.clone(), e.logged), (vec![2, 3], 1));
        let t = r.thanked().expect("thanks sent");
        r.enter_at(&mut e, EntryField::Call, t + Duration::from_millis(100));
        assert_eq!(e.sent, [2, 3], "no CQ over the thanks");
        r.enter_at(&mut e, EntryField::Call, t + Duration::from_secs(1));
        assert_eq!(e.sent, [2, 3, 1]);
    }

    #[test]
    fn space_completes_the_call_or_returns_to_it() {
        let mut r = flow(false);
        let mut e = Fake::default();
        assert_eq!(
            r.space(&mut e, Some(EntryField::Call)),
            at(EntryField::Number)
        );
        assert_eq!(
            r.space(&mut e, Some(EntryField::RstSent)),
            at(EntryField::Call)
        );
        assert_eq!(r.space(&mut e, Some(EntryField::Memo)), None);
        assert_eq!(r.space(&mut e, None), None);
        assert!(!EnterFlow::space_acts(Some(EntryField::Memo)));
        assert!(EnterFlow::space_acts(Some(EntryField::Number)));
    }

    #[test]
    fn not_logged_moves_the_cursor() {
        assert_eq!(
            not_logged(NotLogged::NoCallsign, EntryField::Memo).field,
            EntryField::Call
        );
        assert!(not_logged(NotLogged::InvalidTime, EntryField::Call).select_all);
        assert_eq!(
            not_logged(NotLogged::OutOfPeriod, EntryField::Memo).field,
            EntryField::Memo
        );
    }

    #[test]
    fn status_shows_dupe_next_message_and_suggestions_of_empty_fields() {
        let r = flow(true);
        let mut e = Fake {
            cq: true,
            dupe: true,
            call: "JA1ZLO".into(),
            ..Default::default()
        };
        e.suggest = Some(Suggestion::new(EntryField::Number, "10M", "JA1ZLO", ""));
        let s = EntryStatus::of(&e, &r);
        assert!(s.dupe && s.cq);
        assert_eq!(s.next, Some(Message::Exchange));
        assert_eq!(s.suggestions.len(), 1);
        e.number = "11M".into();
        assert!(
            EntryStatus::of(&e, &r).suggestions.is_empty(),
            "typed fields keep theirs"
        );
        accept_suggestions(&mut e, Some(EntryField::Call));
        assert_eq!(e.number, "11M");
        e.number.clear();
        accept_suggestions(&mut e, Some(EntryField::Call));
        assert_eq!(e.number, "10M");
    }
}
