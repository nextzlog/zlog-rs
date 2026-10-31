pub mod menu;

pub use menu::IniFile;

use std::collections::BTreeMap;

pub type Sections = BTreeMap<String, BTreeMap<String, String>>;

pub fn decode(bytes: &[u8]) -> String {
    if let Some(utf8) = bytes.strip_prefix(BOM) {
        return String::from_utf8_lossy(utf8).into_owned();
    }
    encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned()
}

pub fn encode(text: &str) -> Vec<u8> {
    let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode(text);
    bytes.into_owned()
}

pub fn in_shift_jis(text: &str) -> bool {
    !encoding_rs::SHIFT_JIS.encode(text).2
}

pub fn read(bytes: &[u8]) -> Sections {
    let mut out = Sections::new();
    let mut current = String::new();
    for line in decode(bytes).lines().map(str::trim) {
        if line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let name = name.trim();
            current = out
                .keys()
                .find(|n| n.eq_ignore_ascii_case(name))
                .cloned()
                .unwrap_or_else(|| name.to_string());
        } else if let Some((k, v)) = line.split_once('=') {
            let section = out.entry(current.clone()).or_default();
            let k = k.trim();
            if !section.keys().any(|n| n.eq_ignore_ascii_case(k)) {
                section.insert(k.to_string(), v.trim().to_string());
            }
        }
    }
    out
}

pub fn fits(value: &str) -> bool {
    !value.contains(['\n', '\r'])
}

pub fn key_fits(key: &str) -> bool {
    let k = key.trim();
    !k.is_empty()
        && k == key
        && !k.contains('=')
        && !k.starts_with([';', '#', '['])
        && in_shift_jis(k)
}

pub fn merge(existing: &[u8], settings: &Sections) -> (Vec<u8>, usize) {
    let utf8 = existing.starts_with(BOM);
    let mut left_out = 0;
    let mut todo: BTreeMap<&str, BTreeMap<&str, &str>> = BTreeMap::new();
    for (name, section) in settings {
        let named = !name.is_empty() && !name.contains([']', '\n', '\r']) && in_shift_jis(name);
        for (k, v) in section {
            if named && fits(v) && key_fits(k) && (utf8 || in_shift_jis(v)) {
                todo.entry(name).or_default().insert(k, v);
            } else {
                left_out += 1;
            }
        }
    }
    let enc = |text: &str| -> Vec<u8> {
        if utf8 {
            text.as_bytes().to_vec()
        } else {
            encode_lossy(text)
        }
    };
    let newline: &[u8] = if existing.windows(2).any(|w| w == b"\r\n")
        || existing.strip_prefix(BOM).unwrap_or(existing).is_empty()
    {
        b"\r\n"
    } else {
        b"\n"
    };
    let blank = |l: &[u8]| l.iter().all(u8::is_ascii_whitespace);
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut current: Option<String> = None;
    let flush = |out: &mut Vec<Vec<u8>>,
                 todo: &mut BTreeMap<&str, BTreeMap<&str, &str>>,
                 name: &Option<String>| {
        if let Some(rest) = name.as_deref().and_then(|n| take(todo, n)) {
            let at = out
                .iter()
                .rposition(|l| !blank(l))
                .map_or(out.len(), |i| i + 1);
            let lines: Vec<Vec<u8>> = rest
                .iter()
                .map(|(k, v)| [enc(&format!("{k}={v}")), newline.to_vec()].concat())
                .collect();
            out.splice(at..at, lines);
        }
    };
    let body = existing.strip_prefix(BOM).unwrap_or(existing);
    let mut raw_lines: Vec<&[u8]> = body.split_inclusive(|&b| b == b'\n').collect();
    if let Some(first) = raw_lines.first_mut().filter(|_| utf8) {
        *first = &existing[..BOM.len() + first.len()];
    }
    for raw in raw_lines {
        let content = raw.strip_suffix(b"\n").unwrap_or(raw);
        let content = content.strip_suffix(b"\r").unwrap_or(content);
        let mut raw = raw.to_vec();
        if !raw.ends_with(b"\n") {
            raw.extend_from_slice(newline);
        }
        let text = if utf8 {
            String::from_utf8_lossy(content.strip_prefix(BOM).unwrap_or(content))
        } else {
            encoding_rs::SHIFT_JIS
                .decode_without_bom_handling(content)
                .0
        };
        let trimmed = text.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            flush(&mut out, &mut todo, &current);
            current = Some(name.trim().to_string());
            out.push(raw);
            continue;
        }
        if let (Some(name), Some((k, old))) = (&current, trimmed.split_once('=')) {
            if !trimmed.starts_with(';') && !trimmed.starts_with('#') {
                let section = todo
                    .iter_mut()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name))
                    .map(|(_, s)| s);
                if let Some(v) = section.and_then(|s| take(s, k.trim())) {
                    if old.trim() == v {
                        out.push(raw);
                        continue;
                    }
                    let end = &raw[content.len()..];
                    out.push([enc(&format!("{}={v}", k.trim())), end.to_vec()].concat());
                    continue;
                }
            }
        }
        out.push(raw);
    }
    flush(&mut out, &mut todo, &current);
    for (name, rest) in todo {
        if rest.is_empty() {
            continue;
        }
        if out.last().is_some_and(|l| !blank(l)) {
            out.push(newline.to_vec());
        }
        out.push([enc(&format!("[{name}]")), newline.to_vec()].concat());
        out.extend(
            rest.iter()
                .map(|(k, v)| [enc(&format!("{k}={v}")), newline.to_vec()].concat()),
        );
    }
    let bytes = out.concat();
    if utf8 && !bytes.starts_with(BOM) {
        return ([BOM, &bytes].concat(), left_out);
    }
    (bytes, left_out)
}

