use egui::Ui;
use zlog_ui::device::{DeviceDriver, DriverChoice, NONE};
use zlog_ui::{Section, SettingsPage};

use crate::{texts, So2rDrv};

pub const SECTION: &str = "panel.so2r";

pub struct So2rOptions {
    drivers: Vec<Box<dyn So2rDrv>>,
    device: Option<DriverChoice>,
}

impl So2rOptions {
    pub fn new(drivers: Vec<Box<dyn So2rDrv>>) -> Self {
        So2rOptions {
            drivers,
            device: None,
        }
    }
}

impl SettingsPage for So2rOptions {
    fn id(&self) -> &str {
        SECTION
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "options").into()
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut Section, lang: &str) {
        let t = |k| texts().get(lang, k);
        let list: Vec<&dyn DeviceDriver> = self.drivers.iter().map(|d| &**d as _).collect();
        let labels = (lang, t("device"), t("none"));
        zlog_ui::device::page_ui(ui, &mut self.device, section, NONE, labels, &list);
    }
}
