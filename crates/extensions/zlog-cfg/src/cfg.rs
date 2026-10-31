use zlog_core::{Band, Mode};

pub const BANDS: usize = Band::HIGHEST as usize + 1;
pub const MODES: usize = 9;

pub type PointsTable = [[i32; MODES]; BANDS];

pub const BANDS_WITHOUT_WARC: [Band; 19] = [
    Band::B1_9,
    Band::B3_5,
    Band::B7,
    Band::B14,
    Band::B21,
    Band::B28,
    Band::B50,
    Band::B144,
    Band::B430,
    Band::B1200,
    Band::B2400,
    Band::B5600,
    Band::B10G,
    Band::B10_4G,
    Band::B24G,
    Band::B47G,
    Band::B77G,
    Band::B135G,
    Band::B248G,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialType {
    None,
    All,
    Band,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixMulti {
    Off,
    Wpx,
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialCallMatch {
    Full,
    Partial,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub contest_name: String,
    pub prov: String,
    pub city: String,
    pub power: String,
    pub coeff: bool,
    pub use_coeff: bool,
    pub sent: String,
    pub cw_message_a: [String; 8],
    pub cw_message_b: [String; 8],
    pub cw_message_cq: [String; 3],
    pub points: PointsTable,
    pub local_points: PointsTable,
    pub same_cty_points: Option<PointsTable>,
    pub same_cont_points: Option<PointsTable>,
    pub special_call_points: PointsTable,
    pub special_calls: String,
    pub special_call_match: SpecialCallMatch,
    pub local_country: String,
    pub local_continent: String,
    pub local: Vec<(String, String)>,
    pub alphabet_points: Option<[i32; 43]>,
    pub min_local_len: i32,
    pub dat: String,
    pub use_cty: bool,
    pub cty_entity_only: bool,
    pub count_multi_once: bool,
    pub no_country_multi: String,
    pub accept_different_mode: bool,
    pub all_phone: bool,
    pub cut: i32,
    pub lcut: i32,
    pub tail: i32,
    pub ltail: i32,
    pub undef_multi: bool,
    pub cut_tailing_alphabets: bool,
    pub allow_unlisted_multi: bool,
    pub no_multi: bool,
    pub use_multi2: bool,
    pub prefix_multi: PrefixMulti,
    pub serial: SerialType,
    pub serial_start: i32,
    pub count_higher_points: bool,
    pub warc: bool,
    pub use_utc: bool,
    pub band_plan: String,
    pub use_period: bool,
    pub start_time: i32,
    pub period: i32,
    pub contest_id: String,
    pub use_sent_rst: bool,
    pub allow_dx_no_number: bool,
    pub single_10g: bool,
    pub same_exchange: bool,
    pub numeric_compare: bool,
    pub use_nr_ime: bool,
    pub ignored: Vec<String>,
}

fn ones() -> PointsTable {
    [[1; MODES]; BANDS]
}

impl Default for Config {
    fn default() -> Self {
        Config {
            contest_name: String::new(),
            prov: String::new(),
            city: String::new(),
            power: "-".repeat(19),
            coeff: false,
            use_coeff: true,
            sent: String::new(),
            cw_message_a: Default::default(),
            cw_message_b: Default::default(),
            cw_message_cq: Default::default(),
            points: ones(),
            local_points: ones(),
            same_cty_points: None,
            same_cont_points: None,
            special_call_points: ones(),
            special_calls: String::new(),
            special_call_match: SpecialCallMatch::Full,
            local_country: String::new(),
            local_continent: String::new(),
            local: Vec::new(),
            alphabet_points: None,
            min_local_len: 0,
            dat: String::new(),
            use_cty: false,
            cty_entity_only: false,
            count_multi_once: false,
            no_country_multi: String::new(),
            accept_different_mode: false,
            all_phone: true,
            cut: 0,
            lcut: 0,
            tail: 0,
            ltail: 0,
            undef_multi: false,
            cut_tailing_alphabets: false,
            allow_unlisted_multi: false,
            no_multi: false,
            use_multi2: false,
            prefix_multi: PrefixMulti::Off,
            serial: SerialType::None,
            serial_start: 1,
            count_higher_points: false,
            warc: false,
            use_utc: false,
            band_plan: String::new(),
            use_period: false,
            start_time: -1,
            period: 24,
            contest_id: String::new(),
            use_sent_rst: false,
            allow_dx_no_number: false,
            single_10g: true,
            same_exchange: true,
            numeric_compare: false,
            use_nr_ime: false,
            ignored: Vec::new(),
        }
    }
}

pub(crate) fn str_to_int_def(s: &str, def: i32) -> i32 {
    crate::user::delphi_str_to_int(s).unwrap_or(def)
}

pub fn delphi_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

fn blank(c: char) -> bool {
    ('\u{1}'..=' ').contains(&c)
}

pub(crate) fn text_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split(['\r', '\n'])
}

fn on_off(s: &str) -> bool {
    s.eq_ignore_ascii_case("ON")
}

fn int_def(s: &str, def: i32) -> i32 {
    match str_to_int_def(s, -99) {
        -99 => def,
        w => w,
    }
}

pub(crate) fn comma_text(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    loop {
        while matches!(chars.peek(), Some(c) if blank(*c)) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        let mut item = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                if c == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        item.push('"');
                    } else {
                        break;
                    }
                } else {
                    item.push(c);
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ',' || blank(c) {
                    break;
                }
                item.push(c);
                chars.next();
            }
        }
        while matches!(chars.peek(), Some(c) if blank(*c)) {
            chars.next();
        }
        out.push(item);
        if chars.peek() == Some(&',') {
            chars.next();
            if chars.peek().is_none() {
                out.push(String::new());
            }
        }
    }
    out
}

