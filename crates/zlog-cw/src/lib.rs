//! CW decoding: a [`Demodulator`] built on rumo, which reads every station in the
//! audio band at once, and a window in the manner of CW4ISR listing them.
//!
//! Callsigns and exchanges that rumo recognises are shown as buttons; pressing one
//! asks the application to put it into the entry row.

use std::collections::BTreeMap;

use egui::{Color32, RichText, Ui};
use rumo::{Event, TokenKind, Verdict};
use zlog_audio::Input;
use zlog_core::Mode;
use zlog_modem::{Demodulator, Signal, Word, WordKind};
use zlog_ui::{EntryAction, Panel, View};

zlog_core::texts!();

/// How many recognised words a signal keeps.
const WORDS_KEPT: usize = 12;

/// CW decoder on rumo.
pub struct CwDecoder {
    rate: u32,
    decoder: rumo::Decoder,
    signals: BTreeMap<u32, Signal>,
}

impl CwDecoder {
    pub fn new(rate: u32) -> Self {
        CwDecoder {
            rate,
            decoder: rumo::Decoder::new(rate),
            signals: BTreeMap::new(),
        }
    }

    fn apply(&mut self, event: Event) {
        let new = |track, freq| Signal {
            id: track,
            freq,
            text: String::new(),
            pending: String::new(),
            words: Vec::new(),
            alive: true,
        };
        match event {
            Event::TrackBorn { track, freq, .. } | Event::TrackConfirmed { track, freq, .. } => {
                self.signals
                    .entry(track)
                    .or_insert_with(|| new(track, freq))
                    .freq = freq;
            }
            Event::Commit {
                track, freq, text, ..
            } => {
                let s = self
                    .signals
                    .entry(track)
                    .or_insert_with(|| new(track, freq));
                s.text.push_str(&text);
                s.freq = freq;
            }
            Event::Tentative {
                track, text, guess, ..
            } => {
                if let Some(s) = self.signals.get_mut(&track) {
                    s.pending = format!("{text}{guess}");
                }
            }
            Event::Token {
                track,
                text,
                kind,
                verdict,
                ..
            } => {
                if let Some(s) = self.signals.get_mut(&track) {
                    let kind = match kind {
                        TokenKind::Callsign => WordKind::Callsign,
                        TokenKind::Exchange => WordKind::Exchange,
                        TokenKind::Procedure => WordKind::Procedure,
                        _ => WordKind::Other,
                    };
                    let sure = matches!(verdict, Verdict::Confirmed | Verdict::Repeated);
                    s.words.push(Word { text, kind, sure });
                    let n = s.words.len();
                    if n > WORDS_KEPT {
                        s.words.drain(..n - WORDS_KEPT);
                    }
                }
            }
            Event::TrackLost {
                track, candidate, ..
            } => {
                if candidate {
                    self.signals.remove(&track);
                } else if let Some(s) = self.signals.get_mut(&track) {
                    s.alive = false;
                    s.pending.clear();
                }
            }
            _ => {}
        }
    }
}

