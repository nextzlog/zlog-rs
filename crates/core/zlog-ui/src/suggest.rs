use crate::EntryField;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub field: EntryField,
    pub text: String,
    pub callsign: String,
    pub source: String,
    pub from_call: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agreement {
    Empty,
    Same,
    Differs,
}

impl Suggestion {
    pub fn new(
        field: EntryField,
        text: impl Into<String>,
        callsign: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        Suggestion {
            field,
            text: text.into(),
            callsign: callsign.into(),
            source: source.into(),
            from_call: String::new(),
        }
    }

    pub fn from_call(mut self, call: impl Into<String>) -> Self {
        self.from_call = call.into();
        self
    }

    pub fn agreement(&self, typed: &str) -> Agreement {
        let typed = typed.trim();
        if typed.is_empty() {
            Agreement::Empty
        } else if typed.eq_ignore_ascii_case(self.text.trim()) {
            Agreement::Same
        } else {
            Agreement::Differs
        }
    }

    pub fn is_for(&self, callsign: &str) -> bool {
        !self.callsign.is_empty() && self.callsign.eq_ignore_ascii_case(callsign.trim())
    }
}

pub fn find<'a>(
    list: &'a [Suggestion],
    field: EntryField,
    callsign: &str,
) -> Option<&'a Suggestion> {
    list.iter().find(|s| s.field == field && s.is_for(callsign))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_is_compared_ignoring_case_and_blanks() {
        let s = Suggestion::new(EntryField::Number, "10M", "JA1ZLO", "a.zlox");
        assert_eq!(s.agreement(" "), Agreement::Empty);
        assert_eq!(s.agreement("10m "), Agreement::Same);
        assert_eq!(s.agreement("11M"), Agreement::Differs);
    }

    #[test]
    fn found_only_for_its_field_and_callsign() {
        let list = [
            Suggestion::new(EntryField::Memo, "TARO", "JA1ZLO", ""),
            Suggestion::new(EntryField::Number, "10M", "JA1ZLO", ""),
        ];
        let n = find(&list, EntryField::Number, "ja1zlo").unwrap();
        assert_eq!(n.text, "10M");
        assert!(find(&list, EntryField::Number, "JA1ZL").is_none());
        assert!(find(&list, EntryField::RstRcvd, "JA1ZLO").is_none());
        assert!(!Suggestion::new(EntryField::Number, "1", "", "").is_for(""));
    }
}
