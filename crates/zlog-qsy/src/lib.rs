//! The QSY indicator (`UQsyInfo`, `TMainForm.Update10MinTimer`): a square window
//! showing, for the own TX, the time left until it may change band again (the
//! countdown rule) or its band changes of this clock hour (the QSY count rule),
//! green when a band change is allowed now and yellow when not; grey `None` with
//! both rules off (Options, multi-operator QSY rules).
//!
//! Everything is derived from [`View::log`] and its [`zlog_core::QsyRule`]; see
//! [`indicator`].
//!
//! Not implemented: opening the window by itself when a rule is on (zLog shows it
//! on the first update), and hiding the TX number for single-operator categories
//! (zLog RS has no category; the TX number is always shown).

pub mod indicator;

use std::time::Duration;

use egui::{Color32, RichText, Ui};
use zlog_core::{DelphiTime, Log};
use zlog_ui::{Panel, View};

use indicator::{indicator, Indicator, Shown};

zlog_core::texts!();

/// The QSY indicator window.
#[derive(Default)]
pub struct QsyInfo;

/// The time now in the zone of `log`'s times (UTC or local, `CurrentTime`).
fn now(log: &Log) -> DelphiTime {
    use chrono::{Datelike, Timelike};
    let t = if log.utc_offset_minutes().is_none() {
        chrono::Utc::now().naive_utc()
    } else {
        chrono::Local::now().naive_local()
    };
    DelphiTime::from_ymd_hms(
        t.year(),
        t.month(),
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
    )
}

/// The text of the main line in `lang`.
fn text(lang: &str, shown: Shown) -> String {
    match shown {
        Shown::Off => texts().get(lang, "none").into(),
        Shown::QsyOk => texts().get(lang, "qsy_ok").into(),
        Shown::Left(s) => Shown::left_text(s),
        Shown::Count(n) => texts().format(lang, "count", &[&n]),
    }
}

/// Background and text colours (`SetQsyInfo`): lime and blue when a band change is
/// allowed, yellow and red when not, the window's own when off.
fn colours(ui: &Ui, i: Indicator) -> (Color32, Color32) {
    match (i.shown, i.ok) {
        (Shown::Off, _) => (ui.visuals().panel_fill, ui.visuals().text_color()),
        (_, true) => (Color32::from_rgb(0, 255, 0), Color32::BLUE),
        (_, false) => (Color32::YELLOW, Color32::RED),
    }
}

impl Panel for QsyInfo {
    fn id(&self) -> &str {
        "qsy"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Device
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Assist, 0)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::CLOCK_COUNTER_CLOCKWISE)]
    }

    /// Hidden until chosen: the QSY rules are for multi-operator stations.
    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [180.0, 180.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let tx = view.station.tx_nr;
        let i = indicator(view.log, tx, now(view.log));
        let (fill, colour) = colours(ui, i);
        let tx = texts().format(lang, "tx", &[&tx]);
        let height = ui.available_height();
        let size = (height / 4.0).clamp(12.0, 64.0);
        for (line, part) in [(text(lang, i.shown), 0.6), (tx, 0.4)] {
            egui::Frame::new().fill(fill).show(ui, |ui| {
                ui.set_min_size(egui::vec2(ui.available_width(), height * part - 8.0));
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new(line).size(size).color(colour).strong());
                });
            });
        }
        ui.ctx().request_repaint_after(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn main_line_texts() {
        assert_eq!(text("en", Shown::Off), "None");
        assert_eq!(text("ja", Shown::QsyOk), "QSY OK");
        assert_eq!(text("en", Shown::Left(75)), "01:15");
        assert_eq!(text("en", Shown::Count(3)), "QSY# 3");
    }
}
