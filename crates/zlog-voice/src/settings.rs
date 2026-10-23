//! The settings of the voice memories (section `Voice` of zLog.ini).

use std::path::Path;
use std::time::Duration;

use zlog_ui::Section;

use crate::playback::Timing;

/// The settings of the panel (section `Voice` of zLog.ini and `PTTEnabledPH`,
/// `PTTBeforePH`, `PTTAfterPH` of `Hardware`).
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// WAV file and comment of F1 to F12 (`F#1`, `C#1`).
    pub files: [String; 12],
    pub comments: [String; 12],
    /// Output and input devices by name, empty for the default ones.
    pub output: String,
    pub input: String,
    /// PTT on before each memory and off after it, through the keyer.
    pub ptt: bool,
    /// PTT lead and tail in milliseconds (zLog default 25 and 0).
    pub ptt_before_ms: u32,
    pub ptt_after_ms: u32,
    /// Seconds between CQs and the most CQs in a row (`CQRepeat`, `CQMax`).
    pub cq_repeat: f64,
    pub cq_max: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            files: Default::default(),
            comments: Default::default(),
            output: String::new(),
            input: String::new(),
            ptt: false,
            ptt_before_ms: 25,
            ptt_after_ms: 0,
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
            output: text("output"),
            input: text("input"),
            ptt: get(s, "ptt", d.ptt),
            ptt_before_ms: get(s, "ptt_before", d.ptt_before_ms),
            ptt_after_ms: get(s, "ptt_after", d.ptt_after_ms),
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
        put("output", self.output.clone());
        put("input", self.input.clone());
        put("ptt", self.ptt.to_string());
        put("ptt_before", self.ptt_before_ms.to_string());
        put("ptt_after", self.ptt_after_ms.to_string());
        put("cq_repeat", self.cq_repeat.to_string());
        put("cq_max", self.cq_max.to_string());
    }

    /// The file of memory `n` (0..12) when one is assigned.
    pub fn file(&self, n: usize) -> Option<&Path> {
        let f = self.files.get(n)?.trim();
        (!f.is_empty()).then_some(Path::new(f))
    }

    pub(crate) fn timing(&self) -> Timing {
        Timing {
            ptt: self.ptt,
            before: Duration::from_millis(self.ptt_before_ms as u64),
            after: Duration::from_millis(self.ptt_after_ms as u64),
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
            ptt_before_ms: 100,
            cq_repeat: 3.5,
            cq_max: 4,
            output: "USB Audio".into(),
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
