pub mod indicator;

use std::time::Duration;

use egui::{Color32, RichText, Ui};
use zlog_core::{DelphiTime, Log};
use zlog_ui::{Panel, View};

use indicator::{indicator, Indicator, Shown};

zlog_core::texts!();

#[derive(Default)]
pub struct QsyInfo;

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

fn text(lang: &str, shown: Shown) -> String {
    match shown {
        Shown::Off => texts().get(lang, "none").into(),
        Shown::QsyOk => texts().get(lang, "qsy_ok").into(),
        Shown::Left(s) => Shown::left_text(s),
        Shown::Count(n) => texts().format(lang, "count", &[&n]),
    }
}

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
        let room = ui.available_size() - egui::vec2(0.0, 2.0);
        let size = (room.y / 4.0).clamp(12.0, 64.0);
        egui::Frame::new().fill(fill).show(ui, |ui| {
            ui.set_min_size(room);
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.add_space(((room.y - size * 2.2) / 2.0).max(0.0));
                let main = RichText::new(text(lang, i.shown)).size(size).color(colour);
                let w = room.x;
                ui.add_sized([w, size * 1.3], egui::Label::new(main.strong()));
                let tx = RichText::new(tx).size(size * 0.6).color(colour);
                ui.add_sized([w, size], egui::Label::new(tx));
            });
        });
        ui.ctx().request_repaint_after(Duration::from_millis(500));
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(430, |_| QsyInfo);
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
