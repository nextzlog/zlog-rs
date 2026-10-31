pub mod cabrillo;
pub mod form;
mod panel;

use std::collections::BTreeMap;

use zlog_core::{Contest, Log, ScoreSheet};

use form::{Form, Score};
use rxsl::sheets::summary::{self as jarl, Version};

pub use panel::{register, SubmitForm, Summary, WebSubmit};

zlog_core::texts!();

pub type Section = BTreeMap<String, String>;

pub fn default_form() -> Form {
    let mut f = Form::default();
    f.summary.address = jarl::ADDRESS.into();
    f.summary.oath = jarl::oath(true).into();
    f
}

pub fn load(form: &mut Form, header: &mut cabrillo::Header, section: &Section) {
    let get = |k: &str| section.get(k).map(|v| unescape(v));
    for (key, field, _) in text_fields(form) {
        if let Some(v) = get(&format!("elog.{key}")) {
            *field = v;
        }
    }
    form.summary.version = match get("elog.Version").as_deref() {
        Some("1") => Version::R2,
        _ => Version::R1,
    };
    form.power_measured = get("elog.PowerType").as_deref() == Some("1");
    form.field_extend = get("elog.FieldExtend").as_deref() == Some("1");
    form.license_date = get("elog.LicenseDate").and_then(|d| {
        let v: Vec<&str> = d.split('-').collect();
        match v[..] {
            [y, m, d] => Some((y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)),
            _ => None,
        }
    });
    if let Some(v) = get("elog.NewComer") {
        form.rules.newcomer = v;
    }
    if let Some(v) = get("elog.SeniorJunior") {
        form.rules.senior_junior = v;
    }
    for tag in cabrillo::TAGS {
        if let Some(v) = get(&format!("cabrillo.{tag}")) {
            header.values.insert(tag.into(), v);
        }
    }
    header.utf8 = get("cabrillo.UTF8").as_deref() == Some("1");
    header.zone_hours = match get("cabrillo.TimeZone").as_deref() {
        Some("9") => 9,
        _ => 0,
    };
}

pub fn save(form: &Form, header: &cabrillo::Header, section: &mut Section) {
    let mut form = form.clone();
    for (key, field, _) in text_fields(&mut form) {
        section.insert(format!("elog.{key}"), escape(field));
    }
    let version = if form.summary.version == Version::R1 {
        "0"
    } else {
        "1"
    };
    section.insert("elog.Version".into(), version.into());
    section.insert(
        "elog.PowerType".into(),
        (form.power_measured as u8).to_string(),
    );
    section.insert(
        "elog.FieldExtend".into(),
        (form.field_extend as u8).to_string(),
    );
    if let Some((y, m, d)) = form.license_date {
        section.insert("elog.LicenseDate".into(), format!("{y:04}-{m:02}-{d:02}"));
    }
    section.insert("elog.NewComer".into(), form.rules.newcomer);
    section.insert("elog.SeniorJunior".into(), form.rules.senior_junior);
    for (tag, v) in &header.values {
        if tag != "CLAIMED-SCORE" {
            section.insert(format!("cabrillo.{tag}"), escape(v));
        }
    }
    section.insert("cabrillo.TimeZone".into(), header.zone_hours.to_string());
    section.insert("cabrillo.UTF8".into(), (header.utf8 as u8).to_string());
}

pub struct Input<'a> {
    pub log: &'a Log,
    pub sheet: &'a ScoreSheet,
    pub contest: &'a dyn Contest,
    pub callsign: &'a str,
    pub operator: &'a str,
    pub period_hours: u32,
}

impl Input<'_> {
    fn operators(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.log.qsos.iter().map(|q| q.operator.as_str()).collect();
        out.push(self.operator);
        out
    }
}

