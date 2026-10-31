use std::sync::{Arc, Mutex};

use zlog_ui::device::{DeviceDriver, PortHub};
use zlog_ui::Section;

pub use zlog_serial::SIMULATOR;

pub fn with_simulator(ports: Vec<String>) -> Vec<String> {
    let mut names = vec![SIMULATOR.to_string()];
    names.extend(ports);
    names
}

pub use haml::sim::so2r::SimCommand;
pub use haml::{Rx, So2r, So2rCaps};

pub trait So2rDrv: DeviceDriver {
    fn open(&self, settings: &Section, ports: &PortHub) -> Result<Box<dyn So2r>, String>;

    fn explain(&self, error: &str, _language: &str) -> String {
        error.into()
    }
}

#[derive(Default)]
pub struct SimSo2rDrv {
    pub log: Arc<Mutex<Vec<SimCommand>>>,
}

fn rx_label(rx: Rx) -> &'static str {
    match rx {
        Rx::Rig1 => "RIG1",
        Rx::Rig2 => "RIG2",
        Rx::Both => "Both",
    }
}

pub fn shown(language: &str, tx: u8, rx: Rx, blend: Option<u8>) -> String {
    let t = |k| crate::texts().get(language, k);
    let blend = match blend {
        Some(p) => format!("{p}%"),
        None => t("off").into(),
    };
    format!("TX RIG{tx}  RX {}  {} {blend}", rx_label(rx), t("af_blend"))
}

impl DeviceDriver for SimSo2rDrv {
    fn id(&self) -> &str {
        "sim"
    }

    fn title(&self, language: &str) -> String {
        crate::texts().get(language, "device.sim").into()
    }

    fn simulated(&self) -> bool {
        true
    }

    fn status(&self, language: &str) -> Option<String> {
        let (mut tx, mut rx, mut blend) = (1, Rx::Rig1, None);
        for c in self.log.lock().unwrap_or_else(|e| e.into_inner()).iter() {
            match *c {
                SimCommand::Tx(r) => tx = r,
                SimCommand::Rx(r) => rx = r,
                SimCommand::Blend(on, p) => blend = on.then_some(p),
            }
        }
        Some(shown(language, tx, rx, blend))
    }
}

impl So2rDrv for SimSo2rDrv {
    fn open(&self, _settings: &Section, _ports: &PortHub) -> Result<Box<dyn So2r>, String> {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).clear();
        Ok(Box::new(haml::sim::SimSo2r::new(self.log.clone())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_simulated_box_shows_and_records_what_it_is_told() {
        let drv = SimSo2rDrv::default();
        assert!(drv.simulated() && drv.settings("en").is_empty());
        let mut b = drv.open(&Section::new(), &PortHub::new()).unwrap();
        assert_eq!(drv.status("en").unwrap(), "TX RIG1  RX RIG1  AF blend OFF");
        b.select_rig(2);
        b.rx(Rx::Both);
        b.blend(true, 50);
        assert_eq!(drv.status("en").unwrap(), "TX RIG2  RX Both  AF blend 50%");
        assert!(b.caps().auto_rx && b.error().is_none());
        let log = drv.log.lock().unwrap().clone();
        assert_eq!(
            log,
            [
                SimCommand::Tx(2),
                SimCommand::Rx(Rx::Rig2),
                SimCommand::Rx(Rx::Both),
                SimCommand::Blend(true, 50)
            ]
        );
    }

    #[test]
    fn the_simulator_comes_first_in_the_port_list() {
        assert_eq!(with_simulator(vec!["COM3".into()]), [SIMULATOR, "COM3"]);
    }
}
