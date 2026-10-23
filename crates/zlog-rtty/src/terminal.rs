//! The received text of the RTTY console (`TTTYConsole.RXChar`): lines, the
//! callsigns heard, newest first, and the number after a `599`.

/// The most lines kept; older ones scroll away.
const MAX_LINES: usize = 1000;

/// Lines break here when no CR or LF comes, as a terminal wraps.
const WIDTH: usize = 72;

/// The most callsigns kept in the list.
const MAX_CALLS: usize = 100;

/// Whether `word` looks like a callsign, the default filter of zLog
/// (`CallsignFilter`: 3 to 15 characters, a digit and a letter, letters and digits
/// in parts split by `/`).
pub fn is_callsign(word: &str) -> bool {
    (3..=15).contains(&word.len())
        && word.bytes().any(|b| b.is_ascii_digit())
        && word.bytes().any(|b| b.is_ascii_uppercase())
        && word.split('/').all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
        })
}

/// The number of a word holding `599` (`ExtractRcvdNumber`): what follows it, past
/// one of ` / - |`, up to a space, with `-` and `/` read as spaces.
pub fn number_after_599(word: &str) -> Option<String> {
    let i = word.find("599")?;
    let rest = word[i + 3..].trim_start();
    let rest = rest
        .strip_prefix([' ', '/', '-', '|'])
        .unwrap_or(rest)
        .replace(['-', '/'], " ");
    let n = rest.split(' ').next().unwrap_or("");
    (!n.is_empty()).then(|| n.to_string())
}

/// The text received and the callsigns found in it.
#[derive(Debug, Default)]
pub struct Terminal {
    pub lines: Vec<String>,
    /// The line being received.
    pub current: String,
    word: String,
    /// Callsigns heard, the newest first, each once.
    pub calls: Vec<String>,
}

impl Terminal {
    /// Take in a received character. A word ending while `listening` (not while
    /// sending) adds its callsign to the list, unless it is `my_call`, and gives
    /// the number after a `599`.
    pub fn push(&mut self, c: char, my_call: &str, listening: bool) -> Option<String> {
        match c {
            '\r' | '\n' => {
                // CR LF and LF CR make one line break; blank lines are not kept.
                if !self.current.is_empty() {
                    self.new_line();
                }
            }
            c if c.is_control() => return None,
            c => {
                if self.current.chars().count() >= WIDTH {
                    self.new_line();
                }
                self.current.push(c);
            }
        }
        if c > ' ' {
            self.word.push(c);
            return None;
        }
        let word = std::mem::take(&mut self.word);
        if !listening {
            return None;
        }
        if is_callsign(&word) && word != my_call {
            self.calls.retain(|w| w != &word);
            self.calls.insert(0, word.clone());
            self.calls.truncate(MAX_CALLS);
        }
        number_after_599(&word)
    }

    fn new_line(&mut self) {
        self.lines.push(std::mem::take(&mut self.current));
        if self.lines.len() > MAX_LINES {
            self.lines.drain(..self.lines.len() - MAX_LINES);
        }
    }

    pub fn clear_text(&mut self) {
        self.lines.clear();
        self.current.clear();
        self.word.clear();
    }

    /// The lines, the one being received last.
    pub fn all_lines(&self) -> impl Iterator<Item = &str> {
        self.lines
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(self.current.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receive(t: &mut Terminal, text: &str) -> Vec<String> {
        text.chars()
            .filter_map(|c| t.push(c, "JA1ZLO", true))
            .collect()
    }

    #[test]
    fn callsigns_by_zlogs_default_filter() {
        for c in ["JA1ZLO", "JA1ZLO/1", "7K1ABC/P", "W1AW", "3D2"] {
            assert!(is_callsign(c), "{c}");
        }
        for c in ["CQ", "TEST", "599", "JA", "13M/", "/P", "JA1ZLO//1", "AB-1"] {
            assert!(!is_callsign(c), "{c}");
        }
        assert!(!is_callsign("JA1ZLOABCDEFGHIJ"));
    }

    #[test]
    fn heard_callsigns_newest_first_without_my_own() {
        let mut t = Terminal::default();
        receive(
            &mut t,
            "CQ TEST JA1YAA JA1YAA\r\nJA1ZLO DE JR1ABC 599 13\r\n",
        );
        assert_eq!(t.calls, ["JR1ABC", "JA1YAA"]);
        receive(&mut t, "JA1YAA ");
        assert_eq!(t.calls, ["JA1YAA", "JR1ABC"]);
        // Not while sending.
        "JH1XYZ ".chars().for_each(|c| {
            t.push(c, "JA1ZLO", false);
        });
        assert_eq!(t.calls.len(), 2);
    }

    #[test]
    fn the_number_after_599_as_zlog_takes_it() {
        assert_eq!(number_after_599("599-13M"), Some("13M".into()));
        assert_eq!(number_after_599("599/100110M"), Some("100110M".into()));
        assert_eq!(number_after_599("5991313-x"), Some("1313".into()));
        assert_eq!(number_after_599("599"), None);
        assert_eq!(number_after_599("13M"), None);
        let mut t = Terminal::default();
        assert_eq!(receive(&mut t, "UR 599-13M 599-13M\r"), ["13M", "13M"]);
    }

    #[test]
    fn lines_break_once_and_wrap() {
        let mut t = Terminal::default();
        receive(&mut t, "AB\r\n\r\nCD\n\rEF");
        assert_eq!(t.all_lines().collect::<Vec<_>>(), ["AB", "CD", "EF"]);
        receive(&mut t, &"X".repeat(WIDTH * 2));
        assert_eq!(t.lines.last().unwrap().len(), WIDTH);
        t.clear_text();
        assert_eq!(t.all_lines().collect::<Vec<_>>(), [""]);
    }
}
