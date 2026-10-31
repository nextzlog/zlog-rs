use zlog_core::{Band, Mode, Qso};

use crate::count::{bands, BANDS, MAX_HOURS};
use crate::texts;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Count {
    pub qso: u32,
    pub cw: u32,
    pub multi: u32,
    pub multi2: u32,
    pub points: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimeChart {
    pub start_hour: u32,
    pub data: Vec<Vec<Count>>,
}

pub(crate) const ALL: usize = BANDS;
pub(crate) const SUM: usize = 0;

impl TimeChart {
    pub fn new(qsos: &[Qso], exclude_zero_points: bool) -> Self {
        let mut data = vec![vec![Count::default(); BANDS + 1]; MAX_HOURS + 1];
        let Some(first) = qsos.first() else {
            return TimeChart {
                start_hour: 0,
                data,
            };
        };
        let base = first.time.0;
        let (_, _, base_day, start_hour, _, _) = first.time.to_ymd_hms();
        let offset = start_hour as i64 - 1;
        for q in qsos {
            if q.invalid || q.time.0 < base || (exclude_zero_points && q.points == 0) {
                continue;
            }
            if q.band > Band::HIGHEST {
                continue;
            }
            let (_, _, day, hour, _, _) = q.time.to_ymd_hms();
            let t = if day == base_day {
                hour as i64 - offset
            } else {
                hour as i64 + 24 - offset
            };
            if !(1..=MAX_HOURS as i64).contains(&t) {
                continue;
            }
            for h in [t as usize, SUM] {
                for b in [q.band as usize, ALL] {
                    let c = &mut data[h][b];
                    c.qso += 1;
                    c.points += q.points as u32;
                    c.cw += (q.mode == Mode::Cw) as u32;
                    c.multi += q.new_multi1 as u32;
                    c.multi2 += q.new_multi2 as u32;
                }
            }
        }
        TimeChart { start_hour, data }
    }

    pub fn last_hour(&self) -> usize {
        (1..=MAX_HOURS)
            .rev()
            .find(|h| self.data[*h][ALL].qso > 0)
            .unwrap_or(1)
    }

    pub(crate) fn hour_text(&self, h: usize) -> String {
        format!("{:02}", (self.start_hour as usize + h - 1) % 24)
    }

    pub(crate) fn worked(&self) -> impl Iterator<Item = Band> + '_ {
        bands().filter(|b| self.data[SUM][*b as usize].qso > 0)
    }
}

fn right(s: &str, n: usize) -> String {
    let s = format!("{}{s}", " ".repeat(n));
    s[s.len() - n..].to_string()
}

fn qso_text(n: u32, len: usize) -> String {
    right(&n.to_string(), len)
}

fn cw_text(n: u32, len: usize) -> String {
    if n == 0 {
        " ".repeat(len)
    } else {
        let s = format!("({n}){}", " ".repeat(len));
        s[..len].to_string()
    }
}

pub(crate) fn band_string(b: Band) -> String {
    match b.label().strip_suffix('G') {
        Some(g) => format!("{g} GHz"),
        None => format!("{} MHz", b.label()),
    }
}

pub fn zaq(chart: &TimeChart, language: &str) -> Vec<String> {
    let t = |k| texts().get(language, k);
    let last = chart.last_hour();
    let rule = format!("-----+{}+----", "---".repeat(last));
    let mut out = vec![t("zaq.title").to_string(), String::new()];
    let mut head = "     |".to_string();
    for h in 1..=last {
        head.push(' ');
        head.push_str(&chart.hour_text(h));
    }
    out.push(format!("{head}|{}", t("zaq.sum")));
    out.push(rule.clone());
    for b in chart.worked() {
        let mut s = right(b.label(), 5) + "|";
        for h in 1..=last {
            match chart.data[h][b as usize].qso {
                0 => s.push_str("  -"),
                n => s.push_str(&right(&n.to_string(), 3)),
            }
        }
        s.push('|');
        s.push_str(&right(&chart.data[SUM][b as usize].qso.to_string(), 4));
        out.push(s);
    }
    out.push(rule);
    let mut s = format!("{}|", t("zaq.total"));
    for h in 1..=last {
        s.push_str(&right(&chart.data[h][ALL].qso.to_string(), 3));
    }
    s.push('|');
    s.push_str(&right(&chart.data[SUM][ALL].qso.to_string(), 4));
    out.push(s);
    let mut s = format!("{}|", t("zaq.cumulative"));
    let mut cumulative = 0;
    for h in 1..=last {
        cumulative += chart.data[h][ALL].qso;
        if h % 3 == 0 {
            if cumulative < 1000 {
                s.push_str(&right(&cumulative.to_string(), 3));
            } else {
                s.pop();
                s.push_str(&right(&cumulative.to_string(), 4));
            }
        } else {
            s.push_str("   ");
        }
    }
    s.push('|');
    out.push(s);
    out
}

