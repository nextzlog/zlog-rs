//! Serial ports present on this computer, to find which one a rig or keyer is on.
//!
//! Port of the comportlist plugin (MIT, jucky154). Uses the `serialport` crate, so it
//! lists COM ports on Windows and `/dev/cu.*` devices on macOS. USB ports show the
//! vendor and product, which tells an Icom or Yaesu interface from other devices.

use egui::{RichText, Ui};
use serialport::{SerialPortInfo, SerialPortType};
use zlog_ui::{Panel, View};

/// One line of the list: port name and a description of the device.
pub fn describe(port: &SerialPortInfo) -> (String, String) {
    let detail = match &port.port_type {
        SerialPortType::UsbPort(usb) => {
            let what = [usb.manufacturer.as_deref(), usb.product.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            format!("USB {:04X}:{:04X} {what}", usb.vid, usb.pid)
        }
        SerialPortType::BluetoothPort => "Bluetooth".into(),
        SerialPortType::PciPort => "PCI".into(),
        SerialPortType::Unknown => String::new(),
    };
    (port.port_name.clone(), detail)
}

#[derive(Default)]
pub struct ComPorts {
    ports: Option<Result<Vec<(String, String)>, String>>,
}

impl ComPorts {
    fn scan(&mut self) {
        self.ports = Some(
            serialport::available_ports()
                .map(|ps| ps.iter().map(describe).collect())
                .map_err(|e| e.to_string()),
        );
    }
}

zlog_core::texts!();

impl Panel for ComPorts {
    fn id(&self) -> &str {
        "comports"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [380.0, 200.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        if self.ports.is_none() || ui.button(texts().get(lang, "rescan")).clicked() {
            self.scan();
        }
        match &self.ports {
            Some(Ok(ports)) if ports.is_empty() => {
                ui.label(texts().get(lang, "none"));
            }
            Some(Ok(ports)) => {
                egui::Grid::new("ports").striped(true).show(ui, |ui| {
                    for (name, detail) in ports {
                        ui.label(RichText::new(name).monospace());
                        ui.label(detail);
                        ui.end_row();
                    }
                });
            }
            Some(Err(e)) => {
                ui.colored_label(egui::Color32::RED, e);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
