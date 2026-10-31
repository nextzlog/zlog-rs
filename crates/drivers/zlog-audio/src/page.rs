use std::time::Duration;

use egui::Ui;
use zlog_ui::device::{AudioDrv, AudioIo, DeviceDriver, DriverChoice, PortHub, SOUND_SECTION};
use zlog_ui::{Section, SettingsPage};

use crate::texts;

pub struct SoundPage {
    drivers: Vec<Box<dyn AudioDrv>>,
    ports: PortHub,
    device: Option<DriverChoice>,
    test: Option<Box<dyn AudioIo>>,
    level: f32,
    error: Option<String>,
    samples: Vec<f32>,
}

impl SoundPage {
    pub fn new(drivers: Vec<Box<dyn AudioDrv>>, ports: PortHub) -> Self {
        SoundPage {
            drivers,
            ports,
            device: None,
            test: None,
            level: 0.0,
            error: None,
            samples: Vec::new(),
        }
    }

    pub fn testing(&self) -> bool {
        self.test.is_some()
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn start_test(&mut self) {
        self.error = None;
        let Some(d) = self.device.as_ref() else {
            return;
        };
        let Some(drv) = d.find(&self.drivers) else {
            self.error = Some(texts().get("en", "test.none").into());
            return;
        };
        match drv.open(&d.section(), &self.ports) {
            Ok(io) => self.test = Some(io),
            Err(e) => self.error = Some(e),
        }
    }

    pub fn stop_test(&mut self) {
        if let Some(mut io) = self.test.take() {
            io.stop();
        }
        self.level = 0.0;
    }

    pub fn measure(&mut self) {
        let Some(io) = self.test.as_mut() else {
            return;
        };
        self.samples.clear();
        io.record(&mut self.samples);
        let now = zlog_ui::device::peak(&self.samples);
        self.level = now.max(self.level * 0.9);
    }

    pub fn tone(&mut self) {
        if self.test.is_none() {
            self.start_test();
        }
        if let Some(io) = self.test.as_mut() {
            let rate = io.sample_rate().max(1);
            let step = std::f32::consts::TAU * 700.0 / rate as f32;
            let samples: Vec<f32> = (0..rate).map(|i| 0.3 * (step * i as f32).sin()).collect();
            io.play(&samples);
        }
    }

    pub fn test_ui(&mut self, ui: &mut Ui, lang: &str) {
        let t = |k| texts().get(lang, k);
        ui.horizontal_wrapped(|ui| {
            let label = if self.testing() {
                "test.stop"
            } else {
                "test.start"
            };
            if ui
                .button(t(label))
                .on_hover_text(t("test.start.tip"))
                .clicked()
            {
                if self.testing() {
                    self.stop_test();
                } else {
                    self.start_test();
                }
            }
            if ui
                .button(t("test.tone"))
                .on_hover_text(t("test.tone.tip"))
                .clicked()
            {
                self.tone();
            }
        });
        if self.testing() {
            self.measure();
            zlog_ui::device::level_meter(ui, self.level, lang);
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        if let Some(e) = &self.error {
            ui.colored_label(ui.visuals().warn_fg_color, e);
        }
    }
}

impl SettingsPage for SoundPage {
    fn id(&self) -> &str {
        SOUND_SECTION
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "page").into()
    }

    fn ui(&mut self, ui: &mut Ui, section: &mut Section, lang: &str) {
        let t = |k| texts().get(lang, k);
        ui.weak(t("page.note"));
        let list: Vec<&dyn DeviceDriver> = self.drivers.iter().map(|d| &**d as _).collect();
        let before = self.device.clone();
        zlog_ui::device::page_ui(
            ui,
            &mut self.device,
            section,
            "sound",
            (lang, t("page.device"), t("page.none")),
            &list,
        );
        if before.is_some() && before != self.device {
            self.stop_test();
        }
        ui.separator();
        self.test_ui(ui, lang);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Fake(Arc<Mutex<Vec<f32>>>);

    impl AudioIo for Fake {
        fn sample_rate(&self) -> u32 {
            8000
        }
        fn play(&mut self, samples: &[f32]) {
            self.0.lock().unwrap().extend_from_slice(samples);
        }
        fn stop(&mut self) {}
        fn playing(&self) -> bool {
            false
        }
        fn record(&mut self, out: &mut Vec<f32>) {
            out.extend([0.0, 0.1, -0.5, 0.2]);
        }
    }

    struct FakeDrv(Arc<Mutex<Vec<f32>>>);

    impl DeviceDriver for FakeDrv {
        fn id(&self) -> &str {
            "sound"
        }
        fn title(&self, _language: &str) -> String {
            "Fake".into()
        }
    }

    impl AudioDrv for FakeDrv {
        fn open(&self, _s: &Section, _p: &PortHub) -> Result<Box<dyn AudioIo>, String> {
            Ok(Box::new(Fake(self.0.clone())))
        }
    }

    #[test]
    fn the_page_tests_input_with_a_level_and_plays_a_tone() {
        let played = Arc::new(Mutex::new(Vec::new()));
        let mut page = SoundPage::new(vec![Box::new(FakeDrv(played.clone()))], PortHub::new());
        assert_eq!(page.id(), "sound");
        let mut section = Section::new();
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| page.ui(ui, &mut section, "en"));
        out.textures_delta.clear();
        assert_eq!(section["device"], "sound");
        page.start_test();
        assert!(page.testing());
        page.measure();
        assert!((page.level() - 0.5).abs() < 1e-6);
        page.tone();
        assert_eq!(played.lock().unwrap().len(), 8000);
        page.stop_test();
        assert!(!page.testing() && page.level() == 0.0);
    }

    #[test]
    fn modes_follow_the_sound_page_unless_they_choose_their_own() {
        use zlog_ui::device::{own_sound, sound_section};
        let mut shared = Section::new();
        shared.insert("device".into(), "sound".into());
        shared.insert("sound.input".into(), "USB codec".into());
        let others: zlog_ui::Sections = [(SOUND_SECTION.to_string(), shared.clone())].into();
        let mut own = Section::new();
        own.insert("device".into(), "loopback".into());
        assert_eq!(sound_section(&own, &others), shared);
        assert!(
            own_sound(&own, None),
            "an old setting without the page keeps working"
        );
        own.insert("own_sound".into(), "true".into());
        assert_eq!(sound_section(&own, &others)["device"], "loopback");
    }
}
