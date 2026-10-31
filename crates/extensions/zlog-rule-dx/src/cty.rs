// CTY.DAT (AD1C) is not embedded: its licence does not clearly allow redistribution.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Country {
    pub name: String,
    pub cq_zone: String,
    pub itu_zone: String,
    pub continent: String,
    pub code: String,
}

impl Country {
    pub fn is_waedc(&self) -> bool {
        self.code.starts_with('*')
    }
}

#[derive(Debug, Clone, Default)]
struct Prefix {
    country: usize,
    cq: String,
    itu: String,
    continent: String,
    full_match: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Found<'a> {
    pub country: &'a Country,
    pub index: usize,
    ovr_cq: &'a str,
    ovr_itu: &'a str,
    ovr_continent: &'a str,
}

impl<'a> Found<'a> {
    pub fn continent(&self) -> &'a str {
        if self.ovr_continent.is_empty() {
            &self.country.continent
        } else {
            self.ovr_continent
        }
    }

    pub fn is_unknown(&self) -> bool {
        self.country.code.is_empty()
    }
}

#[derive(Debug, Default)]
struct Table {
    prefixes: HashMap<String, Prefix>,
}

#[derive(Debug)]
pub struct Cty {
    countries: Vec<Country>,
    waedc: Table,
    dxcc: Table,
    loaded: bool,
}

impl Default for Cty {
    fn default() -> Self {
        Cty {
            countries: vec![unknown()],
            waedc: Table::default(),
            dxcc: Table::default(),
            loaded: false,
        }
    }
}

fn unknown() -> Country {
    Country {
        name: "Unknown".into(),
        ..Country::default()
    }
}

fn extract(p: &mut String, open: char, close: char) -> String {
    let Some(a) = p.find(open) else {
        return String::new();
    };
    let b = p[a + 1..].find(close).map(|i| a + 1 + i).unwrap_or(p.len());
    let inner = p[a + 1..b].to_string();
    let end = (b + close.len_utf8()).min(p.len());
    p.replace_range(a..end, "");
    inner
}

static GLOBAL: RwLock<Option<Arc<Cty>>> = RwLock::new(None);

fn search(data: Option<PathBuf>, exe: Option<PathBuf>, env: Option<OsString>) -> Option<PathBuf> {
    let mut found = data.into_iter().find(|p| p.is_file());
    if found.is_none() {
        found = exe
            .into_iter()
            .chain([PathBuf::from(".")])
            .flat_map(|d| [d.join("CTY.DAT"), d.join("cty.dat")])
            .find(|p| p.is_file());
    }
    found.or_else(|| env.map(PathBuf::from))
}

impl Cty {
    pub fn parse(text: &str) -> Cty {
        let mut cty = Cty {
            loaded: true,
            ..Cty::default()
        };
        let flat: String = text.chars().filter(|c| *c != '\r' && *c != '\n').collect();
        for record in flat.split(';') {
            let f: Vec<&str> = record.split(':').map(crate::lists::delphi_trim).collect();
            if f.len() < 9 {
                continue;
            }
            let country = Country {
                name: f[0].into(),
                cq_zone: f[1].into(),
                itu_zone: f[2].into(),
                continent: f[3].into(),
                code: f[7].into(),
            };
            let index = cty.countries.len();
            let waedc = country.is_waedc();
            for raw in f[8].split(',') {
                let mut p = crate::lists::delphi_trim(raw).to_ascii_uppercase();
                if p.is_empty() {
                    continue;
                }
                let full_match = p.starts_with('=');
                if full_match {
                    p.remove(0);
                }
                let cq = extract(&mut p, '(', ')');
                let itu = extract(&mut p, '[', ']');
                let continent = extract(&mut p, '{', '}');
                extract(&mut p, '<', '>');
                extract(&mut p, '~', '~');
                let entry = Prefix {
                    country: index,
                    cq,
                    itu,
                    continent,
                    full_match,
                };
                cty.waedc.prefixes.entry(p.clone()).or_insert(entry.clone());
                if !waedc {
                    cty.dxcc.prefixes.entry(p).or_insert(entry);
                }
            }
            cty.countries.push(country);
        }
        cty
    }

    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Cty> {
        let bytes = std::fs::read(path)?;
        let text: String = bytes.iter().map(|b| *b as char).collect();
        Ok(Cty::parse(&text))
    }

