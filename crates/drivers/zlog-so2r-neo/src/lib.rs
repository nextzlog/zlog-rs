use std::sync::{Arc, Mutex};

use haml::sim::NeoSim;
use haml::so2r::neo::Neo;
use zlog_so2r::{Rx, So2r, So2rDrv, SIMULATOR};
use zlog_ui::device::{DeviceDriver, PortHub, SettingField, SettingKind, KEYER_PORT};
use zlog_ui::Section;

zlog_core::texts!();

#[derive(Default)]
pub struct NeoDrv {
    pub sim: Arc<Mutex<Option<NeoSim>>>,
}

impl DeviceDriver for NeoDrv {
    fn id(&self) -> &str {
        "neo"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "neo").into()
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
        let t = |k| texts().get(language, k);
        let rx = match s.rx {
            Rx::Rig1 => "RIG1",
            Rx::Rig2 => "RIG2",
            Rx::Both => "Both",
        };
        let blend = if s.blend {
            format!("{}%", s.percent())
        } else {
            t("off").into()
        };
        Some(format!(
            "{}: TX RIG{}  RX {rx}  {} {blend}",
            t("simulator"),
            s.tx,
            t("af_blend")
        ))
    }
}

impl So2rDrv for NeoDrv {
    fn open(&self, settings: &Section, _ports: &PortHub) -> Result<Box<dyn So2r>, String> {
        let port = settings.get("port").map_or("", |s| s.as_str());
        let sim = (port == SIMULATOR).then(NeoSim::new);
        *self.sim.lock().unwrap_or_else(|e| e.into_inner()) = sim.clone();
        if let Some(sim) = sim {
            return Ok(Box::new(Neo::on_port(Box::new(sim.transport()))));
        }
        let t = zlog_serial::open(port, 1200, 2)?;
        Ok(Box::new(Neo::on_port(Box::new(t))))
    }
}

#[derive(Default)]
pub struct NeoKeyerDrv;

impl DeviceDriver for NeoKeyerDrv {
    fn id(&self) -> &str {
        "neo_keyer"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "neo_keyer").into()
    }
}

impl So2rDrv for NeoKeyerDrv {
    fn open(&self, _settings: &Section, ports: &PortHub) -> Result<Box<dyn So2r>, String> {
        Ok(Box::new(Neo::on_keyer(ports.slot(KEYER_PORT))))
    }

