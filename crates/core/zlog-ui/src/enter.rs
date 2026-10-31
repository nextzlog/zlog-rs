use crate::Section;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnterSends {
    pub on: bool,
    pub cq: u8,
    pub exchange: u8,
    pub thanks: u8,
    pub my_call: u8,
    pub dupe_sends: bool,
    pub worked_before: u8,
}

impl Default for EnterSends {
    fn default() -> Self {
        EnterSends {
            on: false,
            cq: 1,
            exchange: 2,
            thanks: 3,
            my_call: 7,
            dupe_sends: false,
            worked_before: 4,
        }
    }
}

pub const SECTION: &str = "entry";

pub const KEYS: [&str; 5] = ["cq", "exchange", "thanks", "my_call", "worked_before"];

impl EnterSends {
    pub fn load(s: &Section) -> Self {
        let d = EnterSends::default();
        let flag = |key: &str, default: bool| s.get(key).map_or(default, |v| v == "true");
        let mut e = EnterSends {
            on: flag("enter_sends", d.on),
            dupe_sends: flag("dupe_sends", d.dupe_sends),
            ..d
        };
        for key in KEYS {
            if let Some(n) = s.get(key).and_then(|v| v.parse::<u8>().ok()) {
                if (1..=12).contains(&n) {
                    *e.key_mut(key) = n;
                }
            }
        }
        e
    }

    pub fn save(&self, s: &mut Section) {
        s.insert("enter_sends".into(), self.on.to_string());
        s.insert("dupe_sends".into(), self.dupe_sends.to_string());
        for key in KEYS {
            s.insert(key.into(), self.key(key).to_string());
        }
    }

    pub fn key(&self, key: &str) -> u8 {
        match key {
            "cq" => self.cq,
            "exchange" => self.exchange,
            "thanks" => self.thanks,
            "my_call" => self.my_call,
            _ => self.worked_before,
        }
    }

    pub fn key_mut(&mut self, key: &str) -> &mut u8 {
        match key {
            "cq" => &mut self.cq,
            "exchange" => &mut self.exchange,
            "thanks" => &mut self.thanks,
            "my_call" => &mut self.my_call,
            _ => &mut self.worked_before,
        }
    }

    pub fn step(&self, cq: bool, call: &str, number: &str, called: bool) -> Step {
        match (call.trim().is_empty(), number.trim().is_empty()) {
            (true, _) if cq => Step::Cq,
            (true, _) => Step::Nothing,
            (false, false) if called => Step::Log,
            (false, _) => Step::Call,
        }
    }

    pub fn message(&self, step: Step, cq: bool, dupe: bool) -> Option<Message> {
        match step {
            Step::Nothing => None,
            Step::Cq => Some(Message::Cq),
            Step::Call if dupe => self.dupe_sends.then_some(Message::WorkedBefore),
            Step::Call if cq => Some(Message::Exchange),
            Step::Call => Some(Message::MyCall),
            Step::Log if cq => Some(Message::Thanks),
            Step::Log => Some(Message::Exchange),
        }
    }

    pub fn function_key(&self, m: Message) -> u8 {
        match m {
            Message::Cq => self.cq,
            Message::Exchange => self.exchange,
            Message::Thanks => self.thanks,
            Message::MyCall => self.my_call,
            Message::WorkedBefore => self.worked_before,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Nothing,
    Cq,
    Call,
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    Cq,
    Exchange,
    Thanks,
    MyCall,
    WorkedBefore,
}

impl Message {
    pub fn key(self) -> &'static str {
        match self {
            Message::Cq => "cq",
            Message::Exchange => "exchange",
            Message::Thanks => "thanks",
            Message::MyCall => "my_call",
            Message::WorkedBefore => "worked_before",
        }
    }
}

pub fn message_name(language: &str, key: &str) -> String {
    crate::texts().get(language, &format!("enter.{key}")).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> EnterSends {
        EnterSends {
            on: true,
            ..Default::default()
        }
    }

    #[test]
    fn run_sequence() {
        let e = on();
        let steps = [
            ("", "", false),
            ("JA1ZLO", "", false),
            ("JA1ZLO", "10M", true),
        ];
        let got: Vec<_> = steps
            .iter()
            .map(|(c, n, k)| e.message(e.step(true, c, n, *k), true, false))
            .collect();
        assert_eq!(
            got,
            [
                Some(Message::Cq),
                Some(Message::Exchange),
                Some(Message::Thanks)
            ]
        );
        assert_eq!(e.step(true, "JA1ZLO", "10M", true), Step::Log);
    }

    #[test]
    fn search_and_pounce_sequence() {
        let e = on();
        assert_eq!(e.step(false, "", "", false), Step::Nothing);
        assert_eq!(e.message(Step::Nothing, false, false), None);
        let call = e.message(e.step(false, "JA1ZLO", "", false), false, false);
        assert_eq!(call, Some(Message::MyCall));
        let log = e.message(e.step(false, "JA1ZLO", "10M", true), false, false);
        assert_eq!(log, Some(Message::Exchange));
    }

    #[test]
    fn a_number_logs_only_after_the_call_step() {
        let e = on();
        for cq in [true, false] {
            assert_eq!(e.step(cq, "JA1ZLO", "10M", false), Step::Call);
            assert_eq!(e.step(cq, "JA1ZLO", "", true), Step::Call);
            assert_eq!(e.step(cq, "JA1ZLO", "10M", true), Step::Log);
        }
    }

    #[test]
    fn dupes_warn_or_send_worked_before() {
        let mut e = on();
        assert_eq!(e.message(Step::Call, true, true), None);
        assert_eq!(e.message(Step::Call, false, true), None);
        e.dupe_sends = true;
        assert_eq!(
            e.message(Step::Call, true, true),
            Some(Message::WorkedBefore)
        );
        assert_eq!(e.function_key(Message::WorkedBefore), 4);
    }

    #[test]
    fn default_keys_follow_the_keyer_bank() {
        let e = EnterSends::default();
        assert!(!e.on);
        let keys = [
            Message::Cq,
            Message::Exchange,
            Message::Thanks,
            Message::MyCall,
        ];
        assert_eq!(keys.map(|m| e.function_key(m)), [1, 2, 3, 7]);
    }

    #[test]
    fn settings_round_trip_and_reject_bad_keys() {
        let mut e = on();
        e.thanks = 9;
        e.dupe_sends = true;
        let mut s = Section::new();
        e.save(&mut s);
        assert_eq!(EnterSends::load(&s), e);
        s.insert("cq".into(), "13".into());
        assert_eq!(EnterSends::load(&s).cq, 1);
        assert_eq!(EnterSends::load(&Section::new()), EnterSends::default());
    }
}
