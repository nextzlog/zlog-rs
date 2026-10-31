use std::collections::HashMap;
use std::ops::Deref;
use std::sync::LazyLock;

const ARDX_DAT: &str = include_str!("../data/ARDX.DAT");
const ARRL10_DAT: &str = include_str!("../data/ARRL10.DAT");
const IOTA_DAT: &str = include_str!("../data/IOTA.DAT");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub name: String,
    pub abbrev: String,
    pub alt: String,
}

fn blank(c: char) -> bool {
    c <= ' '
}

pub(crate) fn delphi_trim(s: &str) -> &str {
    s.trim_matches(blank)
}

fn parse_states(text: &str) -> Vec<State> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut l = 1;
    while l + 1 < lines.len() {
        let line = lines[l];
        if line.to_ascii_lowercase().contains("end of file") {
            break;
        }
        let copy = |from: usize, n: usize| line.chars().skip(from).take(n).collect::<String>();
        let name = copy(0, 22).trim_end_matches(blank).to_string();
        let abbrev = delphi_trim(&copy(29, 25)).to_string();
        l += 1;
        let mut alt = String::new();
        if l + 1 < lines.len() {
            alt = delphi_trim(lines[l]).to_string();
            if alt.ends_with(|c: char| !c.is_ascii_alphanumeric()) {
                alt.pop();
            }
            l += 1;
        }
        out.push(State { name, abbrev, alt });
    }
    out
}

pub struct States {
    list: Vec<State>,
    by_alias: HashMap<String, usize>,
}

impl States {
    fn new(list: Vec<State>) -> Self {
        let mut by_alias = HashMap::new();
        for (i, st) in list.iter().enumerate() {
            for a in st.alt.split(',') {
                by_alias.entry(a.to_string()).or_insert(i);
            }
        }
        States { list, by_alias }
    }
}

impl Deref for States {
    type Target = [State];

    fn deref(&self) -> &[State] {
        &self.list
    }
}

pub fn no_states() -> &'static States {
    static S: LazyLock<States> = LazyLock::new(|| States::new(Vec::new()));
    &S
}

pub fn ardx_states() -> &'static States {
    static S: LazyLock<States> = LazyLock::new(|| States::new(parse_states(ARDX_DAT)));
    &S
}

pub fn arrl10_states() -> &'static States {
    static S: LazyLock<States> = LazyLock::new(|| States::new(arrl10_list()));
    &S
}

fn arrl10_list() -> Vec<State> {
    let mut v = parse_states(ARRL10_DAT);
    for r in 1..=3 {
        v.push(State {
            name: format!("ITU Reg. {r}"),
            abbrev: r.to_string(),
            alt: r.to_string(),
        });
    }
    v
}

pub fn find_state<'a>(states: &'a States, nr: &str) -> Option<&'a State> {
    let s = delphi_trim(nr).to_ascii_uppercase();
    if s.is_empty() || s.contains(',') {
        return states
            .iter()
            .find(|st| format!(",{},", st.alt).contains(&format!(",{s},")));
    }
    states.by_alias.get(&s).map(|&i| &states.list[i])
}

pub fn islands() -> Vec<(String, String)> {
    IOTA_DAT
        .lines()
        .skip(1)
        .take_while(|l| !l.to_lowercase().contains("end of file"))
        .map(|l| {
            let r: String = l.chars().take(5).collect();
            let name: String = l.chars().skip(6).collect();
            (r, name)
        })
        .collect()
}

pub fn extract_iota(nr: &str) -> String {
    let Some(i) = nr.find(|c: char| c.is_ascii_uppercase()) else {
        return String::new();
    };
    let mut work = nr[i..].to_string();
    if let Some(p) = work.find('-') {
        work.remove(p);
    }
    let cont = work.get(..2).unwrap_or("");
    if !["AF", "AS", "AN", "EU", "OC", "NA", "SA"].contains(&cont) {
        return String::new();
    }
    let num = &work[2..];
    if !(1..=3).contains(&num.len()) {
        return String::new();
    }
    match num.parse::<u32>() {
        Ok(k) if num.bytes().all(|b| b.is_ascii_digit()) => format!("{cont}{k:03}"),
        _ => String::new(),
    }
}

pub const PREFECTURES: [&str; 50] = [
    "Hokkaido",
    "Aomori",
    "Iwate",
    "Akita",
    "Yamagata",
    "Miyagi",
    "Fukushima",
    "Niigata",
    "Nagano",
    "Tokyo",
    "Kanagawa",
    "Chiba",
    "Saitama",
    "Ibaraki",
    "Tochigi",
    "Gumma",
    "Yamanashi",
    "Shizuoka",
    "Gifu",
    "Aichi",
    "Mie",
    "Kyoto",
    "Shiga",
    "Nara",
    "Osaka",
    "Wakayama",
    "Hyogo",
    "Toyama",
    "Fukui",
    "Ishikawa",
    "Okayama",
    "Shimane",
    "Yamaguchi",
    "Tottori",
    "Hiroshima",
    "Kagawa",
    "Tokushima",
    "Ehime",
    "Kochi",
    "Fukuoka",
    "Saga",
    "Nagasaki",
    "Kumamoto",
    "Oita",
    "Miyazaki",
    "Kagoshima",
    "Okinawa",
    "Ogasawara",
    "Okinotorishima",
    "Minamitorishima",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_dat_copies_chars_and_trims_like_delphi() {
        let text = format!(
            "h\n{:<22}{:7}{:<25}tail\n Québec,QC\u{a0};\nend\n",
            "Québec\u{a0}", "", " QC\u{3000}"
        );
        let s = parse_states(&text);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "Québec\u{a0}");
        assert_eq!(s[0].abbrev, "QC\u{3000}");
        assert_eq!(s[0].alt, "Québec,QC\u{a0}");
        assert_eq!(delphi_trim("\t\u{a0}x\u{3000}\x01"), "\u{a0}x\u{3000}");
    }

    #[test]
    fn lists_load() {
        let s = ardx_states();
        assert_eq!(s[0].abbrev, "AL");
        assert_eq!(find_state(s, "ari").unwrap().abbrev, "AZ");
        assert_eq!(s.last().unwrap().abbrev, "YT");
        let a = arrl10_states();
        assert_eq!(find_state(a, "TAMPS").unwrap().abbrev, "TAM");
        assert_eq!(find_state(a, "2").unwrap().name, "ITU Reg. 2");
        let i = islands();
        assert_eq!(i[0], ("AF001".into(), "3B6 Agalega Islands".into()));
        assert!(i.len() > 1100);
    }

    #[test]
    fn iota_references() {
        assert_eq!(extract_iota("001AS007"), "AS007");
        assert_eq!(extract_iota("12AS-7"), "AS007");
        assert_eq!(extract_iota("5OC147"), "OC147");
        assert_eq!(extract_iota("123"), "");
        assert_eq!(extract_iota("1XX123"), "");
        assert_eq!(extract_iota("1AS1234"), "");
    }
}