fn get_band(s: &str) -> Band {
    match s {
        "3.5" => Band::B3_5,
        "7" => Band::B7,
        "10" => Band::B10,
        "14" => Band::B14,
        "18" => Band::B18,
        "21" => Band::B21,
        "24" => Band::B24,
        "28" => Band::B28,
        "50" => Band::B50,
        "144" => Band::B144,
        "430" => Band::B430,
        "120" => Band::B1200,
        "240" => Band::B2400,
        "560" => Band::B5600,
        "10G" => Band::B10G,
        "104" => Band::B10_4G,
        "24G" => Band::B24G,
        "47G" => Band::B47G,
        "77G" => Band::B77G,
        "135" => Band::B135G,
        "248" => Band::B248G,
        _ => Band::B1_9,
    }
}

fn copy3(s: &str, from: usize) -> String {
    s.chars().skip(from).take(3).collect()
}

fn set_points_table(pt: &mut PointsTable, s: &str) {
    let parts: Vec<&str> = s.split(',').collect();
    for (pair, band) in parts.chunks(2).zip(BANDS_WITHOUT_WARC) {
        let row = &mut pt[band as usize];
        row[Mode::Cw as usize] = str_to_int_def(pair[0], 0);
        let Some(ph) = pair.get(1) else {
            return;
        };
        let k = str_to_int_def(ph, 0);
        for m in [
            Mode::Ssb,
            Mode::Fm,
            Mode::Am,
            Mode::Rtty,
            Mode::Ft4,
            Mode::Ft8,
            Mode::Other,
            Mode::Dv,
        ] {
            row[m as usize] = k;
        }
    }
}

fn set_digit_points(row: &mut [i32; MODES], param: &str, width: usize) {
    let chars: Vec<char> = param.to_ascii_uppercase().chars().collect();
    let field = |i: usize| -> Option<i32> {
        let s: String = chars.get(i * width..(i + 1) * width)?.iter().collect();
        Some(str_to_int_def(&s, 1))
    };
    if chars.len() >= 2 * width {
        row[Mode::Ssb as usize] = field(0).unwrap_or(1);
        row[Mode::Cw as usize] = field(1).unwrap_or(1);
    }
    if chars.len() >= 3 * width {
        row[Mode::Fm as usize] = field(2).unwrap_or(1);
    }
    if chars.len() >= 4 * width {
        row[Mode::Am as usize] = field(3).unwrap_or(1);
    }
}

fn set_extended_points(row: &mut [i32; MODES], param: &str) {
    let mut sl = comma_text(&(param.to_ascii_uppercase() + ",,,,,,,,,"));
    sl.resize(9, String::new());
    let order = [
        Mode::Cw,
        Mode::Ssb,
        Mode::Fm,
        Mode::Am,
        Mode::Rtty,
        Mode::Ft4,
        Mode::Ft8,
        Mode::Dv,
        Mode::Other,
    ];
    for (m, v) in order.into_iter().zip(sl) {
        row[m as usize] = str_to_int_def(&v, 1);
    }
}

fn parse_alpha_points(table: &mut [i32; 43], param: &str, digit: usize) {
    let chars: Vec<char> = param.chars().collect();
    let cnt = chars.len() / digit;
    for i in 0..cnt {
        let n = i * digit;
        let s: Vec<char> = if i + 1 == cnt {
            chars[n..].to_vec()
        } else {
            chars[n..n + digit].to_vec()
        };
        let value = str_to_int_def(&s[1..].iter().collect::<String>(), 0);
        match s[0] {
            '?' => table
                .iter_mut()
                .filter(|v| **v == 0)
                .for_each(|v| *v = value),
            c @ '0'..='Z' => table[c as usize - '0' as usize] = value,
            _ => {}
        }
    }
}

