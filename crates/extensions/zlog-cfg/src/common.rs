use std::collections::{BTreeMap, HashSet};

use zlog_core::{Band, BandScore, Log, Mode, Power, Qso, Station};

pub fn dupe_flags(log: &Log, different_mode: bool, all_phone: bool) -> Vec<bool> {
    let mut seen = HashSet::new();
    log.qsos
        .iter()
        .map(|q| !q.invalid && !seen.insert(dupe_key(q, different_mode, all_phone)))
        .collect()
}

pub fn dupe_key(q: &Qso, different_mode: bool, all_phone: bool) -> String {
    let mut key = format!("{}\t{}", q.core_call(), q.band.label());
    if different_mode {
        key.push('\t');
        key.push_str(if all_phone {
            q.mode_group()
        } else {
            q.mode.label()
        });
    }
    key
}

pub fn is_domestic(call: &str) -> bool {
    let c: Vec<char> = call.chars().collect();
    if c.is_empty() {
        return true;
    }
    if c.len() < 3 {
        return false;
    }
    let (s1, s2, s3) = (c[0], c[1], c[2]);
    (s1 == 'J' && ('A'..='S').contains(&s2))
        || (s1 == '7' && s2 == 'J')
        || (s1 == '7' && ('K'..='N').contains(&s2) && ('1'..='4').contains(&s3))
        || (s1 == '8' && ('J'..='N').contains(&s2))
}

pub fn serial_str(serial: i32) -> String {
    if serial < 1000 {
        let s = format!("0000{serial}");
        s[s.len() - 3..].to_string()
    } else {
        serial.to_string()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Serials {
    start: i32,
    any: bool,
    all: BTreeMap<u8, i32>,
    band: BTreeMap<(Band, u8), i32>,
}

impl Serials {
    pub fn new(start: i32) -> Self {
        Serials {
            start,
            ..Serials::default()
        }
    }

    pub fn scan(log: &Log, start: i32) -> Self {
        let mut s = Serials::new(start);
        for q in &log.qsos {
            s.any = true;
            s.all.insert(q.tx, q.serial);
            s.band.insert((q.band, q.tx), q.serial);
        }
        s
    }

    pub fn next(&self, band: Option<Band>, tx: u8) -> i32 {
        if !self.any {
            return self.start;
        }
        let last = match band {
            Some(b) => self.band.get(&(b, tx)),
            None => self.all.get(&tx),
        };
        last.copied().unwrap_or(0).saturating_add(1)
    }
}

pub struct SentContext<'a> {
    pub station: &'a Station,
    pub prov: &'a str,
    pub qth: &'a str,
    pub power: Power,
    pub serial: Option<i32>,
    pub handle: &'a str,
}

pub fn expand_sent(template: &str, cx: &SentContext) -> Option<String> {
    let t = template.to_ascii_uppercase();
    let st = cx.station;
    let vars = [
        ("$Q", cx.qth),
        ("$V", cx.prov),
        ("$Z", st.cq_zone.as_str()),
        ("$I", st.iaru_zone.as_str()),
        ("$A", st.age.as_str()),
        ("$T", st.iota.as_str()),
        ("$H", cx.handle),
    ];
    if vars.iter().any(|(v, x)| t.contains(v) && x.is_empty())
        || (t.contains("$P") && cx.power.code().is_none())
        || (t.contains("$S") && cx.serial.is_none())
    {
        return None;
    }
    let mut s = t;
    for tag in ["[AR]", "[SK]", "[VA]", "[KN]", "[BK]", "[BT]"] {
        s = s.replace(tag, "");
    }
    let serial = cx.serial.map(serial_str).unwrap_or_default();
    let in_zlog_order = [
        ("$Z", st.cq_zone.as_str()),
        ("$I", st.iaru_zone.as_str()),
        ("$Q", cx.qth),
        ("$V", cx.prov),
        ("$P", ""),
        ("$S", serial.as_str()),
        ("$A", st.age.as_str()),
        ("$T", st.iota.as_str()),
        ("$H", cx.handle),
        ("$N", cx.power.label()),
        ("$O", st.operator.as_str()),
        ("$M", st.callsign.as_str()),
    ];
    for (v, x) in in_zlog_order {
        s = s.replace(v, &x.to_ascii_uppercase());
    }
    Some(s.replace('_', ""))
}

pub fn sum(rows: impl Iterator<Item = BandScore>) -> BandScore {
    rows.fold(BandScore::default(), |a, b| BandScore {
        qsos: a.qsos + b.qsos,
        cw_qsos: a.cw_qsos + b.cw_qsos,
        points: a.points + b.points,
        multi: a.multi + b.multi,
    })
}

pub fn empty_rows(bands: impl Iterator<Item = Band>) -> BTreeMap<Band, BandScore> {
    bands.map(|b| (b, BandScore::default())).collect()
}

pub fn count_qso(row: &mut BandScore, mode: zlog_core::ContestMode, q: &Qso) -> bool {
    let valid = mode.counts(q.mode);
    if valid {
        row.qsos += 1;
        if q.mode == Mode::Cw {
            row.cw_qsos += 1;
        }
    }
    valid
}

pub fn multi_count(text: &str) -> u32 {
    if text.is_empty() {
        0
    } else {
        text.split(',').count() as u32
    }
}

pub fn in_contest(band: Band, (lo, hi): (Band, Band), warc: bool) -> bool {
    band >= lo && band <= hi && (warc || !band.is_warc())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cx<'a>(station: &'a Station, qth: &'a str) -> SentContext<'a> {
        SentContext {
            station,
            prov: "13",
            qth,
            power: Power::P002,
            serial: Some(7),
            handle: "taro",
        }
    }

    #[test]
    fn sent_text_upper_cases_ascii_only() {
        let st = Station::default();
        assert_eq!(
            expand_sent("$q", &cx(&st, "ｔｏｋｙｏ straße")).unwrap(),
            "ｔｏｋｙｏ STRAßE"
        );
    }

    #[test]
    fn tags_are_replaced_in_zlog_order_and_inserted_text_is_expanded_again() {
        let st = Station {
            cq_zone: "$Q".into(),
            callsign: "ja1zlo".into(),
            ..Station::default()
        };
        let s = expand_sent("$Z/$S $H_$M", &cx(&st, "x$S")).unwrap();
        assert_eq!(s, "X007/007 TAROJA1ZLO", "$Z before $Q, $Q before $S");
        let s = expand_sent("$Q", &cx(&st, "$Z")).unwrap();
        assert_eq!(s, "$Z", "$Z runs before $Q, so a $Z in the QTH stays");
        let none = SentContext {
            serial: None,
            ..cx(&st, "x")
        };
        assert_eq!(expand_sent("$S", &none), None);
    }
}