pub enum Failure {
    Missing(&'static str),
    Failed(String),
}

pub fn elog_file(form: &Form, input: &Input<'_>) -> Result<Vec<u8>, Failure> {
    let mut form = form.clone();
    form.fill_defaults(input.contest.name(), input.callsign, &input.operators());
    if form.summary.date.is_empty() {
        form.summary.date = jarl::date_text(today(input.log.utc_offset_minutes()));
    }
    if let Some(k) = form.missing().first() {
        return Err(Failure::Missing(k));
    }
    let score = Score::new(input.sheet, input.log, input.contest.band_range());
    form::elog(&form, &score, input.log, input.period_hours).map_err(Failure::Failed)
}

pub fn cabrillo_file(header: &cabrillo::Header, input: &Input<'_>) -> Result<Vec<u8>, String> {
    let mut header = header.clone();
    if header.get("CALLSIGN").is_empty() {
        header
            .values
            .insert("CALLSIGN".into(), input.callsign.to_string());
    }
    header
        .values
        .entry("CLAIMED-SCORE".into())
        .or_insert_with(|| input.sheet.score.to_string());
    cabrillo::document(
        &header,
        input.log,
        input.contest,
        input.callsign,
        input.period_hours,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Elog,
    Cabrillo,
}

impl Kind {
    pub fn parse(key: &str) -> Option<Kind> {
        match key.to_ascii_lowercase().as_str() {
            "elog" | "em" | "txt" => Some(Kind::Elog),
            "cabrillo" | "cbr" | "log" => Some(Kind::Cabrillo),
            _ => None,
        }
    }
}

pub fn parse_settings(text: &str) -> Result<BTreeMap<String, Section>, String> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    Ok(table
        .into_iter()
        .filter_map(|(name, value)| {
            let toml::Value::Table(t) = value else {
                return None;
            };
            let section = t
                .into_iter()
                .filter_map(|(k, v)| match v {
                    toml::Value::String(s) => Some((k, s)),
                    toml::Value::Integer(i) => Some((k, i.to_string())),
                    toml::Value::Float(f) => Some((k, f.to_string())),
                    toml::Value::Boolean(b) => Some((k, b.to_string())),
                    _ => None,
                })
                .collect();
            Some((name, section))
        })
        .collect())
}

pub const SECTION: &str = "panel.summary";

pub fn write(
    settings: &BTreeMap<String, Section>,
    kind: Kind,
    log: &Log,
    sheet: &ScoreSheet,
    contest: &dyn Contest,
    date: Option<&str>,
) -> Result<Vec<u8>, String> {
    let empty = Section::new();
    let get = |s: &str| settings.get(s).unwrap_or(&empty);
    let (mut form, mut header) = (default_form(), cabrillo::Header::default());
    load(&mut form, &mut header, get(SECTION));
    if let Some(date) = date {
        form.summary.date = date.to_string();
    }
    let station = get("station");
    let text = |k: &str| station.get(k).map_or("", String::as_str);
    let use_period = get("preferences")
        .get("use_contest_period")
        .is_none_or(|v| v != "false");
    let input = Input {
        log,
        sheet,
        contest,
        callsign: text("callsign"),
        operator: text("operator"),
        period_hours: if use_period {
            contest.period_hours()
        } else {
            0
        },
    };
    match kind {
        Kind::Elog => elog_file(&form, &input).map_err(|e| match e {
            Failure::Missing(k) => format!("{SECTION}: {k}"),
            Failure::Failed(e) => e,
        }),
        Kind::Cabrillo => cabrillo_file(&header, &input),
    }
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => match chars.next() {
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            },
            (c, false) => out.push(c),
        }
    }
    out
}

pub(crate) fn text_fields(f: &mut Form) -> [(&'static str, &mut String, bool); 20] {
    let s = &mut f.summary;
    [
        ("CategoryCode", &mut s.category_code, false),
        ("CategoryName", &mut s.category_name, false),
        ("Callsign", &mut s.callsign, false),
        ("OperatorCallsign", &mut s.op_callsign, false),
        ("Address", &mut s.address, true),
        ("Telephone", &mut s.tel, false),
        ("OperatorName", &mut s.name, false),
        ("EMail", &mut s.email, false),
        ("License", &mut s.license_class, false),
        ("Power", &mut s.power, false),
        ("QTH", &mut s.op_place, false),
        ("PowerSupply", &mut s.power_supply, false),
        ("Equipment", &mut s.equipment, true),
        ("Age", &mut f.age, false),
        ("Comment", &mut s.comments, true),
        ("MultiOpList", &mut f.operators, true),
        ("ClubID", &mut s.club_number, false),
        ("ClubName", &mut s.club_name, false),
        ("Oath", &mut s.oath, true),
        ("Signature", &mut s.signature, false),
    ]
}

pub(crate) fn today(offset_minutes: Option<i32>) -> (i32, u32, u32) {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let t = 25569.0 + (unix + offset_minutes.unwrap_or(540) as f64 * 60.0) / 86400.0;
    let (y, m, d, ..) = zlog_core::DelphiTime(t).to_ymd_hms();
    (y, m, d)
}

pub(crate) fn float_to_str(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.*e}", 14, v);
        let parsed: f64 = s.parse().unwrap_or(v);
        format!("{parsed}")
    }
}

pub(crate) fn without_out_of_period(log: &Log, period_hours: u32) -> Log {
    let mut out = log.clone();
    out.qsos.retain(|q| !log.out_of_period(q, period_hours));
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use zlog_core::{Band, Contest, ContestMode, Log, Power, Qso, Scoring, Station};

    pub(crate) struct Plain;

    #[test]
    fn floats() {
        assert_eq!(super::float_to_str(1234.0), "1234");
        assert_eq!(super::float_to_str(1.5), "1.5");
        assert_eq!(super::float_to_str(0.1 + 0.2), "0.3");
    }

    impl Contest for Plain {
        fn name(&self) -> &str {
            "Plain"
        }
        fn band_range(&self) -> (Band, Band) {
            (Band::B1_9, Band::HIGHEST)
        }
        fn contest_mode(&self) -> ContestMode {
            ContestMode::Cw
        }
        fn sent_number(&self, _: Band, _: Power, _: &Station) -> String {
            String::new()
        }
        fn valid_number(&self, _: &Qso) -> bool {
            true
        }
        fn score(&mut self, _: &Log) -> Scoring {
            Scoring::default()
        }
    }
}