    fn explain(&self, error: &str, language: &str) -> String {
        match error {
            haml::so2r::neo::NO_KEYER => texts().get(language, "no_keyer").into(),
            e => e.into(),
        }
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.driver::<dyn zlog_so2r::So2rDrv>(10, || Box::<NeoDrv>::default());
    b.driver::<dyn zlog_so2r::So2rDrv>(11, || Box::new(NeoKeyerDrv));
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_serial::Loop;
    use zlog_ui::tx::{Signal, TxCommand, TxHub};

    fn sim_port() -> Section {
        let mut s = Section::new();
        s.insert("port".into(), SIMULATOR.into());
        s
    }

    #[test]
    fn on_the_keyer_port_the_bytes_reach_the_keyers_wire() {
        let hub = PortHub::new();
        let mut b = NeoKeyerDrv.open(&Section::new(), &hub).unwrap();
        b.select_rig(2);
        b.blend(true, 50);
        let e = b.error().unwrap();
        assert_eq!(
            NeoKeyerDrv.explain(&e, "ja"),
            "WinKeyer のポートが開いていません。開くと SO2R Neo に伝えます"
        );
        assert_ne!(NeoKeyerDrv.explain(&e, "en"), e, "in zLog's words");
        b.poll();
        for switch in [0x95, 0x96] {
            let (port, w) = Loop::pair();
            hub.publish(KEYER_PORT, Box::new(port));
            b.poll();
            assert_eq!(w.read_written(), [switch, 0x87, 0xC7, 0xCF]);
            b.poll();
            assert!(w.read_written().is_empty());
            b.rx(Rx::Both);
            assert_eq!(w.read_written(), [0x96]);
            assert!(b.error().is_none());
            hub.withdraw(KEYER_PORT);
        }
    }

    #[test]
    fn drivers_keep_the_old_ids() {
        let neo = NeoDrv::default();
        assert_eq!((neo.id(), NeoKeyerDrv.id()), ("neo", "neo_keyer"));
        assert_eq!(neo.settings("en")[0].key, "port");
        assert!(NeoKeyerDrv.settings("en").is_empty());
        assert!(neo.open(&Section::new(), &PortHub::new()).is_err());
    }

    #[test]
    fn the_simulator_port_shows_what_the_box_was_told() {
        let neo = NeoDrv::default();
        let SettingKind::Port(names) = &neo.settings("en")[0].kind else {
            panic!("port field");
        };
        assert_eq!(names[0], SIMULATOR);
        assert!(neo.status("en").is_none());
        let mut b = neo.open(&sim_port(), &PortHub::new()).unwrap();
        assert_eq!(
            neo.status("en").unwrap(),
            "Simulator: TX RIG1  RX RIG1  AF blend OFF"
        );
        b.select_rig(2);
        b.rx(Rx::Both);
        b.blend(true, 30);
        assert_eq!(
            neo.status("en").unwrap(),
            "Simulator: TX RIG2  RX Both  AF blend 30%"
        );
        assert_eq!(
            neo.status("ja").unwrap(),
            "シミュレーター: TX RIG2  RX Both  AF ブレンド 30%"
        );
        assert!(b.error().is_none() && b.caps().auto_rx);
    }

    #[test]
    fn on_the_keyer_port_the_winkeyer_simulator_gets_the_bytes() {
        use zlog_keyer::KeyerDrv;
        let wk = zlog_keyer_winkey::WinKeyerDrv::default();
        let hub = PortHub::new();
        let keyer = KeyerDrv::open(&wk, &sim_port(), &hub).unwrap();
        let sim = wk.sim.lock().unwrap().clone().unwrap();
        let mut b = NeoKeyerDrv.open(&Section::new(), &hub).unwrap();
        b.select_rig(2);
        b.blend(true, 50);
        assert!(b.error().is_none());
        assert_eq!(sim.state().neo, [0x94, 0x95, 0x87, 0xC7, 0xCF]);
        assert!(sim.state().open && !sim.state().busy());
        drop(keyer);
    }

    #[test]
    fn cw_follows_the_tx_rig_through_the_winkeyer() {
        use zlog_keyer::KeyerDrv;
        use zlog_ui::Panel;
        let wk = zlog_keyer_winkey::WinKeyerDrv::default();
        let ports = PortHub::new();
        let tx = TxHub::default();
        tx.set_keyer(Some(KeyerDrv::open(&wk, &sim_port(), &ports).unwrap()));
        let sim = wk.sim.lock().unwrap().clone().unwrap();
        let mut p = zlog_so2r::So2rPanel::new(Vec::new(), ports.clone()).on(tx.clone());
        p.attach(NeoKeyerDrv.open(&Section::new(), &ports).unwrap());
        p.run_command("toggle_tx");
        let cw = |radio| TxCommand::Send {
            id: zlog_ui::tx::ticket(),
            radio,
            signal: Signal::Cw("TEST".into()),
        };
        tx.send(cw(tx.on_air().unwrap()));
        assert_eq!(sim.state().neo, [0x94], "typed CW goes to rig 2");
        tx.send(TxCommand::Stop);
        tx.send(cw(1));
        assert_eq!(
            sim.state().neo,
            [0x94, 0x90],
            "TX on rig 1 before the message"
        );
        assert!(sim.state().sent.starts_with('T') || !sim.state().buffer.is_empty());
        assert_eq!(p.tx(), 2);
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
