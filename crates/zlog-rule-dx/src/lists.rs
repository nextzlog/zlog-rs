//! Fixed multiplier lists: zLog's `ARDX.DAT` and `ARRL10.DAT` (states and provinces),
//! `IOTA.DAT` (islands) and the JIDX prefectures (`UJIDX_DX_Multi.KenNames`).

const ARDX_DAT: &str = include_str!("../data/ARDX.DAT");
const ARRL10_DAT: &str = include_str!("../data/ARRL10.DAT");
const IOTA_DAT: &str = include_str!("../data/IOTA.DAT");

/// A state or province (`TState`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub name: String,
    pub abbrev: String,
    /// Accepted spellings, comma separated (`AltAbbrev`).
    pub alt: String,
}

/// Parse a state list (`TStateList.LoadFromFile`): a title line, then pairs of a
/// name/abbreviation line and an alternatives line ending in `;`.
fn parse_states(text: &str) -> Vec<State> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut l = 1;
    while l + 1 < lines.len() {
        let line = lines[l];
        if line.to_lowercase().contains("end of file") {
            break;
        }
        let name = line.get(..22).unwrap_or(line).trim_end().to_string();
        let abbrev = line.get(29..).unwrap_or("").trim().to_string();
        l += 1;
        let mut alt = String::new();
        if l + 1 < lines.len() {
            alt = lines[l].trim().to_string();
            if alt.ends_with(|c: char| !c.is_ascii_alphanumeric()) {
                alt.pop();
            }
            l += 1;
        }
        out.push(State { name, abbrev, alt });
    }
    out
}

/// States, provinces and DX for ARRL DX and CQ WW RTTY (`ARDX.DAT`).
pub fn ardx_states() -> Vec<State> {
    parse_states(ARDX_DAT)
}

/// States, provinces and Mexican states for ARRL 10m, with the ITU regions 1 to 3
/// that maritime mobiles send (`TARRL10Multi.FormCreate`).
pub fn arrl10_states() -> Vec<State> {
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

/// The state a received number names (`GetState`).
pub fn find_state<'a>(states: &'a [State], nr: &str) -> Option<&'a State> {
    let s = nr.trim().to_ascii_uppercase();
    states
        .iter()
        .find(|st| format!(",{},", st.alt).contains(&format!(",{s},")))
}

/// IOTA references and names (`TIslandList.LoadFromFile`).
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

/// The IOTA reference in a received number, normalised to `AS007`
/// (`TIOTAMulti.ExtractMulti`); empty if there is none.
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

/// JIDX prefectures 1 to 50 (`KenNames`).
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
    fn lists_load() {
        let s = ardx_states();
        assert_eq!(s[0].abbrev, "AL");
        assert_eq!(find_state(&s, "ari").unwrap().abbrev, "AZ");
        assert_eq!(s.last().unwrap().abbrev, "YT");
        let a = arrl10_states();
        assert_eq!(find_state(&a, "TAMPS").unwrap().abbrev, "TAM");
        assert_eq!(find_state(&a, "2").unwrap().name, "ITU Reg. 2");
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
