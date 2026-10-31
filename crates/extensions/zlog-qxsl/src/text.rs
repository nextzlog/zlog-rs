use crate::cabrillo;
use crate::delphi::{ansi as sjis, date_time, fill_left, fill_right};
use rxsl::formats::{zall::ZAll, zdos::ZDos};
use rxsl::{Band as QBand, Item, QName};
use zlog_core::qso::QslState;
use zlog_core::{Band, Contest, Log, Qso};

fn memo(q: &Qso) -> String {
    zlog_core::zlo::memo_str(q, QslState::None)
}

const FLAGS: [&str; 4] = ["PSE QSL", "NO QSL", "*", "-DUPE-"];

fn rxsl_item(q: &Qso) -> Option<Item> {
    let fits = |new: bool, multi: &str| !new || !multi.is_empty();
    if q.band == Band::Unknown
        || !fits(q.new_multi1, &q.multi1)
        || !fits(q.new_multi2, &q.multi2)
        || (q.new_multi2 != q.new_multi1 && !q.multi2.is_empty())
    {
        return None;
    }
    let mut it = crate::to_item(q, 0);
    let exact = QBand::parse(&q.freq).ok().filter(|f| {
        let word = q.memo.split(' ').next().unwrap_or("");
        format!("{}.{}", f.0 / 1000, f.0 % 1000 / 100) == q.freq
            && crate::exact_band(&f.khz()).is_none()
            && rxsl::conv::nominal_band(&f.khz()) == it.attrs.qxsl("band").map(str::to_string)
            && !FLAGS.contains(&word)
    });
    let mut bare = q.clone();
    if let Some(f) = exact {
        it.attrs.set(QName::qxsl("band"), f.khz());
        bare.freq.clear();
    }
    it.attrs.set(QName::qxsl("note"), memo(&bare));
    Some(it)
}

fn rxsl_encode(format: &dyn rxsl::Format, log: &Log) -> Option<Vec<u8>> {
    let items = log.qsos.iter().map(rxsl_item).collect::<Option<Vec<_>>>()?;
    format.encode(&items).ok()
}

pub fn zlog_all(log: &Log) -> Vec<u8> {
    rxsl_encode(&ZAll { offset: 0 }, log).unwrap_or_else(|| native_zlog_all(log))
}

pub fn zlog_dos(log: &Log) -> Vec<u8> {
    let dos = ZDos {
        offset: 0,
        year: None,
    };
    rxsl_encode(&dos, log).unwrap_or_else(|| native_zlog_dos(log))
}

fn native_zlog_all(log: &Log) -> Vec<u8> {
    let mut out = b"zLog for Windows \r\n".to_vec();
    for q in &log.qsos {
        let (y, mo, d, h, mi) = date_time(q.time.0);
        out.extend(sjis(&format!("{y:04}/{mo:02}/{d:02} {h:02}:{mi:02} ")));
        out.extend(fill_right(&q.callsign, 13));
        out.extend(fill_right(&q.rst_sent.to_string(), 4));
        out.extend(fill_right(&q.nr_sent, 8));
        out.extend(fill_right(&q.rst_rcvd.to_string(), 4));
        out.extend(fill_right(&q.nr_rcvd, 8));
        for (new, multi) in [(q.new_multi1, &q.multi1), (q.new_multi2, &q.multi2)] {
            out.extend(match new {
                true => fill_right(multi, 6),
                false => b"-     ".to_vec(),
            });
        }
        out.extend(fill_right(q.band.label(), 5));
        out.extend(fill_right(q.mode.label(), 5));
        out.extend(fill_right(&q.points.to_string(), 3));
        if !q.operator.is_empty() {
            out.extend(fill_right(&format!("%%{}%%", q.operator), 19));
        }
        out.extend(sjis(&memo(q)));
        out.extend(b"\r\n");
    }
    out
}

fn native_zlog_dos(log: &Log) -> Vec<u8> {
    const LONG: &str = "mon day time  callsign      sent                              rcvd                           multi   MHz mode pts memo";
    const SHORT: &str =
        "mon day time  callsign      sent         rcvd      multi   MHz mode pts memo";
    let last = log
        .qsos
        .last()
        .map_or(0, |q| q.nr_sent.encode_utf16().count());
    let long = last >= 10;
    let nr = if long { 31 } else { 10 };
    let mut out = sjis(if long { LONG } else { SHORT });
    out.extend(b"\r\n");
    for q in &log.qsos {
        let (_, mo, d, h, mi) = date_time(q.time.0);
        out.extend(fill_left(&mo.to_string(), 3));
        out.push(b' ');
        out.extend(fill_left(&d.to_string(), 3));
        out.extend(format!(" {:04} ", h * 100 + mi).bytes());
        out.extend(fill_right(&q.callsign, 11));
        out.extend(fill_left(&q.rst_sent.to_string(), 3));
        out.extend(fill_right(&q.nr_sent, nr));
        out.extend(fill_left(&q.rst_rcvd.to_string(), 3));
        out.extend(fill_right(&q.nr_rcvd, nr));
        out.extend(match q.new_multi1 {
            true => fill_left(&q.multi1, 6),
            false => b"      ".to_vec(),
        });
        out.extend(b"  ");
        out.extend(fill_left(q.band.label(), 4));
        out.extend(b"  ");
        out.extend(fill_right(q.mode.label(), 3));
        out.push(b' ');
        out.extend(fill_right(&q.points.to_string(), 2));
        let gap: &[u8] = if long { b"" } else { b"  " };
        if !q.operator.is_empty() {
            out.extend(gap);
            out.extend(sjis(&format!("%%{}%%", q.operator)));
        }
        out.extend(gap);
        out.extend(sjis(&memo(q)));
        out.extend(b"\r\n");
    }
    out
}