    pub fn installed_path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("zlog").join("cty.dat"))
    }

    pub fn default_path() -> Option<PathBuf> {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf));
        search(Cty::installed_path(), exe, std::env::var_os("ZLOG_CTY_DAT"))
    }

    pub fn global() -> Arc<Cty> {
        let mut slot = GLOBAL.write().unwrap_or_else(|e| e.into_inner());
        slot.get_or_insert_with(|| Arc::new(Cty::from_default_path()))
            .clone()
    }

    pub fn reload() {
        let cty = Arc::new(Cty::from_default_path());
        *GLOBAL.write().unwrap_or_else(|e| e.into_inner()) = Some(cty);
    }

    fn from_default_path() -> Cty {
        Cty::default_path()
            .and_then(|p| Cty::load(p).ok())
            .unwrap_or_default()
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn countries(&self) -> &[Country] {
        &self.countries
    }

    fn found<'a>(&'a self, p: &'a Prefix) -> Found<'a> {
        Found {
            country: &self.countries[p.country],
            index: p.country,
            ovr_cq: &p.cq,
            ovr_itu: &p.itu,
            ovr_continent: &p.continent,
        }
    }

    fn unknown_found(&self) -> Found<'_> {
        Found {
            country: &self.countries[0],
            index: 0,
            ovr_cq: "",
            ovr_itu: "",
            ovr_continent: "",
        }
    }

    fn forward<'a>(t: &'a Table, call: &str) -> Option<&'a Prefix> {
        (1..=call.len())
            .rev()
            .filter_map(|n| call.get(..n))
            .filter_map(|p| t.prefixes.get(p))
            .find(|p| !p.full_match)
    }

    pub fn lookup(&self, callsign: &str, waedc: bool) -> Found<'_> {
        let t = if waedc { &self.waedc } else { &self.dxcc };
        let call = callsign.trim().to_ascii_uppercase();
        if call.is_empty() {
            return self.unknown_found();
        }
        if let Some(p) = t.prefixes.get(&call).filter(|p| p.full_match) {
            return self.found(p);
        }
        let (mut first, mut right) = match call.split_once('/') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => (call.clone(), String::new()),
        };
        if right == "MM" {
            return self.unknown_found();
        }
        const IGNORED: [&str; 13] = [
            "AA", "AT", "AG", "AE", "M", "P", "AM", "QRP", "A", "KT", "N", "T", "0",
        ];
        if IGNORED.contains(&right.as_str())
            || (right.len() == 1 && right.as_bytes()[0].is_ascii_digit())
        {
            right.clear();
        }
        if !first.is_empty() && !right.is_empty() && first.len() > right.len() {
            std::mem::swap(&mut first, &mut right);
        }
        Self::forward(t, &first)
            .or_else(|| Self::forward(t, &right))
            .map(|p| self.found(p))
            .unwrap_or_else(|| self.unknown_found())
    }

    pub fn guess_cq_zone(&self, callsign: &str, waedc: bool) -> String {
        let f = self.lookup(callsign, waedc);
        let call = callsign.to_ascii_uppercase();
        let mut z: u32 = f.country.cq_zone.parse().unwrap_or(0);
        let area = call_area_digit(&call);
        match f.country.code.as_str() {
            "W" | "K" => {
                z = match area {
                    1..=4 => 5,
                    5 | 8 | 9 | 0 => 4,
                    _ => 3,
                }
            }
            "VE" => {
                z = match area {
                    1 | 2 | 9 => 5,
                    3..=6 => 4,
                    7 => 3,
                    8 => 1,
                    _ => 2,
                }
            }
            "VK" => z = if matches!(area, 6 | 8) { 29 } else { 30 },
            "BY" => z = if matches!(area, 9 | 0) { 23 } else { 24 },
            "UA" | "UA0" | "UA9" => z = russian_zone(&call).unwrap_or(z),
            _ => {}
        }
        if !f.ovr_cq.is_empty() {
            z = f.ovr_cq.parse().unwrap_or(0);
        }
        if z == 0 {
            String::new()
        } else {
            z.to_string()
        }
    }

    pub fn guess_itu_zone(&self, callsign: &str, waedc: bool) -> String {
        let f = self.lookup(callsign, waedc);
        let call = callsign.to_ascii_uppercase();
        let mut z: u32 = f.country.itu_zone.parse().unwrap_or(0);
        let area = call_area_digit(&call);
        match f.country.code.as_str() {
            "W" | "K" => {
                z = match area {
                    1..=4 => 8,
                    5 | 8 | 9 | 0 => 7,
                    _ => 6,
                }
            }
            "VE" => {
                z = match area {
                    1 | 2 => 8,
                    3..=6 => 3,
                    7 | 8 => 2,
                    9 => 9,
                    _ => 4,
                }
            }
            "VK" => {
                z = match area {
                    6 | 8 => 58,
                    9 | 0 => 30,
                    _ => 55,
                }
            }
            _ => {}
        }
        if !f.ovr_itu.is_empty() {
            z = f.ovr_itu.parse().unwrap_or(0);
        }
        if z == 0 {
            String::new()
        } else {
            z.to_string()
        }
    }
}

