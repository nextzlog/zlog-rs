#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub callsign: String,
    pub sent_template: String,
    pub nr_sent: String,
    pub rst_sent: String,
    pub nr_rcvd: String,
    pub cq_zone: String,
    pub iaru_zone: String,
    pub qth: String,
    pub prov: String,
    pub operator: String,
    pub iota: String,
    pub serial: String,
    pub power_code: String,
    pub power: String,
    pub age: String,
    pub last_callsign: String,
    pub last_number: String,
    pub greeting: String,
    pub time_utc: String,
    pub my_call: String,
    pub edited_after_tab: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Abbrev {
    pub zero: char,
    pub one: char,
    pub nine: char,
}

impl Default for Abbrev {
    fn default() -> Self {
        Abbrev {
            zero: 'O',
            one: 'A',
            nine: 'N',
        }
    }
}

impl Abbrev {
    pub fn apply(&self, s: &str) -> String {
        s.chars()
            .map(|c| match c {
                '0' => self.zero,
                '1' => self.one,
                '9' => self.nine,
                c => c,
            })
            .collect()
    }
}

pub const GREETING_HOURS: [u32; 3] = [0, 12, 18];

pub fn greeting(local_hour: u32, hours: &[u32; 3]) -> &'static str {
    match local_hour {
        h if h < hours[0] => "GE",
        h if h < hours[1] => "GM",
        h if h < hours[2] => "GA",
        _ => "GE",
    }
}

fn edited_call(f: &Fields) -> &str {
    if f.callsign.is_empty() {
        &f.last_callsign
    } else if f.edited_after_tab {
        &f.callsign
    } else {
        ""
    }
}

const PROSIGNS: [(&str, &str); 6] = [
    ("[AR]", "a"),
    ("[SK]", "s"),
    ("[VA]", "s"),
    ("[KN]", "k"),
    ("[BK]", "b"),
    ("[BT]", "t"),
];

pub fn expand_rtty(template: &str, f: &Fields) -> String {
    let mut out = PROSIGNS
        .iter()
        .fold(template.to_ascii_uppercase(), |s, (from, _)| {
            s.replace(from, "")
        });
    for (tag, value) in [
        ("$X", f.sent_template.as_str()),
        ("$Y", &f.nr_sent),
        ("$B", &f.callsign),
        ("$R", &f.rst_sent),
        ("$F", &f.nr_rcvd),
        ("$Z", &f.cq_zone),
        ("$I", &f.iaru_zone),
        ("$Q", &f.qth),
        ("$V", &f.prov),
        ("$O", &f.operator),
        ("$T", &f.iota),
        ("$D", "\r\n"),
        ("$E", edited_call(f)),
        ("$H", &f.nr_sent),
        ("$S", &f.serial),
        ("$P", &f.power_code),
        ("$A", &f.age),
        ("$N", &f.power),
        ("$L", &f.last_callsign),
        ("$U", &f.last_number),
        ("$G", &f.greeting),
        ("$D", &f.time_utc),
        ("$C", &f.callsign),
        ("$M", &f.my_call),
    ] {
        out = out.replace(tag, value);
    }
    out
}

pub fn expand(template: &str, f: &Fields, abbrev: &Abbrev) -> String {
    let e = edited_call(f);
    let cut = |s: &str| abbrev.apply(s);
    let both = |s: &str| [cut(s), s.to_string()];
    let same = |s: &str| [s.to_string(), s.to_string()];
    let in_zlog_order = [
        (
            'X',
            [f.sent_template.clone(), f.sent_template.to_lowercase()],
        ),
        ('Y', both(&f.nr_sent)),
        ('B', same(&f.callsign)),
        ('R', both(&f.rst_sent)),
        ('F', both(&f.nr_rcvd)),
        ('Z', both(&f.cq_zone)),
        ('I', both(&f.iaru_zone)),
        ('Q', both(&f.qth)),
        ('V', both(&f.prov)),
        ('O', same(&f.operator)),
        ('T', both(&f.iota)),
        ('H', same(&f.nr_sent)),
        ('S', both(&f.serial)),
        ('P', same(&f.power_code)),
        ('A', both(&f.age)),
        ('N', both(&f.power)),
        ('L', same(&f.last_callsign)),
        ('U', both(&f.last_number)),
        ('G', same(&f.greeting)),
        ('D', both(&f.time_utc)),
        ('C', same(&f.callsign)),
    ];
    let mut out = template.to_string();
    for (tag, [upper, lower]) in in_zlog_order {
        out = out.replace(&format!("${tag}"), &upper);
        out = out.replace(&format!("${}", tag.to_ascii_lowercase()), &lower);
    }
    out = out.replace("$E", e);
    out = out.replace("$M", &f.my_call).replace("$m", &f.my_call);
    prosigns(&out.to_ascii_uppercase())
}