pub fn cabrillo(log: &Log, contest: &dyn Contest, mycall: &str) -> Result<Vec<u8>, String> {
    let mut out = format!("START-OF-LOG: 3.0\r\nCALLSIGN: {mycall}\r\n");
    for tag in cabrillo::TAGS.iter().filter(|t| **t != "CALLSIGN") {
        out.push_str(&format!("{tag}: \r\n"));
        if *tag == "CLUB" {
            out.push_str("CREATED-BY: \r\n");
        }
    }
    let mut out = sjis(&out);
    let exchange = cabrillo::Exchange::of("", contest);
    out.extend(cabrillo::qso_lines(log, exchange, false, mycall, 0)?);
    out.extend(b"END-OF-LOG:\r\n");
    Ok(out)
}

pub fn adif(log: &Log, contest: &dyn Contest) -> Vec<u8> {
    let field = |name: &str, value: &str| {
        let mut b = sjis(&format!("<{name}:{}>", value.encode_utf16().count()));
        b.extend(sjis(value));
        b
    };
    let offset = log.utc_offset_minutes().map_or(0, |m| -m);
    let serial = contest.next_serial_on(Band::B14).is_some();
    let mut out = b"ADIF export from zLog for Windows\r\nAll times in UTC\r\n<eoh>\r\n".to_vec();
    for q in &log.qsos {
        let (y, mo, d, h, mi) = date_time(q.time.0 + f64::from(offset) / 1440.0);
        let extra = contest.adif_fields(q);
        let (id, extra): (Vec<_>, Vec<_>) = extra.iter().partition(|(n, _)| n == "contest_id");
        out.extend(field("qso_date", &format!("{y:04}{mo:02}{d:02}")));
        out.extend(field("time_on", &format!("{h:02}{mi:02}")));
        out.extend(field("time_off", &format!("{h:02}{mi:02}")));
        out.extend(field("call", &q.callsign));
        out.extend(field("rst_sent", &q.rst_sent.to_string()));
        match serial {
            true => out.extend(field("stx", &q.serial.to_string())),
            false => out.extend(field("stx_string", &q.nr_sent)),
        }
        out.extend(field("rst_rcvd", &q.rst_rcvd.to_string()));
        out.extend(field(if serial { "srx" } else { "srx_string" }, &q.nr_rcvd));
        for (i, (name, value)) in extra.iter().enumerate() {
            if i == 0 || !value.is_empty() {
                out.extend(field(name, value));
            }
        }
        let band = cabrillo::ADIF_BANDS.get(q.band as usize).copied();
        out.extend(field("band", band.unwrap_or("")));
        out.extend(field("mode", q.mode.label()));
        if !q.operator.is_empty() {
            out.extend(field("operator", &q.operator));
        }
        if let Some(khz) = q.freq.trim().parse::<f64>().ok().filter(|k| *k != 0.0) {
            out.extend(field("freq", &format!("{:.4}", khz / 1000.0)));
        }
        for (name, value) in id.iter().filter(|(_, v)| !v.is_empty()) {
            out.extend(field(name, value));
        }
        out.extend(b"<eor>\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allja1() -> Log {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../apps/zlog-max/tests/regress/logs/allja1.zlo"
        );
        Log::load(path).unwrap().0
    }

    #[test]
    fn rxsl_writes_zlog_text_like_zlog() {
        let mut log = allja1();
        log.qsos[0].freq = "14025.5".into();
        log.qsos[1].freq = "21000".into();
        log.qsos[2].qsl_state = QslState::NoQsl;
        log.qsos[2].forced = true;
        log.qsos[3].memo = "* hi".into();
        log.qsos[3].freq = "50100.0".into();
        log.qsos[4].qsy_violation = true;
        log.qsos[5].operator = "JA1ZLO".into();
        log.qsos[6].band = Band::B10G;
        let dos = ZDos {
            offset: 0,
            year: None,
        };
        let all = rxsl_encode(&ZAll { offset: 0 }, &log).unwrap();
        assert_eq!(all, native_zlog_all(&log));
        assert!(String::from_utf8_lossy(&all).contains("(14025.5)"));
        assert_eq!(rxsl_encode(&dos, &log).unwrap(), native_zlog_dos(&log));
        log.qsos[6].new_multi2 = true;
        log.qsos[6].multi2 = "X".into();
        assert!(rxsl_encode(&ZAll { offset: 0 }, &log).is_none());
        assert_eq!(zlog_all(&log), native_zlog_all(&log));
    }
}
