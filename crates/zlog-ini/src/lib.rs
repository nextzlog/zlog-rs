//! zLog's `zlog.ini` read into and written from the settings of zLog RS.
//!
//! The conversion knows nothing of what the settings mean: a section of the
//! settings is a section of the INI file and a key is a key, both ways. Keeping
//! zlog.ini compatible is each extension's own business, by naming its section
//! and keys as zLog does.
//!
//! zlog.ini is Shift_JIS (Windows code page 932). Export merges into an existing
//! file: lines it does not change, comments included, stay as they are, and the
//! values it sets replace those of the same section and key. Values an INI line
//! cannot hold (with a line break) are left out.

use std::collections::BTreeMap;

/// Settings: sections by name, each with its values by key.
pub type Sections = BTreeMap<String, BTreeMap<String, String>>;

/// Text of an INI file in Shift_JIS (UTF-8 when it starts with a byte-order mark).
pub fn decode(bytes: &[u8]) -> String {
    if let Some(utf8) = bytes.strip_prefix(BOM) {
        return String::from_utf8_lossy(utf8).into_owned();
    }
    encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned()
}

/// `text` in Shift_JIS. Characters it has no code for become numeric character
/// references (`&#128512;`); [`fits`] keeps such values out of [`merge`].
pub fn encode(text: &str) -> Vec<u8> {
    let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode(text);
    bytes.into_owned()
}

/// Whether Shift_JIS has a code for every character of `text`.
pub fn in_shift_jis(text: &str) -> bool {
    !encoding_rs::SHIFT_JIS.encode(text).2
}

/// The sections of an INI file. Lines without `=` and comments (`;`, `#`) are
/// skipped; a key given twice keeps its first value, as Windows'
/// `GetPrivateProfileString` (Delphi's `TIniFile`) reads it.
pub fn read(bytes: &[u8]) -> Sections {
    let mut out = Sections::new();
    let mut current = String::new();
    for line in decode(bytes).lines().map(str::trim) {
        if line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.trim().to_string();
        } else if let Some((k, v)) = line.split_once('=') {
            out.entry(current.clone())
                .or_default()
                .entry(k.trim().to_string())
                .or_insert_with(|| v.trim().to_string());
        }
    }
    out
}

/// Whether an INI line can hold `value` (one line, in Shift_JIS).
pub fn fits(value: &str) -> bool {
    !value.contains(['\n', '\r']) && in_shift_jis(value)
}

/// Whether `key` reads back as a key of an INI line (not empty, no `=`, not
/// taken for a comment or a section header, in Shift_JIS).
pub fn key_fits(key: &str) -> bool {
    let k = key.trim();
    !k.is_empty()
        && k == key
        && !k.contains('=')
        && !k.starts_with([';', '#', '['])
        && in_shift_jis(k)
}

