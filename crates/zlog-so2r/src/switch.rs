//! The SO2R box: which rig transmits and which one the operator hears, sent as zLog
//! sends it (`TdmZLogKeyer.SetTxRigFlag`, `SetRxRigFlag`, `So2rNeoSwitchRig`).
//!
//! - A rig selected (`TMainForm.SwitchRig`): the transmitter, then the receiver
//!   follow it. SO2R Neo gets (new TX, old RX), then (new TX, new RX); OTRSP gets
//!   `TX`n and `RX`n, each only when it differs from the one sent last
//!   (`FPrevTxRigSet`, `FPrevRxRigSet`).
//! - Listening to both rigs and the AF blend are SO2R Neo commands; zLog sends
//!   nothing to OTRSP for them (`actionSo2rNeoSelRxExecute`, `OnZLogSo2rBlend`).

use zlog_rig::Transport;

use crate::neo::{self, Rx};
use crate::otrsp;

/// The SO2R box (zLog `TSo2rType`; COM-port, parallel and MK2R boxes are not
/// implemented).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Device {
    #[default]
    None,
    Neo,
    /// SO2R Neo on the WinKeyer port of the keyer, as zLog uses it: its commands
    /// go through the keyer ([`zlog_ui::EntryAction::KeyerPort`]).
    NeoKeyer,
    Otrsp,
}

impl Device {
    pub const ALL: [Device; 4] = [Device::None, Device::Neo, Device::NeoKeyer, Device::Otrsp];

    /// Key of its name in the lang files, also written to the settings.
    pub fn key(self) -> &'static str {
        match self {
            Device::None => "none",
            Device::Neo => "neo",
            Device::NeoKeyer => "neo_keyer",
            Device::Otrsp => "otrsp",
        }
    }

    pub fn from_key(s: &str) -> Device {
        Device::ALL
            .into_iter()
            .find(|d| d.key() == s)
            .unwrap_or_default()
    }
}

/// A connected box.
pub struct Switch {
    pub device: Device,
    transport: Box<dyn Transport>,
    /// The rig transmitting, 1 or 2.
    pub tx: u8,
    pub rx: Rx,
    /// What OTRSP was last told.
    sent_tx: Option<u8>,
    sent_rx: Option<u8>,
    /// The last write that failed.
    pub error: Option<String>,
}

impl Switch {
    /// A box on `transport`; SO2R Neo on the keyer's port talks as SO2R Neo.
    pub fn new(device: Device, transport: Box<dyn Transport>) -> Self {
        let device = match device {
            Device::NeoKeyer => Device::Neo,
            d => d,
        };
        Switch {
            device,
            transport,
            tx: 1,
            rx: Rx::Rig1,
            sent_tx: None,
            sent_rx: None,
            error: None,
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        if let Err(e) = self.transport.write(bytes) {
            self.error = Some(e.to_string());
        }
    }

    /// Transmit on rig 1 or 2 and listen to it.
    pub fn select_rig(&mut self, rig: u8) {
        self.tx = rig;
        match self.device {
            Device::Neo => self.send(&[neo::switch_rig(rig, self.rx)]),
            Device::Otrsp if self.sent_tx != Some(rig) => {
                self.send(&otrsp::tx(rig));
                self.sent_tx = Some(rig);
            }
            _ => {}
        }
        self.rx = Rx::of(rig);
        match self.device {
            Device::Neo => self.send(&[neo::switch_rig(rig, self.rx)]),
            Device::Otrsp if self.sent_rx != Some(rig) => {
                self.send(&otrsp::rx(rig));
                self.sent_rx = Some(rig);
            }
            _ => {}
        }
    }

    /// Listen to `rx`, still transmitting on the same rig. For OTRSP only one rig
    /// may be heard; zLog sends it nothing for both.
    pub fn listen(&mut self, rx: Rx) {
        self.rx = rx;
        match (self.device, rx) {
            (Device::Neo, _) => self.send(&[neo::switch_rig(self.tx, rx)]),
            (Device::Otrsp, Rx::Rig1 | Rx::Rig2) => {
                let rig = if rx == Rx::Rig2 { 2 } else { 1 };
                self.send(&otrsp::rx(rig));
                self.sent_rx = Some(rig);
            }
            _ => {}
        }
    }

    /// Blend the receivers (with `ratio` in percent) or not.
    pub fn blend(&mut self, on: bool, ratio: u8) {
        if self.device == Device::Neo {
            self.send(&[neo::blend_mode(on)]);
            if on {
                self.send(&neo::blend_ratio(ratio));
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Records what is written to it.
    #[derive(Clone, Default)]
    pub(crate) struct Wire(pub(crate) Arc<Mutex<Vec<u8>>>);

    impl Transport for Wire {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(())
        }
        fn read(&mut self, _buf: &mut Vec<u8>) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Wire {
        pub(crate) fn take(&self) -> Vec<u8> {
            std::mem::take(&mut self.0.lock().unwrap())
        }
    }

    fn switch(device: Device) -> (Wire, Switch) {
        let w = Wire::default();
        (w.clone(), Switch::new(device, Box::new(w)))
    }

    #[test]
    fn neo_switches_tx_then_rx() {
        let (w, mut s) = switch(Device::Neo);
        s.select_rig(2);
        // TX on rig 2 with the old RX (rig 1), then RX on rig 2.
        assert_eq!(w.take(), [0x94, 0x95]);
        s.listen(Rx::Both);
        assert_eq!(w.take(), [0x96]);
        s.select_rig(1);
        assert_eq!(w.take(), [0x92, 0x90]);
        s.blend(true, 50);
        s.blend(false, 50);
        assert_eq!(w.take(), [0x87, 0xC7, 0xCF, 0x86]);
    }

    #[test]
    fn otrsp_sends_only_changes() {
        let (w, mut s) = switch(Device::Otrsp);
        s.select_rig(1);
        assert_eq!(w.take(), b"TX1\rRX1\r");
        s.select_rig(1);
        assert!(w.take().is_empty());
        s.select_rig(2);
        assert_eq!(w.take(), b"TX2\rRX2\r");
        s.listen(Rx::Rig1);
        s.listen(Rx::Both);
        s.blend(true, 50);
        assert_eq!(w.take(), b"RX1\r");
    }
}