pub fn call_area_digit(call: &str) -> u32 {
    let b = call.as_bytes();
    let pos = call.find('/').map(|p| p + 1).unwrap_or(0);
    let digit = if pos > 4 {
        b.iter().rev().find(|c| c.is_ascii_digit())
    } else {
        b.iter().find(|c| c.is_ascii_digit())
    };
    digit.map(|c| (c - b'0') as u32).unwrap_or(6)
}

fn ex_pos(pattern: &str, s: &str) -> bool {
    let p = pattern.as_bytes();
    s.as_bytes()
        .windows(p.len())
        .any(|w| w.iter().zip(p).all(|(a, b)| *b == b'?' || a == b))
}

fn russian_zone(call: &str) -> Option<u32> {
    let mut z = None;
    let next_after = |c: char| {
        call.find(c)
            .and_then(|k| call.as_bytes().get(k + 1).copied())
    };
    if ex_pos("U?0", call) || ex_pos("R?0", call) || call.contains("R0") {
        if let Some(n) = next_after('0') {
            z = Some(match n {
                b'A' | b'B' | b'H' | b'O' | b'P' | b'S' | b'T' | b'U' | b'V' | b'W' => 18,
                b'Y' => 23,
                _ => 19,
            });
        }
    }
    if ex_pos("U?8", call) || ex_pos("R?8", call) {
        z = Some(18);
    }
    if ex_pos("U?9", call) || ex_pos("R?9", call) {
        if let Some(n) = next_after('9') {
            z = Some(match n {
                b'S' | b'T' | b'W' => 16,
                b'H' | b'I' | b'O' | b'P' | b'U' | b'V' | b'Y' | b'Z' => 18,
                _ => 17,
            });
        }
    }
    z
}

const PORTABLE: [&str; 11] = [
    "AA", "AT", "AG", "AE", "M", "P", "AM", "QRP", "A", "KT", "MM",
];

fn last_digit_prefix(s: &str) -> String {
    match s.rfind(|c: char| c.is_ascii_digit()) {
        None => format!("{s}0"),
        Some(k) if s.ends_with(|c: char| c.is_ascii_alphabetic()) => s[..=k].to_string(),
        Some(_) => s.to_string(),
    }
}

