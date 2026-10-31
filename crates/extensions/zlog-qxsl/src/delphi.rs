pub const CRLF: &str = "\r\n";

pub fn ansi(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut buf = [0u8; 4];
    for c in s.chars() {
        let (bytes, _, bad) = encoding_rs::SHIFT_JIS.encode(c.encode_utf8(&mut buf));
        if bad {
            out.push(b'?');
        } else {
            out.extend_from_slice(&bytes);
        }
    }
    out
}

pub fn fill_right(s: &str, len: usize) -> Vec<u8> {
    let mut b = ansi(s);
    b.resize(len, b' ');
    b
}

pub fn fill_left(s: &str, len: usize) -> Vec<u8> {
    let mut b = vec![b' '; len];
    b.extend(ansi(s));
    b.split_off(b.len() - len)
}

pub fn date_time(t: f64) -> (i32, u32, u32, u32, u32) {
    let ms = (t.fract().abs() * 86_400_000.0).round() as i64;
    let day = zlog_core::DelphiTime(t.trunc());
    let (y, m, d, ..) = day.to_ymd_hms();
    let ms = ms.min(86_399_999);
    (y, m, d, (ms / 3_600_000) as u32, (ms / 60_000 % 60) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delphi_padding_on_shift_jis_bytes() {
        assert_eq!(ansi("JA1ZLO\u{1F600}"), b"JA1ZLO?");
        assert_eq!(ansi("東京"), [0x93, 0x8C, 0x8B, 0x9E]);
        assert_eq!(fill_right("ABCDEFG", 3), b"ABC");
        assert_eq!(fill_left("10.1G", 4), b"0.1G");
        assert_eq!(fill_left("7", 4), b"   7");
    }

    #[test]
    fn date_time_truncates_minutes_like_decode_time() {
        let t = zlog_core::DelphiTime::from_ymd_hms(2026, 11, 28, 23, 59, 59).0 + 0.6 / 86400.0;
        assert_eq!(date_time(t), (2026, 11, 28, 23, 59));
        let t = zlog_core::DelphiTime::from_ymd_hms(2026, 11, 28, 1, 15, 0).0;
        assert_eq!(date_time(t), (2026, 11, 28, 1, 15));
    }
}