enum Line {
    Skip,
    Name(String),
    Command(String, String),
}

fn parse_line(line: &str) -> Line {
    let line = delphi_trim(line);
    if line.is_empty() || line.starts_with(';') {
        return Line::Skip;
    }
    if let Some(name) = line.strip_prefix('#') {
        return Line::Name(name.to_string());
    }
    let line = line.split(';').next().unwrap_or("");
    let Some(p) = line.find('\t').or_else(|| line.find(' ')) else {
        return Line::Skip;
    };
    Line::Command(
        delphi_trim(&line[..p]).to_ascii_uppercase(),
        delphi_trim(&line[p + 1..]).to_string(),
    )
}

pub(crate) fn is_exit(line: &str) -> bool {
    matches!(parse_line(line), Line::Command(c, _) if c == "EXIT")
}

impl Config {
    pub fn parse(text: &str) -> Config {
        let text = crate::script::split(text).0;
        let mut d = Config::default();
        for raw in text_lines(text) {
            let (cmd, param) = match parse_line(raw) {
                Line::Skip => continue,
                Line::Name(n) => {
                    d.contest_name = n;
                    continue;
                }
                Line::Command(c, p) => (c, p),
            };
            if cmd == "EXIT" {
                break;
            }
            if !d.apply(&cmd, &param) {
                d.ignored.push(cmd);
            }
        }
        d.coeff = text_lines(text).any(|l| {
            let l = l.to_ascii_uppercase();
            l.starts_with("COEFF") && l.contains("ON")
        });
        d
    }