pub fn wpx_prefix(callsign: &str) -> String {
    let mut s = callsign.to_ascii_uppercase();
    if let Some(i) = s.find('/') {
        let tail = s[i + 1..].to_string();
        if PORTABLE.contains(&tail.as_str()) {
            s.truncate(i);
        } else if tail.len() == 1 && tail.as_bytes()[0].is_ascii_digit() {
            let b = s.as_bytes();
            let j = (1..b.len()).find(|&j| b[j].is_ascii_digit());
            return match j {
                Some(j) => format!("{}{}", &s[..j], tail),
                None => format!("{s}{tail}"),
            };
        } else if i > 3 {
            return last_digit_prefix(&tail);
        } else {
            return last_digit_prefix(&s[..i]);
        }
    }
    let b = s.as_bytes();
    if !(1..b.len()).any(|j| b[j].is_ascii_digit()) {
        return format!("{}0", s.get(..2).unwrap_or(&s));
    }
    match s.rfind(|c: char| c.is_ascii_digit()) {
        Some(k) if k > 0 => s[..=k].to_string(),
        _ => s.get(..1).unwrap_or("").to_string(),
    }
}

pub fn call_area(callsign: &str, code: &str) -> String {
    if !matches!(code, "JA" | "K" | "VE" | "VK") {
        return String::new();
    }
    fn sub(s: &str) -> char {
        let b = s.as_bytes();
        let mut area = None;
        let mut in_number = false;
        for i in 1..b.len() {
            let d = b[i].is_ascii_digit();
            if !in_number && d {
                area = Some(b[i]);
                in_number = true;
            }
            if in_number && !d {
                area = Some(b[i - 1]);
                break;
            }
        }
        area.map(|c| c as char).unwrap_or('0')
    }
    let call = callsign.to_ascii_uppercase();
    let Some(i) = call.find('/') else {
        return format!("{code}{}", sub(&call));
    };
    let tail = &call[i + 1..];
    if PORTABLE.contains(&tail) {
        return String::new();
    }
    if tail.len() == 1 && tail.as_bytes()[0].is_ascii_digit() {
        format!("{code}{tail}")
    } else if i > 3 {
        format!("{code}{}", sub(tail))
    } else {
        format!("{code}{}", sub(&call[..i]))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn the_data_folder_comes_before_the_exe_folder_and_the_variable() {
        let dir = std::env::temp_dir().join(format!("zlog-cty-order-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data = dir.join("data/cty.dat");
        let exe = dir.join("exe");
        std::fs::create_dir_all(data.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&exe).unwrap();
        let env = || Some(OsString::from("env.dat"));
        assert_eq!(
            search(Some(data.clone()), Some(exe.clone()), env()),
            env().map(PathBuf::from)
        );
        std::fs::write(exe.join("CTY.DAT"), "x").unwrap();
        assert_eq!(
            search(Some(data.clone()), Some(exe.clone()), env()),
            Some(exe.join("CTY.DAT"))
        );
        std::fs::write(&data, "x").unwrap();
        assert_eq!(
            search(Some(data.clone()), Some(exe.clone()), env()),
            Some(data)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_installed_file_lives_in_the_zlog_data_folder() {
        let p = Cty::installed_path().unwrap();
        assert!(p.ends_with("zlog/cty.dat"));
    }

    pub const FIXTURE: &str = "\
Japan:                    25:  45:  AS:   36.40:  -138.38:    -9.0:  JA:
    JA,JE,JR,7K,=JA1TEST/W;
United States:            05:  08:  NA:   37.53:    91.67:     5.0:  K:
    K,N,W,AA,=K1MM(4)[7];
Canada:                   05:  09:  NA:   44.35:    78.75:     5.0:  VE:
    VE,VA;
Mexico:                   06:  10:  NA:   21.32:   100.23:     6.0:  XE:
    XE;
Australia:                30:  59:  OC:  -23.70:  -132.33:   -10.0:  VK:
    VK;
Fed. Rep. of Germany:     14:  28:  EU:   51.00:   -10.00:    -1.0:  DL:
    DL,DJ;
Italy:                    15:  28:  EU:   42.82:   -12.58:    -1.0:  I:
    I;
Sicily:                   15:  28:  EU:   37.50:   -14.00:    -1.0:  *IT9:
    IT9;
Hawaii:                   31:  61:  OC:   21.12:   157.48:    10.0:  KH6:
    KH6,AH6;
China:                    24:  44:  AS:   36.00:  -102.00:    -8.0:  BY:
    BY;
Asiatic Russia:           17:  30:  AS:   55.88:   -84.08:    -7.0:  UA9:
    UA9,R9{EU};
";

    #[test]
    fn lookup_rules() {
        let c = Cty::parse(FIXTURE);
        assert_eq!(c.lookup("JA1ZLO", true).country.code, "JA");
        assert_eq!(c.lookup("7K1XXX", true).country.code, "JA");
        assert_eq!(c.lookup("JA1TEST/W", true).country.code, "JA");
        assert_eq!(c.lookup("JA1ZLO/W6", true).country.code, "K");
        assert_eq!(c.lookup("W6/JA1ZLO", true).country.code, "K");
        assert_eq!(c.lookup("JA1ZLO/P", true).country.code, "JA");
        assert!(c.lookup("JA1ZLO/MM", true).is_unknown());
        assert_eq!(c.lookup("IT9ABC", true).country.code, "*IT9");
        assert_eq!(c.lookup("IT9ABC", false).country.code, "I");
        assert_eq!(c.lookup("R9AA", true).continent(), "EU");
        assert!(c.lookup("ZZ1ZZ", true).is_unknown());
        assert_eq!(c.guess_cq_zone("K1MM", true), "4");
        assert_eq!(c.guess_itu_zone("K1MM", true), "7");
        assert_eq!(c.guess_cq_zone("W6AAA", true), "3");
        assert_eq!(c.guess_itu_zone("W1AW", true), "8");
        assert_eq!(c.guess_cq_zone("VK6AA", true), "29");
        assert_eq!(c.guess_cq_zone("UA9SA", true), "16");
        assert_eq!(c.guess_cq_zone("JA1ZLO", true), "25");
        assert_eq!(Cty::default().guess_cq_zone("JA1ZLO", true), "");
    }

    #[test]
    fn wpx_prefixes() {
        assert_eq!(wpx_prefix("JA1ZLO"), "JA1");
        assert_eq!(wpx_prefix("JA1ZLO/2"), "JA2");
        assert_eq!(wpx_prefix("JA1ZLO/JD1"), "JD1");
        assert_eq!(wpx_prefix("KH0AM/W6"), "W6");
        assert_eq!(wpx_prefix("KH0/AD6AJ"), "KH0");
        assert_eq!(wpx_prefix("AD6AJ/PA"), "PA0");
        assert_eq!(wpx_prefix("PA/AD6AJ"), "PA0");
        assert_eq!(wpx_prefix("XEFTA"), "XE0");
        assert_eq!(wpx_prefix("N8BJQ/QRP"), "N8");
        assert_eq!(wpx_prefix("4X4ZZ"), "4X4");
    }

    #[test]
    fn call_areas() {
        assert_eq!(call_area("JA1ZLO", "JA"), "JA1");
        assert_eq!(call_area("7K1XXX", "JA"), "JA1");
        assert_eq!(call_area("JA1ZLO/3", "JA"), "JA3");
        assert_eq!(call_area("JA1ZLO/P", "JA"), "");
        assert_eq!(call_area("W6AAA", "K"), "K6");
        assert_eq!(call_area("DL1AAA", "DL"), "");
    }
}
