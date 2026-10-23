//! The running frequencies of the stations on the network (`TFreqList`,
//! UFreqList.pas): the latest `FREQ` line of each PC, this one's included.
//!
//! zLog keeps 31 slots. A station sits in the slot of its entry row's band (the
//! ordinal that starts the line), so two stations on one band keep only the later;
//! the multiplier station (ordinal `30`) takes the first free slot above the highest
//! band. A new line from a PC first removes that PC's old line, matched by the PC
//! name in brackets ([`crate::protocol::freq_pc_name`]).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::{RichText, ScrollArea, Ui};
use zlog_core::Band;
use zlog_ui::{Command, MenuGroup, Panel, View};

use crate::protocol::freq_pc_name;
use crate::session::Session;
use crate::texts;

/// `FreqArray`: slots 0 to 30.
const SLOTS: usize = 31;

/// `ProcessFreqData` ignores lines shorter than this.
const MIN_LENGTH: usize = 30;

#[derive(Debug, Clone, PartialEq)]
pub struct FreqList {
    slots: [Option<String>; SLOTS],
}

impl Default for FreqList {
    fn default() -> Self {
        FreqList {
            slots: std::array::from_fn(|_| None),
        }
    }
}

impl FreqList {
    /// `ProcessFreqData`: take one `FREQ` line (without `FREQ `). A line whose
    /// first two characters are no number is dropped (zLog's `StrToInt` raises
    /// there and the line is lost too).
    pub fn take(&mut self, line: &str) {
        if line.chars().count() < MIN_LENGTH {
            return;
        }
        let ordinal: String = line.chars().take(2).collect();
        let Ok(ordinal) = ordinal.trim_end().parse::<usize>() else {
            return;
        };
        let pc = freq_pc_name(line);
        for slot in &mut self.slots {
            if slot.as_deref().is_some_and(|s| freq_pc_name(s) == pc) {
                *slot = None;
            }
        }
        let highest = Band::HIGHEST as usize;
        if ordinal <= highest {
            self.slots[ordinal] = Some(line.to_string());
        } else if let Some(free) = self.slots[highest + 1..].iter_mut().find(|s| s.is_none()) {
            *free = Some(line.to_string());
        }
    }

    /// `ClearBtnClick`.
    pub fn clear(&mut self) {
        *self = FreqList::default();
    }

    /// `UpdateData`: the lines in slot order, each without its first three
    /// characters (the band ordinal and its blank).
    pub fn lines(&self) -> Vec<String> {
        self.slots
            .iter()
            .flatten()
            .map(|s| s.chars().skip(3).collect())
            .collect()
    }
}

/// The Running Frequencies window: the list and its Clear button.
pub struct FreqPanel {
    session: Arc<Mutex<Session>>,
}

impl Default for FreqPanel {
    fn default() -> Self {
        FreqPanel::new(crate::shared())
    }
}

impl FreqPanel {
    pub fn new(session: Arc<Mutex<Session>>) -> Self {
        FreqPanel { session }
    }
}

impl Panel for FreqPanel {
    fn id(&self) -> &str {
        "link-freq"
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Network
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Link, 1)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "freq.title").into()
    }

    /// In zLog's Z-Server menu, without a toolbar button; here one, hidden at first.
    fn commands(&self, language: &str) -> Vec<Command> {
        vec![Command::window(
            texts().get(language, "freq.toolbar"),
            texts().get(language, "freq.tip"),
        )
        .icon(zlog_ui::icon::RADIO)]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [420.0, 240.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = texts();
        let lang = view.language;
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        let mut s = self.session.lock().unwrap();
        if ui.button(t.get(lang, "freq.clear")).clicked() {
            s.freqs.clear();
        }
        ui.separator();
        let lines = s.freqs.lines();
        ScrollArea::vertical()
            .id_salt("link-freq")
            .auto_shrink(false)
            .show(ui, |ui| {
                for l in &lines {
                    ui.label(RichText::new(l).monospace());
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::Mode;

    fn line(band: Band, multi: bool, khz: u64, pc: &str) -> String {
        let l = crate::protocol::freq(
            band,
            band,
            multi,
            khz * 1000,
            Mode::Cw,
            true,
            "12:00:00",
            pc,
        );
        l["#ZLOG# FREQ ".len()..].to_string()
    }

    #[test]
    fn one_line_per_pc_in_band_order() {
        let mut f = FreqList::default();
        f.take(&line(Band::B14, false, 14_020, "PC2"));
        f.take(&line(Band::B7, false, 7_010, "PC1"));
        assert_eq!(f.lines().len(), 2);
        assert!(f.lines()[0].starts_with("7 "));
        // PC1 moves to 21 MHz: its 7 MHz line goes.
        f.take(&line(Band::B21, false, 21_030, "PC1"));
        let lines = f.lines();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("14 ") && lines[1].starts_with("21 "));
    }

    /// Two stations on one band: the later takes the slot, as in zLog.
    #[test]
    fn a_band_holds_one_station() {
        let mut f = FreqList::default();
        f.take(&line(Band::B7, false, 7_010, "PC1"));
        f.take(&line(Band::B7, false, 7_020, "PC2"));
        assert_eq!(f.lines().len(), 1);
        assert!(f.lines()[0].ends_with("[PC2]"));
    }

    /// Multiplier stations (ordinal 30) fill the slots above the highest band.
    #[test]
    fn multiplier_stations_take_free_slots() {
        let mut f = FreqList::default();
        f.take(&line(Band::B7, true, 7_010, "M1"));
        f.take(&line(Band::B14, true, 14_010, "M2"));
        f.take(&line(Band::B1_9, false, 1_910, "PC1"));
        let lines = f.lines();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].ends_with("[PC1]"));
        assert!(lines[1].ends_with("[M1]") && lines[2].ends_with("[M2]"));
        f.clear();
        assert!(f.lines().is_empty());
    }

    #[test]
    fn short_or_unnumbered_lines_are_dropped() {
        let mut f = FreqList::default();
        f.take("2  7  short");
        f.take("xx 7        7025.0 CW   CQ 12:00:00  [PC1]");
        assert!(f.lines().is_empty());
    }
}
