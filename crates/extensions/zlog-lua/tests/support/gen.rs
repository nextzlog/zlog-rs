use zlog_core::{Band, ContestMode, DelphiTime, Log, Mode, Qso};

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    pub fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

const TAILS: &[&str] = &["", "", "", "T", "C", "M", "H", "P", "X", "t", "TT"];

const SENT: &[&str] = &[
    "0001", "0002", "0002", "0001", "3000", "1999", "5001", "30", "", "00010", "2",
];

const PREFIXES: &[&str] = &["JA", "JE", "JH", "JR", "7K", "7N", "8J", "8N", "W", "DL"];

pub fn codes(dat: &str) -> Vec<String> {
    dat.lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(String::from)
        .collect()
}

fn digits(rng: &mut Rng, n: usize) -> String {
    (0..n)
        .map(|_| char::from(b'0' + rng.below(10) as u8))
        .collect()
}

fn callsign(rng: &mut Rng, pool: &mut Vec<String>) -> String {
    if !pool.is_empty() && rng.chance(30) {
        return rng.pick(pool).clone();
    }
    let mut c = rng.pick(PREFIXES).to_string();
    c.push(char::from(b'0' + rng.below(10) as u8));
    for _ in 0..rng.below(4) {
        c.push(char::from(b'A' + rng.below(26) as u8));
    }
    match rng.below(12) {
        0 => c.push_str("/1"),
        1 => c.push_str("/P"),
        2 => c = format!("JA1/{c}"),
        3 => c.truncate(2),
        4 => c.push('_'),
        _ => {}
    }
    pool.push(c.clone());
    c
}

fn number(rng: &mut Rng, codes: &[String]) -> String {
    match rng.below(14) {
        0 => String::new(),
        5..=7 => digits(rng, 4),
        1 => {
            let n = 1 + rng.below(8);
            digits(rng, n)
        }
        2 => {
            let n = 2 + rng.below(5);
            format!("{}{}", digits(rng, n), rng.pick(TAILS))
        }
        3 => "ABC".into(),
        _ if codes.is_empty() => digits(rng, 3),
        _ => format!("{}{}", rng.pick(codes), rng.pick(TAILS)),
    }
}

pub fn log(seed: u64, len: usize, codes: &[String]) -> Log {
    let mut rng = Rng::new(seed);
    let mut pool = Vec::new();
    let start = DelphiTime::from_ymd_hms(2026, 7, 31, 21, 0, 0).0;
    let mut t = start;
    let qsos = (0..len)
        .map(|_| {
            t += rng.below(400) as f64 / 86400.0;
            if rng.chance(2) {
                t += 1.0;
            }
            let band = if rng.chance(85) {
                *rng.pick(&[
                    Band::B3_5,
                    Band::B7,
                    Band::B14,
                    Band::B21,
                    Band::B50,
                    Band::B144,
                    Band::B430,
                    Band::B1200,
                    Band::B2400,
                    Band::B5600,
                    Band::B10G,
                ])
            } else {
                *rng.pick(Band::ALL)
            };
            let mode = if rng.chance(80) {
                *rng.pick(&[Mode::Cw, Mode::Ssb, Mode::Fm, Mode::Am])
            } else {
                *rng.pick(&[Mode::Rtty, Mode::Ft4, Mode::Ft8, Mode::Other, Mode::Dv])
            };
            Qso {
                time: DelphiTime(t),
                callsign: callsign(&mut rng, &mut pool),
                nr_rcvd: number(&mut rng, codes),
                nr_sent: rng.pick(SENT).to_string(),
                band,
                mode,
                invalid: rng.chance(3),
                ..Qso::default()
            }
        })
        .collect();
    Log {
        qsos,
        ..Log::default()
    }
}

pub const MODES: &[(ContestMode, &str)] = &[
    (ContestMode::Mix, "mix"),
    (ContestMode::Cw, "cw"),
    (ContestMode::All, "all"),
];

pub fn describe(log: &Log, contest: &mut dyn zlog_core::Contest) -> String {
    let mut l = log.clone();
    let sheet = zlog_core::rescore(&mut l, contest).unwrap();
    let mut out = String::new();
    for q in &l.qsos {
        out.push_str(&format!(
            "{} {} [{}] {} [{}] {} {} [{}]\n",
            u8::from(q.dupe),
            q.points,
            q.multi1,
            u8::from(q.new_multi1),
            q.multi2,
            u8::from(q.new_multi2),
            u8::from(contest.valid_number(q)),
            contest.extract_multi(q),
        ));
        out.push_str(&format!("check {:?}\n", contest.check_multi(q)));
    }
    for (b, r) in &sheet.bands {
        out.push_str(&format!(
            "band {} {} {} {} {}\n",
            b.label(),
            r.qsos,
            r.cw_qsos,
            r.points,
            r.multi
        ));
    }
    let t = sheet.total;
    out.push_str(&format!(
        "total {} {} {} {} score {}\n",
        t.qsos, t.cw_qsos, t.points, t.multi, sheet.score
    ));
    out
}
