use encoding_rs::SHIFT_JIS;
use zlog_core::qso::QslState;
use zlog_core::zlo::{read_records, write_records_with, ZLO_RECORD};
use zlog_core::{Format, Qso};

use crate::protocol::Record;

pub const RESPONSE_CAPACITY: usize = 256;

pub fn to_record(q: &Qso) -> Record {
    let bytes = write_records_with(std::slice::from_ref(q), Format::Zlo, QslState::None);
    bytes[..ZLO_RECORD].try_into().unwrap()
}

pub fn from_record(r: &Record) -> Qso {
    read_records(r)
        .ok()
        .and_then(|rs| rs.records.into_iter().next())
        .unwrap_or_default()
}

pub fn to_plugin(s: &str) -> std::ffi::CString {
    let mut bytes = Vec::with_capacity(s.len() + 1);
    for c in s.chars() {
        if c == '\0' {
            continue;
        }
        let mut buf = [0u8; 4];
        let (enc, _, bad) = SHIFT_JIS.encode(c.encode_utf8(&mut buf));
        if bad {
            bytes.push(b'?');
        } else {
            bytes.extend_from_slice(&enc);
        }
    }
    std::ffi::CString::new(bytes).unwrap()
}

pub fn from_plugin(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

pub fn query_answer(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for c in s.chars() {
        let one = to_plugin(&c.to_string());
        let b = one.as_bytes();
        if out.len() + b.len() > RESPONSE_CAPACITY {
            break;
        }
        out.extend_from_slice(b);
    }
    out
}

pub mod commit {
    pub const CALL: u64 = 1 << 0;
    pub const SENT: u64 = 1 << 1;
    pub const RCVD: u64 = 1 << 2;
    pub const NAME: u64 = 1 << 3;
    pub const NOTE: u64 = 1 << 4;
    pub const MUL1: u64 = 1 << 5;
    pub const MUL2: u64 = 1 << 6;
}

pub fn apply_commit(base: &Qso, patch: &Qso, mask: u64) -> Qso {
    let mut q = base.clone();
    if mask & commit::CALL != 0 {
        q.callsign = patch.callsign.clone();
    }
    if mask & commit::SENT != 0 {
        q.nr_sent = patch.nr_sent.clone();
    }
    if mask & commit::RCVD != 0 {
        q.nr_rcvd = patch.nr_rcvd.clone();
    }
    if mask & commit::NAME != 0 {
        q.operator = patch.operator.clone();
    }
    if mask & commit::NOTE != 0 {
        q.memo = patch.memo.clone();
    }
    if mask & commit::MUL1 != 0 {
        q.multi1 = patch.multi1.clone();
    }
    if mask & commit::MUL2 != 0 {
        q.multi2 = patch.multi2.clone();
    }
    q
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::{Band, DelphiTime, Mode};

    #[test]
    fn record_layout_is_tqsodata() {
        let q = Qso {
            time: DelphiTime(45000.5),
            callsign: "JA1ZLO".into(),
            nr_sent: "10H".into(),
            nr_rcvd: "東京".into(),
            serial: 7,
            mode: Mode::Cw,
            band: Band::B14,
            multi1: "M1".into(),
            operator: "OP".into(),
            reserve3: 123_456_700,
            ..Qso::default()
        };
        let r = to_record(&q);
        assert_eq!(f64::from_le_bytes(r[0..8].try_into().unwrap()), 45000.5);
        assert_eq!(r[8], 6);
        assert_eq!(&r[9..15], b"JA1ZLO");
        assert_eq!(r[21], 3);
        assert_eq!(r[52], 4);
        assert_eq!(&r[53..57], &SHIFT_JIS.encode("東京").0[..]);
        assert_eq!(i32::from_le_bytes(r[88..92].try_into().unwrap()), 7);
        assert_eq!(r[92], Mode::Cw as u8);
        assert_eq!(r[93], Band::B14 as u8);
        assert_eq!(r[95], 2);
        assert_eq!(r[160], 2);
        assert_eq!(
            i32::from_le_bytes(r[252..256].try_into().unwrap()),
            123_456_700
        );
        let back = from_record(&r);
        assert_eq!(back.callsign, "JA1ZLO");
        assert_eq!(back.nr_rcvd, "東京");
        assert_eq!(back.reserve3, q.reserve3);
        assert_eq!(back.band, Band::B14);
    }

    #[test]
    fn strings_go_out_in_shift_jis_and_come_back_in_utf8() {
        assert_eq!(
            to_plugin("東京").as_bytes(),
            &SHIFT_JIS.encode("東京").0[..]
        );
        assert_eq!(to_plugin("a\0b").as_bytes(), b"ab");
        assert_eq!(to_plugin("😀").as_bytes(), b"?");
        assert_eq!(from_plugin("札幌".as_bytes()), "札幌");
        assert_eq!(from_plugin(b"a\xffb"), "a\u{fffd}b");
    }

    #[test]
    fn query_answers_fit_the_buffer_without_splitting_characters() {
        assert_eq!(query_answer("JA1ZLO"), b"JA1ZLO");
        let long = "あ".repeat(200);
        let a = query_answer(&long);
        assert_eq!(a.len(), 256);
        assert_eq!(SHIFT_JIS.decode(&a).0, "あ".repeat(128));
        let odd = format!("x{}", "あ".repeat(200));
        assert_eq!(query_answer(&odd).len(), 255);
    }

    #[test]
    fn commit_changes_only_masked_fields() {
        let base = Qso {
            callsign: "JA1AAA".into(),
            nr_rcvd: "10".into(),
            memo: "m".into(),
            ..Qso::default()
        };
        let patch = Qso {
            callsign: "JA1BBB".into(),
            nr_rcvd: "20".into(),
            memo: "new".into(),
            ..Qso::default()
        };
        let q = apply_commit(&base, &patch, commit::RCVD | commit::NOTE);
        assert_eq!(q.callsign, "JA1AAA");
        assert_eq!(q.nr_rcvd, "20");
        assert_eq!(q.memo, "new");
    }
}