fn encode_lossy(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut buf = [0; 4];
    for c in text.chars() {
        let (b, _, bad) = encoding_rs::SHIFT_JIS.encode(c.encode_utf8(&mut buf));
        if bad {
            out.push(b'?');
        } else {
            out.extend_from_slice(&b);
        }
    }
    out
}

const BOM: &[u8] = b"\xEF\xBB\xBF";

fn take<V>(map: &mut BTreeMap<&str, V>, name: &str) -> Option<V> {
    let k = *map.keys().find(|k| k.eq_ignore_ascii_case(name))?;
    map.remove(k)
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.settings_file(10, IniFile::default);
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

    #[test]
    fn untouched_lines_keep_their_bytes() {
        let old = b"[a]\r\nx=\x82\r\nmemo=\x81\x7f\xfd\r\nk=old\nname=\xe6\x9d\xb1\r\n";
        let (bytes, left_out) = merge(
            old,
            &sections(&[
                ("a", "k", "new"),
                ("a", "x", "\u{FFFD}"),
                ("a", "name", "a\u{1F600}b"),
            ]),
        );
        assert_eq!(left_out, 2);
        assert_eq!(
            bytes,
            b"[a]\r\nx=\x82\r\nmemo=\x81\x7f\xfd\r\nk=new\nname=\xe6\x9d\xb1\r\n"
        );
        let (bytes, _) = merge(b"\xEF\xBB\xBF", &sections(&[("a", "k", "\u{1F600}")]));
        assert_eq!(bytes, "\u{feff}[a]\r\nk=\u{1F600}\r\n".as_bytes());
    }

    #[test]
    fn read_ignores_case_and_the_first_key_wins() {
        let s = read(b"[Categories]\r\nCall=JA1A\r\n[categories]\r\ncall=JA1B\r\nPower=H\r\n");
        assert_eq!(s.len(), 1);
        assert_eq!(s["Categories"].len(), 2);
        assert_eq!(s["Categories"]["Call"], "JA1A");
    }
}
