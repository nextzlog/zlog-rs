//! The Delphi string functions whose output the documents depend on.

/// Line break of `TStrings` and of the files zLog writes.
pub const CRLF: &str = "\r\n";

/// `TStrings.Text` of a memo: every line followed by CRLF, after the trailing
/// blank lines are removed (`RemoveBlankLines`).
pub fn memo_text(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines.iter().map(|l| format!("{l}{CRLF}")).collect()
}

/// `TStrings.CommaText`: items separated by commas; an item with a space, a
/// comma, a quote or a control character is quoted, its quotes doubled.
pub fn comma_text(items: &[&str]) -> String {
    let quote = |s: &&str| {
        if s.is_empty() || s.chars().any(|c| c <= ' ' || c == ',' || c == '"') {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    items.iter().map(quote).collect::<Vec<_>>().join(",")
}

/// `FloatToStr`: no decimals for whole numbers, otherwise the shortest form.
pub fn float_to_str(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        // Delphi keeps 15 significant digits.
        let s = format!("{:.*e}", 14, v);
        let parsed: f64 = s.parse().unwrap_or(v);
        format!("{parsed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memo_and_comma_text() {
        assert_eq!(memo_text("a\nb\n\n"), "a\r\nb\r\n");
        assert_eq!(memo_text(""), "");
        assert_eq!(
            comma_text(&["JA1ZLO", "JA1 YAA", "x,y"]),
            "JA1ZLO,\"JA1 YAA\",\"x,y\""
        );
    }

    #[test]
    fn floats() {
        assert_eq!(float_to_str(1234.0), "1234");
        assert_eq!(float_to_str(1.5), "1.5");
        assert_eq!(float_to_str(0.1 + 0.2), "0.3");
    }
}