fn prosigns(s: &str) -> String {
    PROSIGNS
        .iter()
        .fold(s.to_string(), |s, (from, to)| s.replace(from, to))
}

pub fn fields(
    view: &zlog_ui::View<'_>,
    unix_secs: u64,
    local_hour: u32,
    leading_zeros: bool,
    greeting_hours: &[u32; 3],
) -> Fields {
    let station = view.station;
    let power = view.power;
    let sent = view.contest.sent_number(view.band, power, station);
    let serial = view
        .contest
        .next_serial_of_tx(view.band, view.station.tx_nr)
        .map(|n| n as i64)
        .unwrap_or(view.log.qsos.len() as i64 + 1);
    let last = view.log.qsos.last();
    let mut f = Fields {
        callsign: view.callsign.to_string(),
        sent_template: view
            .contest
            .sent_template(station)
            .unwrap_or_else(|| sent.clone()),
        nr_sent: sent,
        rst_sent: if !view.rst_sent.is_empty() {
            view.rst_sent
        } else if view.mode == zlog_core::Mode::Cw {
            "599"
        } else {
            "59"
        }
        .into(),
        nr_rcvd: view.number.to_string(),
        cq_zone: station.cq_zone.clone(),
        iaru_zone: station.iaru_zone.clone(),
        qth: station.city.clone(),
        prov: station.prov.clone(),
        operator: station.operator.clone(),
        iota: station.iota.clone(),
        serial: if leading_zeros {
            format!("{serial:03}")
        } else {
            serial.to_string()
        },
        power_code: power.code().map(String::from).unwrap_or_default(),
        power: power.label().into(),
        age: station.age.to_uppercase(),
        last_callsign: last.map(|q| q.callsign.clone()).unwrap_or_default(),
        last_number: last.map(|q| q.nr_rcvd.clone()).unwrap_or_default(),
        greeting: String::new(),
        time_utc: String::new(),
        my_call: station.callsign.to_uppercase(),
        edited_after_tab: view.tab.edited(view.callsign),
    };
    stamp(&mut f, unix_secs, local_hour, greeting_hours);
    f
}

