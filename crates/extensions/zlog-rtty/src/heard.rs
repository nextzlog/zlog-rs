use regex::Regex;
use rtty_ui::Heard;

pub const DEFAULT_CALLSIGN: &str = "[A-Z0-9]{0,2}[A-Z][0-9]+[A-Z]{1,4}";

fn anchored(pattern: &str) -> Result<Regex, String> {
    Regex::new(&format!("^(?:{})$", pattern.trim())).map_err(|e| e.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filters {
    pub callsign: String,
    pub number: String,
}

impl Default for Filters {
    fn default() -> Self {
        Filters {
            callsign: DEFAULT_CALLSIGN.into(),
            number: String::new(),
        }
    }
}

pub struct Matcher {
    callsign: Regex,
    number: Option<Regex>,
}

impl Filters {
    pub fn callsign_error(&self) -> Option<String> {
        anchored(&self.callsign).err()
    }

    pub fn number_error(&self) -> Option<String> {
        anchored(&self.number).err()
    }

    pub fn matcher(&self) -> Result<Matcher, String> {
        let callsign = match self.callsign.trim() {
            "" => anchored(DEFAULT_CALLSIGN)?,
            p => anchored(p)?,
        };
        let number = match self.number.trim() {
            "" => None,
            p => Some(anchored(p)?),
        };
        Ok(Matcher { callsign, number })
    }
}

impl Matcher {
    pub fn pass(&self, heard: &Heard) -> Option<(String, String)> {
        let call = heard.callsign.clone()?;
        if !call.split('/').any(|p| self.callsign.is_match(p)) {
            return None;
        }
        if heard.exchange.is_empty() {
            return Some((call, String::new()));
        }
        let ok = |s: &str| self.number.as_ref().is_none_or(|re| re.is_match(s));
        let joined = heard.exchange.join(" ");
        let number = match ok(&joined) {
            true => Some(joined),
            false => heard.exchange.iter().rev().find(|w| ok(w)).cloned(),
        }?;
        Some((call, number))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(text: &str) -> Heard {
        let words: Vec<String> = text.split_whitespace().map(String::from).collect();
        rtty_ui::heard::parse(&words, "JA1ZLO")
    }

    #[test]
    fn filters_pass_calls_and_numbers() {
        let m = Filters::default().matcher().unwrap();
        assert_eq!(
            m.pass(&heard("JA1YWX 599 001 001")),
            Some(("JA1YWX".into(), "001".into()))
        );
        assert_eq!(
            m.pass(&heard("CQ DE JA1YWX/1 JA1YWX/1")),
            Some(("JA1YWX/1".into(), String::new()))
        );
        assert_eq!(m.pass(&heard("599 001")), None);
        let f = Filters {
            callsign: "JA[0-9][A-Z]{3}".into(),
            number: "[0-9]{2}[A-Z]".into(),
        };
        let m = f.matcher().unwrap();
        assert_eq!(m.pass(&heard("JH1ABC 599 13M")), None);
        assert_eq!(m.pass(&heard("JA1YWX 599 001")), None);
        assert_eq!(
            m.pass(&heard("JA1YWX 599 001 13M")),
            Some(("JA1YWX".into(), "13M".into()))
        );
    }

    #[test]
    fn invalid_patterns_are_reported() {
        let f = Filters {
            callsign: "[A-Z".into(),
            number: "(1".into(),
        };
        assert!(f.callsign_error().is_some() && f.number_error().is_some());
        assert!(f.matcher().is_err());
        assert!(Filters::default().callsign_error().is_none());
        assert!(Filters::default().number_error().is_none());
    }
}
