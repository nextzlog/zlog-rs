use std::sync::{Arc, Mutex};

use haml::sim::OtrspSim;
use haml::so2r::otrsp::{Otrsp, BAUD};
use zlog_so2r::{So2r, So2rDrv, SIMULATOR};
use zlog_ui::device::{DeviceDriver, PortHub, SettingField, SettingKind};
use zlog_ui::Section;

zlog_core::texts!();

#[derive(Default)]
pub struct OtrspDrv {
    pub sim: Arc<Mutex<Option<OtrspSim>>>,
}

impl DeviceDriver for OtrspDrv {
    fn id(&self) -> &str {
        "otrsp"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn settings(&self, language: &str) -> Vec<SettingField> {
        vec![SettingField::new(
            "port",
            texts().get(language, "port"),
            SettingKind::Port(zlog_so2r::with_simulator(zlog_serial::port_names())),
        )]
    }

    fn status(&self, language: &str) -> Option<String> {
        let s = self
            .sim
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()?
            .state();
        Some(format!(
            "{}: TX RIG{}  RX RIG{}",
            texts().get(language, "simulator"),
            s.tx,
            s.rx
        ))
    }
}

impl So2rDrv for OtrspDrv {
    fn open(&self, settings: &Section, _ports: &PortHub) -> Result<Box<dyn So2r>, String> {
        let port = settings.get("port").map_or("", |s| s.as_str());
        let sim = (port == SIMULATOR).then(OtrspSim::new);
        *self.sim.lock().unwrap_or_else(|e| e.into_inner()) = sim.clone();
        if let Some(sim) = sim {
            return Ok(Box::new(Otrsp::new(Box::new(sim.transport()))));
        }
        let t = zlog_serial::open(port, BAUD, 1)?;
        Ok(Box::new(Otrsp::new(Box::new(t))))
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.driver::<dyn zlog_so2r::So2rDrv>(20, || Box::<OtrspDrv>::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_so2r::Rx;
    use zlog_ui::tx::TxHub;

    #[test]
    fn the_driver_keeps_the_old_id() {
        let d = OtrspDrv::default();
        assert_eq!(d.id(), "otrsp");
        let mut s = Section::new();
        s.insert("port".into(), "/nonexistent/zlog-so2r-otrsp".into());
        assert!(d.open(&s, &PortHub::new()).is_err());
    }

    #[test]
    fn the_simulator_port_shows_what_the_box_was_told() {
        let d = OtrspDrv::default();
        let SettingKind::Port(names) = &d.settings("en")[0].kind else {
            panic!("port field");
        };
        assert_eq!(names[0], SIMULATOR);
        let mut s = Section::new();
        s.insert("port".into(), SIMULATOR.into());
        let mut b = d.open(&s, &PortHub::new()).unwrap();
        b.select_rig(2);
        assert_eq!(d.status("en").unwrap(), "Simulator: TX RIG2  RX RIG2");
        b.rx(Rx::Rig1);
        assert_eq!(d.status("ja").unwrap(), "シミュレーター: TX RIG2  RX RIG1");
        assert!(b.error().is_none() && !b.caps().auto_rx);
    }

    #[test]
    fn the_arbiter_tells_the_box_only_the_tx_changes() {
        let d = OtrspDrv::default();
        let mut s = Section::new();
        s.insert("port".into(), SIMULATOR.into());
        let tx = TxHub::default();
        tx.set_so2r(Some(d.open(&s, &PortHub::new()).unwrap()));
        let sim = d.sim.lock().unwrap().clone().unwrap();
        tx.send(zlog_ui::tx::TxCommand::Focus(1));
        for radio in [2, 2, 1] {
            tx.transmit_on(radio);
        }
        assert_eq!(sim.state().lines, ["TX1", "RX1", "TX2", "TX1"]);
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
