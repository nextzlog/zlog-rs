use egui::{RichText, Ui};
use zlog_serial::PortInfo;
use zlog_ui::{Panel, View};

pub fn describe(port: &PortInfo) -> (String, String) {
    (port.name.clone(), port.detail())
}

pub use zlog_serial::SIMULATOR;

pub fn rows(language: &str, found: &[(String, String)]) -> Vec<(String, String)> {
    let mut rows = vec![(
        SIMULATOR.to_string(),
        texts().get(language, "simulator").to_string(),
    )];
    rows.extend(found.iter().cloned());
    rows
}

#[derive(Default)]
pub struct ComPorts;

zlog_core::texts!();

impl Panel for ComPorts {
    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::Dialog
    }

    fn id(&self) -> &str {
        "ports"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Device
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Ports, 0)
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        vec![zlog_ui::Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::USB)]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn default_size(&self) -> [f32; 2] {
        [380.0, 200.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let scanning = zlog_serial::ports().scanning;
        let state = if scanning {
            texts().get(lang, "scanning")
        } else {
            ""
        };
        if !state.is_empty() {
            ui.weak(state);
        }
        let listed = zlog_serial::ports();
        let found: Vec<_> = match &listed.ports {
            Ok(ports) => ports.iter().map(describe).collect(),
            Err(_) => Vec::new(),
        };
        egui::Grid::new("ports")
            .striped(true)
            .min_col_width(ui.available_width() / 2.0 - ui.spacing().item_spacing.x)
            .show(ui, |ui| {
                for (name, detail) in rows(lang, &found) {
                    ui.label(RichText::new(name).monospace());
                    ui.add(egui::Label::new(detail).wrap());
                    ui.end_row();
                }
            });
        match &listed.ports {
            Ok(ports) if ports.is_empty() && !listed.scanning => {
                ui.label(texts().get(lang, "none"));
            }
            Err(e) => {
                ui.colored_label(zlog_ui::palette::error(ui), e);
            }
            _ => {}
        }
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(580, |_| ComPorts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_simulator_is_listed_first() {
        let found = [("COM3".to_string(), "USB 0403:6001".to_string())];
        let r = rows("ja", &found);
        assert_eq!(r[0].0, SIMULATOR);
        assert!(r[0].1.contains("シミュレーター"));
        assert_eq!(r[1], found[0]);
        assert_eq!(rows("en", &[]).len(), 1);
        let usb = PortInfo {
            usb: Some(zlog_serial::UsbId {
                vid: 0x0403,
                pid: 0x6001,
                serial: None,
            }),
            description: "FTDI".into(),
            ..PortInfo::named("COM3")
        };
        assert_eq!(describe(&usb), ("COM3".into(), "USB 0403:6001 FTDI".into()));
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
