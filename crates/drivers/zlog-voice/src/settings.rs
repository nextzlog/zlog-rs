use std::path::Path;
use std::time::Duration;

use zlog_ui::Section;

use crate::playback::Timing;

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub files: [String; 12],
    pub comments: [String; 12],
    pub ptt: bool,
    pub cq_repeat: f64,
    pub cq_max: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            files: Default::default(),
            comments: Default::default(),
            ptt: false,
            cq_repeat: 2.0,
            cq_max: 15,
        }
    }
}

pub(crate) fn get<T: std::str::FromStr>(s: &Section, key: &str, default: T) -> T {
    s.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Settings {
    pub fn load(s: &Section) -> Self {
        let d = Settings::default();
        let text = |key: &str| s.get(key).cloned().unwrap_or_default();
        Settings {
            files: std::array::from_fn(|n| text(&format!("file{}", n + 1))),
            comments: std::array::from_fn(|n| text(&format!("comment{}", n + 1))),
            ptt: get(s, "ptt", d.ptt),
            cq_repeat: get(s, "cq_repeat", d.cq_repeat).clamp(0.0, 60.0),
            cq_max: get(s, "cq_max", d.cq_max).max(1),
        }
    }

    pub fn save(&self, s: &mut Section) {
        let mut put = |k: &str, v: String| {
            s.insert(k.into(), v);
        };
        for n in 0..12 {
            put(&format!("file{}", n + 1), self.files[n].clone());
            put(&format!("comment{}", n + 1), self.comments[n].clone());
        }
        put("ptt", self.ptt.to_string());
        put("cq_repeat", self.cq_repeat.to_string());
        put("cq_max", self.cq_max.to_string());
    }

    pub fn file(&self, n: usize) -> Option<&Path> {
        let f = self.files.get(n)?.trim();
        (!f.is_empty()).then_some(Path::new(f))
    }

    pub(crate) fn timing(&self) -> Timing {
        let delays = zlog_ui::tx::hub().delays();
        Timing {
            ptt: self.ptt,
            before: delays.before,
            after: delays.after,
            interval: Duration::from_secs(self.cq_repeat.clamp(0.0, 60.0).trunc() as u64),
            max: self.cq_max.max(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let mut s = Settings {
            ptt: true,
            cq_repeat: 3.5,
            cq_max: 4,
            ..Settings::default()
        };
        s.files[2] = "/voice/f3.wav".into();
        s.comments[2] = "TU".into();
        let mut sec = Section::new();
        s.save(&mut sec);
        assert_eq!(Settings::load(&sec), s);
        assert_eq!(Settings::load(&Section::new()), Settings::default());
    }

    #[test]
    fn ptt_delays_come_from_the_one_transmitter_setting() {
        let d = zlog_ui::tx::hub().delays();
        let t = Settings::default().timing();
        assert_eq!((t.before, t.after), (d.before, d.after));
    }

    #[test]
    fn memories_are_assigned_to_function_keys() {
        let mut s = Settings::default();
        assert_eq!(s.file(0), None);
        s.files[0] = " /voice/cq.wav ".into();
        s.files[11] = "f12.wav".into();
        assert_eq!(s.file(0), Some(Path::new("/voice/cq.wav")));
        assert_eq!(s.file(11), Some(Path::new("f12.wav")));
        assert_eq!(s.file(1), None);
        assert_eq!(s.file(12), None);
    }
}
