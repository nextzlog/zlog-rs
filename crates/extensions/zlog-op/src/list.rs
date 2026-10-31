use zlog_core::{Band, Power, Station};
use zlog_ui::Section;

pub const POWER_BANDS: [Band; 19] = [
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

pub fn power_codes(index: usize) -> &'static [char] {
    match index {
        0..=6 => &['-', 'H', 'M', 'L', 'P'],
        7..=8 => &['-', 'M', 'L', 'P'],
        9 => &['-', 'L', 'P'],
        _ => &['-', 'P'],
    }
}

fn power_index(band: Band) -> usize {
    POWER_BANDS.iter().position(|b| *b == band).unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operator {
    pub callsign: String,
    power: String,
    pub age: String,
}

impl Default for Operator {
    fn default() -> Self {
        Operator {
            callsign: String::new(),
            power: "-".repeat(POWER_BANDS.len()),
            age: String::new(),
        }
    }
}

impl Operator {
    pub fn new(callsign: &str, power: &str, age: &str) -> Self {
        let mut op = Operator {
            callsign: callsign.into(),
            age: age.into(),
            ..Operator::default()
        };
        op.set_power(power);
        op
    }

    pub fn power(&self) -> &str {
        &self.power
    }

    pub fn set_power(&mut self, text: &str) {
        let padded = text.chars().chain(std::iter::repeat('-'));
        self.power = padded
            .take(POWER_BANDS.len())
            .enumerate()
            .map(|(i, c)| if power_codes(i).contains(&c) { c } else { '-' })
            .collect();
    }

    pub fn power_for(&self, band: Band) -> Option<Power> {
        let codes: Vec<char> = self.power.chars().collect();
        let c = match codes[power_index(band)] {
            '-' => codes[0].to_ascii_uppercase(),
            c => c,
        };
        Power::from_code(c)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Operators(pub Vec<Operator>);

impl Operators {
    pub fn get(&self, callsign: &str) -> Option<&Operator> {
        self.0.iter().find(|o| o.callsign == callsign)
    }

    pub fn put(&mut self, op: Operator) {
        match self.0.iter_mut().find(|o| o.callsign == op.callsign) {
            Some(o) => *o = op,
            None => self.0.push(op),
        }
    }

    pub fn power_for(&self, operator: &str, band: Band, station: &Station) -> Option<Power> {
        match self.get(operator) {
            Some(op) => op.power_for(band),
            None => Some(station.power_for(band)),
        }
    }

    pub fn load(section: &Section) -> Self {
        let mut list = Operators::default();
        let count: usize = section
            .get("operators")
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        for i in 0..count {
            let get = |k: &str| {
                section
                    .get(&format!("{k}.{i}"))
                    .cloned()
                    .unwrap_or_default()
            };
            let callsign = get("callsign");
            if callsign.is_empty() || list.get(&callsign).is_some() {
                continue;
            }
            list.0
                .push(Operator::new(&callsign, &get("power"), &get("age")));
        }
        list
    }

    pub fn save(&self, section: &mut Section) {
        let count: usize = section
            .get("operators")
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        for i in 0..count {
            for k in ["callsign", "power", "age"] {
                section.remove(&format!("{k}.{i}"));
            }
        }
        section.insert("operators".into(), self.0.len().to_string());
        for (i, op) in self.0.iter().enumerate() {
            section.insert(format!("callsign.{i}"), op.callsign.clone());
            section.insert(format!("power.{i}"), op.power.clone());
            section.insert(format!("age.{i}"), op.age.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_text_is_padded_and_checked_per_band() {
        let op = Operator::new("JA1ZLO", "HHHHHHHHHHHH", "");
        assert_eq!(op.power(), format!("HHHHHHH{}", "-".repeat(12)));
        let op = Operator::new("JA1ZLO", "hMMMMMMHMMPP", "");
        assert_eq!(op.power(), "-MMMMMM-M-PP-------");
        assert_eq!(Operator::new("X", "", "").power(), "-".repeat(19));
    }

    #[test]
    fn power_of_a_band_falls_back_to_the_first_code() {
        let op = Operator::new("JA1ZLO", "L-H----M-L", "");
        assert_eq!(op.power_for(Band::B1_9), Some(Power::P002));
        assert_eq!(op.power_for(Band::B7), Some(Power::P010));
        assert_eq!(op.power_for(Band::B3_5), Some(Power::P002));
        assert_eq!(op.power_for(Band::B10), Some(Power::P002));
        assert_eq!(op.power_for(Band::B144), Some(Power::P005));
        assert_eq!(op.power_for(Band::B430), Some(Power::P002));
        assert_eq!(op.power_for(Band::B1200), Some(Power::P002));
        let none = Operator::new("JA1ZLO", "", "");
        assert_eq!(none.power_for(Band::B7), None);
    }

    #[test]
    fn unlisted_operators_take_the_station_power() {
        let mut st = Station::default();
        st.power.insert(Band::B7, Power::P005);
        let list = Operators(vec![Operator::new("JA1ZLO", "P", "")]);
        assert_eq!(list.power_for("JA1ZLO", Band::B7, &st), Some(Power::P001));
        assert_eq!(list.power_for("", Band::B7, &st), Some(Power::P005));
        assert_eq!(list.power_for("JA1YWX", Band::B7, &st), Some(Power::P005));
    }

    #[test]
    fn put_replaces_by_callsign() {
        let mut list = Operators::default();
        list.put(Operator::new("JA1ZLO", "H", "20"));
        list.put(Operator::new("JA1YWX", "M", ""));
        list.put(Operator::new("JA1ZLO", "L", "21"));
        assert_eq!(list.0.len(), 2);
        assert_eq!(list.get("JA1ZLO").unwrap().age, "21");
    }

    #[test]
    fn saved_and_loaded() {
        let list = Operators(vec![
            Operator::new("JA1ZLO", "H", "20"),
            Operator::new("JA1YWX", "MMMMMMMM", ""),
        ]);
        let mut section = Section::new();
        Operators(vec![Operator::default(); 3]).save(&mut section);
        list.save(&mut section);
        assert!(!section.contains_key("callsign.2"));
        assert_eq!(Operators::load(&section), list);
        section.insert("operators".into(), "4".into());
        section.insert("callsign.2".into(), "".into());
        section.insert("callsign.3".into(), "JA1ZLO".into());
        assert_eq!(Operators::load(&section), list);
    }
}