/// `existing` (an INI file, empty for a new one) with the values of `settings`
/// set, in Shift_JIS. Returns the file and how many values were left out.
pub fn merge(existing: &[u8], settings: &Sections) -> (Vec<u8>, usize) {
    let mut left_out = 0;
    // What is still to be written, by section.
    let mut todo: BTreeMap<&str, BTreeMap<&str, &str>> = BTreeMap::new();
    for (name, section) in settings {
        // Keys outside any section, and sections an INI header cannot name.
        let named = !name.is_empty() && !name.contains([']', '\n', '\r']) && in_shift_jis(name);
        for (k, v) in section {
            if named && fits(v) && key_fits(k) {
                todo.entry(name).or_default().insert(k, v);
            } else {
                left_out += 1;
            }
        }
    }
    let text = decode(existing);
    let newline = if text.contains("\r\n") || text.is_empty() {
        "\r\n"
    } else {
        "\n"
    };
    let mut out: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    let flush = |out: &mut Vec<String>,
                 todo: &mut BTreeMap<&str, BTreeMap<&str, &str>>,
                 name: &Option<String>| {
        if let Some(rest) = name.as_deref().and_then(|n| take(todo, n)) {
            // Before the blank lines that end the section.
            let at = out
                .iter()
                .rposition(|l| !l.trim().is_empty())
                .map_or(out.len(), |i| i + 1);
            let lines: Vec<String> = rest.iter().map(|(k, v)| format!("{k}={v}")).collect();
            out.splice(at..at, lines);
        }
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            flush(&mut out, &mut todo, &current);
            current = Some(name.trim().to_string());
            out.push(line.to_string());
            continue;
        }
        if let (Some(name), Some((k, _))) = (&current, trimmed.split_once('=')) {
            if !trimmed.starts_with(';') && !trimmed.starts_with('#') {
                let section = todo
                    .iter_mut()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name))
                    .map(|(_, s)| s);
                if let Some(v) = section.and_then(|s| take(s, k.trim())) {
                    out.push(format!("{}={v}", k.trim()));
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    flush(&mut out, &mut todo, &current);
    for (name, rest) in todo {
        if rest.is_empty() {
            continue;
        }
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push(format!("[{name}]"));
        out.extend(rest.iter().map(|(k, v)| format!("{k}={v}")));
    }
    let mut text = out.join(newline);
    text.push_str(newline);
    if existing.starts_with(BOM) {
        // Stays UTF-8, so no character of the lines left alone is lost.
        return ([BOM, text.as_bytes()].concat(), left_out);
    }
    (encode(&text), left_out)
}

/// The UTF-8 byte-order mark.
const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Remove the entry of `map` named `name` in any case, as Windows matches the
/// sections and keys of an INI file (`TMemIniFile` too).
fn take<V>(map: &mut BTreeMap<&str, V>, name: &str) -> Option<V> {
    let k = *map.keys().find(|k| k.eq_ignore_ascii_case(name))?;
    map.remove(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sections(list: &[(&str, &str, &str)]) -> Sections {
        let mut s = Sections::new();
        for (n, k, v) in list {
            s.entry(n.to_string())
                .or_default()
                .insert(k.to_string(), v.to_string());
        }
        s
    }

    #[test]
    fn reads_shift_jis_sections_and_keys() {
        let ini = encode("; zLog\r\n[Categories]\r\nCall=JA1ZLO\r\nName = 東京\r\n\r\n[Z-Link]\r\nPCName=PC1\r\n");
        let s = read(&ini);
        assert_eq!(s["Categories"]["Name"], "東京");
        assert_eq!(s["Z-Link"]["PCName"], "PC1");
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn merge_keeps_other_lines_and_adds_new_ones() {
        let old =
            encode("; comment\r\n[Categories]\r\nCall=JA1ZLO\r\nPower=H\r\n\r\n[Other]\r\nx=1\r\n");
        let new = sections(&[
            ("Categories", "Call", "JA1YAA"),
            ("Categories", "Op", "太郎"),
            ("Z-Link", "PCName", "PC2"),
            ("keyer", "pad", "CQ\nTEST"),
        ]);
        let (bytes, left_out) = merge(&old, &new);
        assert_eq!(left_out, 1);
        assert_eq!(
            decode(&bytes),
            "; comment\r\n[Categories]\r\nCall=JA1YAA\r\nPower=H\r\nOp=太郎\r\n\r\n[Other]\r\nx=1\r\n\r\n[Z-Link]\r\nPCName=PC2\r\n"
        );
        // Read back, the values are there.
        let back = read(&bytes);
        assert_eq!(back["Categories"]["Op"], "太郎");
        assert_eq!(back["Other"]["x"], "1");
    }

    #[test]
    fn what_an_ini_file_cannot_hold_is_left_out() {
        let new = sections(&[
            ("a", "emoji", "😀"),
            ("a", ";k", "v"),
            ("a", "ok", "東京"),
            ("", "top", "v"),
        ]);
        let (bytes, left_out) = merge(b"", &new);
        assert_eq!(left_out, 3);
        assert_eq!(decode(&bytes), "[a]\r\nok=東京\r\n");
    }

    #[test]
    fn a_new_file_has_every_section() {
        let (bytes, _) = merge(b"", &sections(&[("a", "k", "v"), ("b", "k", "w")]));
        assert_eq!(decode(&bytes), "[a]\r\nk=v\r\n\r\n[b]\r\nk=w\r\n");
    }

    /// Regression G12: a section or key written in another case was added again
    /// (zLog reads the first), and a UTF-8 file became Shift_JIS, its characters
    /// outside Shift_JIS turned into `&#NNNN;`.
    #[test]
    fn merge_matches_any_case_and_keeps_utf8() {
        let (bytes, _) = merge(
            b"[categories]\r\ncall=JA1AAA\r\n",
            &sections(&[("Categories", "Call", "JA1ZLO")]),
        );
        assert_eq!(decode(&bytes), "[categories]\r\ncall=JA1ZLO\r\n");
        let old = "\u{feff}; \u{1F600}\n[a]\nk=v\n";
        let (bytes, _) = merge(old.as_bytes(), &sections(&[("a", "k", "w")]));
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "\u{feff}; \u{1F600}\n[a]\nk=w\n"
        );
    }

    #[test]
    fn utf8_with_a_byte_order_mark_is_read_too() {
        let s = read("\u{feff}[a]\nk=東京\n".as_bytes());
        assert_eq!(s["a"]["k"], "東京");
    }
}