impl Demodulator for CwDecoder {
    fn mode(&self) -> Mode {
        Mode::Cw
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn push(&mut self, samples: &[f32]) {
        for e in self.decoder.push(samples) {
            self.apply(e);
        }
    }

    fn signals(&self) -> Vec<Signal> {
        let mut v: Vec<Signal> = self.signals.values().cloned().collect();
        v.sort_by(|a, b| a.freq.total_cmp(&b.freq));
        v
    }

    fn reset(&mut self) {
        *self = CwDecoder::new(self.rate);
    }
}

/// The CW decoder window: input device, start and stop, and one line per station.
#[derive(Default)]
pub struct CwPanel {
    devices: Vec<String>,
    device: Option<String>,
    input: Option<Input>,
    decoder: Option<CwDecoder>,
    error: Option<String>,
    actions: Vec<EntryAction>,
}

impl CwPanel {
    fn start(&mut self) {
        match Input::open(self.device.as_deref()) {
            Ok(input) => {
                self.decoder = Some(CwDecoder::new(input.sample_rate));
                self.input = Some(input);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
}

impl Panel for CwPanel {
    fn id(&self) -> &str {
        "cw-decoder"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [560.0, 320.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let t = |k| texts().get(lang, k);
        if self.devices.is_empty() {
            self.devices = zlog_audio::devices();
        }
        if let (Some(input), Some(dec)) = (&self.input, self.decoder.as_mut()) {
            dec.push(&input.take());
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        ui.horizontal(|ui| {
            ui.label(t("device"));
            let shown = self
                .device
                .clone()
                .unwrap_or_else(|| t("default_device").into());
            egui::ComboBox::from_id_salt("cw_device")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.device, None, t("default_device"));
                    for d in &self.devices {
                        ui.selectable_value(&mut self.device, Some(d.clone()), d);
                    }
                });
            if self.input.is_none() {
                if ui.button(t("start")).clicked() {
                    self.start();
                }
            } else if ui.button(t("stop")).clicked() {
                self.input = None;
            }
            if ui.button(t("reset")).clicked() {
                if let Some(d) = self.decoder.as_mut() {
                    d.reset();
                }
            }
        });
        if let Some(e) = &self.error {
            ui.colored_label(Color32::RED, texts().format(lang, "error", &[e]));
        }
        if self.input.is_none() {
            ui.weak(t("not_running"));
        }
        let signals = self
            .decoder
            .as_ref()
            .map(|d| d.signals())
            .unwrap_or_default();
        if signals.is_empty() {
            ui.weak(t("no_signals"));
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for s in &signals {
                ui.horizontal_wrapped(|ui| {
                    let freq = RichText::new(format!("{:5.0} Hz", s.freq)).monospace();
                    ui.label(if s.alive { freq.strong() } else { freq.weak() });
                    let tail: String = s
                        .text
                        .chars()
                        .rev()
                        .take(60)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    ui.monospace(tail);
                    ui.label(RichText::new(&s.pending).monospace().color(Color32::GRAY));
                });
                ui.horizontal_wrapped(|ui| {
                    for w in s
                        .words
                        .iter()
                        .filter(|w| matches!(w.kind, WordKind::Callsign | WordKind::Exchange))
                    {
                        let text = RichText::new(&w.text).monospace();
                        let text = if w.sure {
                            text
                        } else {
                            text.color(Color32::from_rgb(180, 120, 0))
                        };
                        if ui.small_button(text).clicked() {
                            self.actions.push(match w.kind {
                                WordKind::Callsign => EntryAction::Callsign(w.text.clone()),
                                _ => EntryAction::Number(w.text.clone()),
                            });
                        }
                    }
                });
                ui.separator();
            }
        });
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keyed tone of `text` at `freq` Hz and `wpm`, with soft edges, at `rate` Hz.
    fn morse(text: &str, freq: f64, wpm: f64, rate: u32) -> Vec<f32> {
        let code = |c: char| match c {
            'A' => ".-",
            'B' => "-...",
            'C' => "-.-.",
            'D' => "-..",
            'E' => ".",
            'F' => "..-.",
            'G' => "--.",
            'H' => "....",
            'I' => "..",
            'J' => ".---",
            'K' => "-.-",
            'L' => ".-..",
            'M' => "--",
            'N' => "-.",
            'O' => "---",
            'P' => ".--.",
            'Q' => "--.-",
            'R' => ".-.",
            'S' => "...",
            'T' => "-",
            'U' => "..-",
            'V' => "...-",
            'W' => ".--",
            'X' => "-..-",
            'Y' => "-.--",
            'Z' => "--..",
            '0' => "-----",
            '1' => ".----",
            '2' => "..---",
            '3' => "...--",
            '4' => "....-",
            '5' => ".....",
            '6' => "-....",
            '7' => "--...",
            '8' => "---..",
            '9' => "----.",
            _ => "",
        };
        let unit = 1.2 / wpm;
        let mut keys: Vec<(bool, f64)> = vec![(false, 1.0)];
        for word in text.split(' ') {
            for c in word.chars() {
                for e in code(c).chars() {
                    keys.push((true, if e == '.' { unit } else { 3.0 * unit }));
                    keys.push((false, unit));
                }
                keys.push((false, 2.0 * unit));
            }
            keys.push((false, 4.0 * unit));
        }
        keys.push((false, 1.0));
        let mut out = Vec::new();
        let mut t = 0.0f64;
        let mut seed = 1u64;
        for (on, dur) in keys {
            let n = (dur * rate as f64) as usize;
            for i in 0..n {
                let edge = ((i.min(n - i) as f64) / (0.005 * rate as f64)).min(1.0);
                let amp = if on { 0.5 * edge } else { 0.0 };
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let noise = ((seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 0.02;
                out.push((amp * (2.0 * std::f64::consts::PI * freq * t).sin() + noise) as f32);
                t += 1.0 / rate as f64;
            }
        }
        out
    }

    #[test]
    fn decodes_a_station_and_finds_its_callsign() {
        let rate = 8000;
        let audio = morse("CQ TEST JA1ZLO JA1ZLO TEST", 700.0, 25.0, rate);
        let mut dec = CwDecoder::new(rate);
        for chunk in audio.chunks(256) {
            dec.push(chunk);
        }
        for e in dec.decoder.flush() {
            dec.apply(e);
        }
        let signals = dec.signals();
        let text: String = signals.iter().map(|s| s.text.clone()).collect();
        assert!(text.contains("JA1ZLO"), "decoded: {text:?}");
        let s = signals.iter().find(|s| s.text.contains("JA1ZLO")).unwrap();
        assert!((s.freq - 700.0).abs() < 20.0, "freq {}", s.freq);
        assert!(
            s.words
                .iter()
                .any(|w| w.kind == WordKind::Callsign && w.text == "JA1ZLO"),
            "words: {:?}",
            s.words
        );
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
