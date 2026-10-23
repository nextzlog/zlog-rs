//! CW decoding: the CW4ISR window of the [cw4i](https://github.com/nextzlog/cw4i)
//! crate, which reads every station in the audio band at once.
//!
//! A callsign or number clicked in the window goes straight into the entry row,
//! where the standalone CW4ISR copies it to the clipboard.

use cw4i::{Cw4i, Pick};
use egui::Ui;
use zlog_ui::{EntryAction, Panel, View};

zlog_core::texts!();

/// The CW4ISR window.
#[derive(Default)]
pub struct CwPanel {
    decoder: Cw4i,
}

/// The entry-row request for a picked word.
pub fn action(pick: Pick) -> EntryAction {
    match pick {
        Pick::Callsign(c) => EntryAction::Callsign(c),
        Pick::Number(n) => EntryAction::Number(n),
    }
}

impl Panel for CwPanel {
    fn id(&self) -> &str {
        "cw-decoder"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Device
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Cw, 1)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::EAR)]
    }

    fn default_size(&self) -> [f32; 2] {
        [720.0, 480.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        if self.decoder.language != view.language {
            self.decoder.language = view.language.into();
        }
        self.decoder.process();
        self.decoder.ui(ui);
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        self.decoder.take_picks().into_iter().map(action).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_fill_the_entry_row() {
        assert_eq!(
            action(Pick::Callsign("JA1ZLO".into())),
            EntryAction::Callsign("JA1ZLO".into())
        );
        assert_eq!(
            action(Pick::Number("13M".into())),
            EntryAction::Number("13M".into())
        );
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