pub fn stamp(f: &mut Fields, unix_secs: u64, local_hour: u32, greeting_hours: &[u32; 3]) {
    let day = unix_secs % 86_400;
    f.greeting = greeting(local_hour, greeting_hours).into();
    f.time_utc = format!("{:02}{:02}", day / 3600, day % 3600 / 60);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Fields {
        Fields {
            callsign: "JA1ZLO".into(),
            sent_template: "$V$P".into(),
            nr_sent: "10H".into(),
            prov: "10".into(),
            power_code: "H".into(),
            rst_sent: "599".into(),
            nr_rcvd: "100109M".into(),
            serial: "019".into(),
            my_call: "jr1abc".into(),
            last_callsign: "JA1YAA".into(),
            ..Fields::default()
        }
    }

    #[test]
    fn default_messages() {
        let f = fields();
        let a = Abbrev::default();
        assert_eq!(expand("CQ TEST $M TEST", &f, &a), "CQ TEST JR1ABC TEST");
        assert_eq!(expand("$C $R$X", &f, &a), "JA1ZLO 5NNAOH");
        assert_eq!(expand("$C?", &f, &a), "JA1ZLO?");
    }

    #[test]
    fn cut_numbers_by_case() {
        let f = fields();
        let a = Abbrev::default();
        assert_eq!(expand("$S $s $Y $y $H", &f, &a), "OAN 019 AOH 10H 10H");
        assert_eq!(expand("$F", &f, &a), "AOOAONM");
        let t = Abbrev {
            zero: 'T',
            ..Abbrev::default()
        };
        assert_eq!(expand("$S", &f, &t), "TAN");
    }

    #[test]
    fn prosigns_and_unknown() {
        let f = fields();
        let a = Abbrev::default();
        assert_eq!(expand("tu [ar] $W $", &f, &a), "TU a $W $");
        assert_eq!(expand("[BK] [KN][BT][SK][VA]", &f, &a), "b ktss");
    }

    #[test]
    fn x_is_the_sent_template_expanded_by_the_later_tags() {
        let mut f = fields();
        let a = Abbrev::default();
        assert_eq!(expand("$X $x", &f, &a), "AOH 10H");
        f.sent_template = "599$Z".into();
        f.cq_zone = "25".into();
        assert_eq!(expand("$X|$x", &f, &a), "59925|59925");
        f.sent_template = "10H".into();
        assert_eq!(expand("$X", &f, &a), "10H");
    }

    #[test]
    fn x_keeps_literal_digits_of_the_template() {
        let mut f = fields();
        let a = Abbrev::default();
        f.sent_template = "001$V$P".into();
        assert_eq!(expand("$X|$x", &f, &a), "001AOH|00110H");
        f.sent_template = "1909$S".into();
        assert_eq!(expand("$X|$x", &f, &a), "1909OAN|1909019");
        f.sent_template = "100".into();
        assert_eq!(expand("$X $Y", &f, &a), "100 AOH");
    }

    #[test]
    fn upper_casing_is_ascii_only() {
        let mut f = fields();
        f.callsign = "straße é".into();
        assert_eq!(expand("$C", &f, &Abbrev::default()), "STRAßE é");
    }

    #[test]
    fn e_is_the_call_only_when_edited_after_tab() {
        let mut f = fields();
        let a = Abbrev::default();
        assert_eq!(expand("$E TU", &f, &a), " TU");
        f.edited_after_tab = true;
        assert_eq!(expand("$E TU", &f, &a), "JA1ZLO TU");
        f.callsign.clear();
        assert_eq!(expand("$E TU", &f, &a), "JA1YAA TU");
        assert_eq!(expand("$e", &f, &a), "$E");
    }

    #[test]
    fn a_tab_state_is_edited_once_the_call_differs() {
        use zlog_ui::TabState;
        assert!(!TabState::Normal.edited("JA1ZLO"));
        let sent = TabState::Sent("JA1ZLO".into());
        assert!(!sent.edited("JA1ZLO"));
        assert!(sent.edited("JA1ZL"));
        assert!(sent.edited("JA1ZLA"));
    }

    #[test]
    fn rtty_follows_set_str_no_abbrev() {
        let mut f = fields();
        f.callsign = "ja1zlo".into();
        f.cq_zone = "25".into();
        f.time_utc = "1234".into();
        f.greeting = "GE".into();
        f.age = "45".into();
        let cases = [
            ("TU [KN]", "TU "),
            ("[AR][SK][VA][BK][BT]x", "X"),
            ("A$DB", "A\r\nB"),
            ("a$db", "A\r\nB"),
            ("$C $c $B", "ja1zlo ja1zlo ja1zlo"),
            ("$R $S $Y $H $F", "599 019 10H 10H 100109M"),
            ("$X", "10H"),
            ("$Z $G $A $M", "25 GE 45 jr1abc"),
            ("$e TU", " TU"),
            ("$W $", "$W $"),
        ];
        for (t, want) in cases {
            assert_eq!(expand_rtty(t, &f), want, "{t}");
        }
        f.sent_template = "$S$D".into();
        assert_eq!(expand_rtty("$X", &f), "019\r\n");
        f.sent_template = "$Z".into();
        assert_eq!(expand_rtty("$X", &f), "25");
    }

    #[test]
    fn greetings() {
        let h = &GREETING_HOURS;
        assert_eq!(greeting(0, h), "GM");
        assert_eq!(greeting(6, h), "GM");
        assert_eq!(greeting(11, h), "GM");
        assert_eq!(greeting(12, h), "GA");
        assert_eq!(greeting(17, h), "GA");
        assert_eq!(greeting(18, h), "GE");
        assert_eq!(greeting(23, h), "GE");
    }

    #[test]
    fn greetings_with_configured_hours() {
        let h = [4, 10, 16];
        assert_eq!(greeting(3, &h), "GE");
        assert_eq!(greeting(4, &h), "GM");
        assert_eq!(greeting(10, &h), "GA");
        assert_eq!(greeting(16, &h), "GE");
        assert_eq!(greeting(5, &[24, 24, 24]), "GE");
        assert_eq!(greeting(5, &[0, 0, 0]), "GE");
    }
}