    fn apply(&mut self, cmd: &str, param: &str) -> bool {
        let upper = param.to_ascii_uppercase();
        if let Some(i) = cmd
            .strip_prefix('F')
            .and_then(|r| r.strip_suffix("_A").or_else(|| r.strip_suffix("_B")))
            .filter(|n| n.len() == 1)
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| (1..=8).contains(n))
        {
            if cmd.ends_with("_A") {
                self.cw_message_a[i - 1] = param.into();
            } else {
                self.cw_message_b[i - 1] = param.into();
            }
            return true;
        }
        if cmd.starts_with("PT") {
            let b = get_band(&copy3(cmd, 2));
            set_digit_points(&mut self.points[b as usize], param, 1);
            return true;
        }
        if cmd.starts_with("LPT") {
            let b = get_band(&copy3(cmd, 3));
            set_digit_points(&mut self.local_points[b as usize], param, 1);
            return true;
        }
        if cmd.starts_with("XPT") {
            let b = get_band(&copy3(cmd, 3));
            set_digit_points(&mut self.points[b as usize], param, 2);
            return true;
        }
        if cmd.starts_with("XLPT") {
            let b = get_band(&copy3(cmd, 4));
            set_digit_points(&mut self.local_points[b as usize], param, 2);
            return true;
        }
        if cmd.starts_with("EXPT") {
            let b = get_band(&copy3(cmd, 4));
            set_extended_points(&mut self.points[b as usize], param);
            return true;
        }
        if cmd.starts_with("EXLPT") {
            let b = get_band(&copy3(cmd, 5));
            set_extended_points(&mut self.local_points[b as usize], param);
            return true;
        }
        match cmd {
            "PROV" => self.prov = upper,
            "CITY" => self.city = upper,
            "POWER" => {
                let chars: Vec<char> = upper.chars().collect();
                let mut p: Vec<char> = chars.iter().copied().take(19).collect();
                if chars.len() > 19 {
                    p[18] = chars[chars.len() - 1];
                }
                p.resize(19, '-');
                self.power = p.into_iter().collect();
            }
            "COEFF" => self.use_coeff = on_off(param),
            "SENDNR" => self.sent = param.into(),
            "CQ2" => self.cw_message_cq[1] = param.into(),
            "CQ3" => self.cw_message_cq[2] = param.into(),
            "SAMECTYPT" => {
                let mut t = self.same_cty_points.unwrap_or_else(ones);
                set_points_table(&mut t, param);
                self.same_cty_points = Some(t);
            }
            "SAMECONTPT" => {
                let mut t = self.same_cont_points.unwrap_or_else(ones);
                set_points_table(&mut t, param);
                self.same_cont_points = Some(t);
            }
            "LOCALPT" => set_points_table(&mut self.local_points, param),
            "DEFAULTPT" => set_points_table(&mut self.points, param),
            "SPECIALCALLPT" => set_points_table(&mut self.special_call_points, param),
            "SPECIALCALLS" => {
                if !self.special_calls.is_empty() {
                    self.special_calls.push(',');
                }
                self.special_calls.push_str(&upper);
            }
            "LOCALCTY" => self.local_country = upper,
            "LOCALCONT" => self.local_continent = upper,
            "LOCAL" => {
                for s in comma_text(&upper).into_iter().filter(|s| !s.is_empty()) {
                    let key: String = if self.min_local_len > 0 {
                        s.chars().take(self.min_local_len as usize).collect()
                    } else {
                        s.clone()
                    };
                    if !self.local.iter().any(|(k, _)| *k == key) {
                        self.local.push((key, s));
                    }
                }
            }
            "ALPHAPT" | "ALPHAPT2" => {
                let mut t = self.alphabet_points.unwrap_or([0; 43]);
                parse_alpha_points(&mut t, &upper, if cmd == "ALPHAPT" { 2 } else { 3 });
                self.alphabet_points = Some(t);
            }
            "LOCMIN" => self.min_local_len = int_def(param, self.min_local_len),
            "DAT" => {
                self.dat = if param.contains('.') {
                    param.into()
                } else {
                    format!("{param}.DAT")
                }
            }
            "TIME" => {
                if upper == "UTC" {
                    self.use_utc = true;
                }
            }
            "CTY" => self.use_cty = true,
            "CTYENTITYONLY" => self.cty_entity_only = on_off(param),
            "COUNTMULTIONCE" => self.count_multi_once = on_off(param),
            "NOCTYMULTI" => self.no_country_multi = upper,
            "MODE" => self.accept_different_mode = on_off(param),
            "ALLPHONE" => self.all_phone = on_off(param),
            "CUT" => self.cut = int_def(param, self.cut),
            "LCUT" => self.lcut = int_def(param, self.lcut),
            "TAIL" => self.tail = int_def(param, self.tail),
            "LTAIL" => self.ltail = int_def(param, self.ltail),
            "UNDEFMULTI" => self.undef_multi = on_off(param),
            "JARL" | "CUTTAILABT" => self.cut_tailing_alphabets = on_off(param),
            "UNLISTEDMULTI" => self.allow_unlisted_multi = on_off(param),
            "NOMULTI" => self.no_multi = on_off(param),
            "PXMULTI" => match upper.as_str() {
                "NORMAL" => self.prefix_multi = PrefixMulti::Normal,
                "WPX" => self.prefix_multi = PrefixMulti::Wpx,
                "OFF" => self.prefix_multi = PrefixMulti::Off,
                _ => {}
            },
            "SERIAL" => match upper.as_str() {
                "ALL" => self.serial = SerialType::All,
                "BAND" => self.serial = SerialType::Band,
                _ => {}
            },
            "SERIALSTART" => self.serial_start = str_to_int_def(param, 1),
            "COUNTHIGH" => self.count_higher_points = on_off(param),
            "WARC" => self.warc = on_off(param),
            "BANDPLAN" => self.band_plan = upper,
            "USEPERIOD" => self.use_period = on_off(param),
            "STARTTIME" => self.start_time = str_to_int_def(param, -1),
            "PERIOD" => self.period = str_to_int_def(param, 0),
            "USEMULTI2" => self.use_multi2 = on_off(param),
            "CONTESTID" => self.contest_id = upper,
            "USESENTRST" => self.use_sent_rst = on_off(param),
            "ALLOWDXNONR" => self.allow_dx_no_number = on_off(param),
            "SPCALLMATCH" => {
                self.special_call_match = if upper == "PARTIAL" {
                    SpecialCallMatch::Partial
                } else {
                    SpecialCallMatch::Full
                }
            }
            "SINGLE10G" => self.single_10g = on_off(param),
            "SAMEEXCHANGE" => self.same_exchange = on_off(param),
            "NRNUMCOMPARE" => self.numeric_compare = on_off(param),
            "USENRIME" => self.use_nr_ime = on_off(param),
            _ => return false,
        }
        true
    }

    pub fn power_char(&self, band: Band) -> char {
        BANDS_WITHOUT_WARC
            .iter()
            .position(|b| *b == band)
            .and_then(|i| self.power.chars().nth(i))
            .unwrap_or('-')
    }

    pub fn band_low(&self) -> Band {
        BANDS_WITHOUT_WARC
            .iter()
            .copied()
            .find(|b| self.power_char(*b) != '-')
            .unwrap_or(Band::B1_9)
    }

    pub fn band_high(&self) -> Band {
        BANDS_WITHOUT_WARC
            .iter()
            .copied()
            .rev()
            .find(|b| self.power_char(*b) != '-')
            .unwrap_or(Band::B50)
    }

    pub fn band_available(&self, band: Band) -> bool {
        if band == Band::B10_4G {
            return !self.single_10g;
        }
        if band.is_warc() {
            return false;
        }
        self.power_char(band) != '-'
    }

    pub fn at(table: &PointsTable, band: Band, mode: Mode) -> i32 {
        table
            .get(band as usize)
            .map(|r| r[mode as usize])
            .unwrap_or(0)
    }
}