pub fn zaf(
    chart: &TimeChart,
    show_cw: bool,
    exclude_zero_hours: bool,
    language: &str,
) -> Vec<String> {
    let t = |k| texts().get(language, k);
    let all = chart.data[SUM][ALL];
    let with2 = all.multi2 > 0;
    let mut out = vec![t("zaf.result").to_string(), String::new()];
    out.push(format!(
        "   {}",
        t(if with2 { "zaf.head2" } else { "zaf.head" })
    ));
    for b in chart.worked() {
        let c = chart.data[SUM][b as usize];
        let mut s = format!(
            "{}    {}     {}   {}",
            right(&band_string(b), 10),
            right(&c.qso.to_string(), 5),
            right(&c.points.to_string(), 5),
            right(&c.multi.to_string(), 5)
        );
        if with2 {
            s.push_str("   ");
            s.push_str(&right(&c.multi2.to_string(), 5));
        }
        out.push(s);
    }
    let mut s = format!(
        "   {}    {}    {} {}{}",
        t("zaf.total"),
        right(&all.qso.to_string(), 6),
        right(&all.points.to_string(), 6),
        t("zaf.times"),
        right(&all.multi.to_string(), 5)
    );
    if with2 {
        s.push_str("   ");
        s.push_str(&right(&all.multi2.to_string(), 5));
    }
    s.push_str(&format!("  {}  ", t("zaf.equals")));
    s.push_str(&(all.points * (all.multi + all.multi2)).to_string());
    out.push(s);
    out.push(String::new());

    out.push(t(if show_cw { "zaf.time_cw" } else { "zaf.time" }).to_string());
    out.push(String::new());
    let gap = if show_cw { "     " } else { "    " };
    let mut head = String::new();
    for b in chart.worked() {
        head.push_str(gap);
        head.push_str(&right(b.label(), 5));
    }
    head.push_str(if show_cw { "       ALL" } else { "      ALL" });
    out.push(format!("       {}", head.trim()));
    out.push(String::new());
    let cell = |c: Count, cw_len: usize| {
        if show_cw {
            format!("{}{}", qso_text(c.qso, 5), cw_text(c.cw, cw_len))
        } else {
            format!("{}    ", qso_text(c.qso, 5))
        }
    };
    for h in 1..=chart.last_hour() {
        if exclude_zero_hours && chart.data[h][ALL].qso == 0 {
            continue;
        }
        let mut s = String::new();
        for b in chart.worked() {
            s.push_str(&cell(chart.data[h][b as usize], 5));
        }
        let a = chart.data[h][ALL];
        s.push_str(&qso_text(a.qso, 5));
        if show_cw {
            s.push_str(cw_text(a.cw, 5).trim());
        }
        out.push(format!(" [{}]{s}", chart.hour_text(h)));
    }
    out.push(String::new());
    let mut s = "Total".to_string();
    for b in chart.worked() {
        s.push_str(&cell(chart.data[SUM][b as usize], 5));
    }
    s.push_str(&qso_text(all.qso, 5));
    if show_cw {
        s.push_str(cw_text(all.cw, 6).trim());
    }
    out.push(s);
    out.push(String::new());
    out.push(String::new());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::count::tests::qso;

    fn log() -> Vec<Qso> {
        let mut v = vec![
            qso(21, 0, Band::B7, 1),
            qso(21, 10, Band::B7, 1),
            qso(22, 5, Band::B3_5, 1),
            qso(24, 30, Band::B7, 1),
        ];
        v[0].new_multi1 = true;
        v[2].new_multi1 = true;
        v[2].mode = Mode::Ssb;
        v
    }

    #[test]
    fn chart_counts_by_hour() {
        let c = TimeChart::new(&log(), false);
        assert_eq!(c.start_hour, 21);
        assert_eq!(c.data[1][Band::B7 as usize].qso, 2);
        assert_eq!(c.data[2][Band::B3_5 as usize].qso, 1);
        assert_eq!(c.data[4][Band::B7 as usize].qso, 1);
        assert_eq!(c.last_hour(), 4);
        assert_eq!(c.data[SUM][ALL].multi, 2);
    }

    #[test]
    fn zaq_layout() {
        let c = TimeChart::new(&log(), false);
        let text = zaq(&c, "en");
        let expected = [
            "<Time chart>",
            "",
            "     | 21 22 23 00|Sum",
            "-----+------------+----",
            "  3.5|  -  1  -  -|   1",
            "    7|  2  -  -  1|   3",
            "-----+------------+----",
            "Total|  2  1  0  1|   4",
            "Cumul|        3   |",
        ];
        assert_eq!(text, expected);
    }

    #[test]
    fn zaf_layout() {
        let c = TimeChart::new(&log(), false);
        let text = zaf(&c, true, true, "en");
        let expected = [
            "<Result>",
            "",
            "   Band       QSOs     Points   Multi",
            "   3.5 MHz        1         1       1",
            "     7 MHz        3         3       1",
            "   Totals         4         4 x    2  =  8",
            "",
            "<QSOs per hour>  (CW in brackets)",
            "",
            "       3.5         7       ALL",
            "",
            " [21]    0         2(2)      2(2)",
            " [22]    1         0         1",
            " [00]    0         1(1)      1(1)",
            "",
            "Total    1         3(3)      4(3)",
            "",
            "",
        ];
        assert_eq!(text, expected);
    }
}
