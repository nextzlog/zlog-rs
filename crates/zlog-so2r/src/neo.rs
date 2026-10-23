//! SO2R Neo commands: single bytes on the WinKeyer port (`TdmZLogKeyer.So2rNeo*`,
//! UzLogKeyer.pas).

/// What the operator listens to (`rx` of `So2rNeoSwitchRig`: 0 left, 1 right, 2 both).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rx {
    Rig1,
    Rig2,
    Both,
}

impl Rx {
    /// The receiver of rig 1 or 2.
    pub fn of(rig: u8) -> Rx {
        if rig == 2 {
            Rx::Rig2
        } else {
            Rx::Rig1
        }
    }
}

/// Transmit on rig `tx` (1 or 2) and listen to `rx` (`So2rNeoSwitchRig`: $90 to $92
/// for TX on rig 1, $94 to $96 on rig 2).
pub fn switch_rig(tx: u8, rx: Rx) -> u8 {
    let tx = if tx == 2 { 0x94 } else { 0x90 };
    tx + rx as u8
}

/// Blend both receivers or not (`So2rNeoSetAudioBlendMode`).
pub fn blend_mode(on: bool) -> u8 {
    if on {
        0x87
    } else {
        0x86
    }
}

/// The blend ratio in percent, as two nibbles of 255 × ratio / 100
/// (`So2rNeoSetAudioBlendRatio`).
pub fn blend_ratio(percent: u8) -> [u8; 2] {
    let r = (255 * u32::from(percent.min(100)) / 100) as u8;
    [0xC0 | (r >> 4), 0xC0 | (r & 0x0F)]
}

/// While sending on rig `tx`, listen to the other rig (`So2rNeoReverseRx`).
pub fn reverse_rx(tx: u8) -> Rx {
    if tx == 2 {
        Rx::Rig1
    } else {
        Rx::Rig2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_as_zlog_sends_them() {
        assert_eq!(switch_rig(1, Rx::Rig1), 0x90);
        assert_eq!(switch_rig(1, Rx::Rig2), 0x91);
        assert_eq!(switch_rig(1, Rx::Both), 0x92);
        assert_eq!(switch_rig(2, Rx::Rig1), 0x94);
        assert_eq!(switch_rig(2, Rx::Rig2), 0x95);
        assert_eq!(switch_rig(2, Rx::Both), 0x96);
        assert_eq!((blend_mode(true), blend_mode(false)), (0x87, 0x86));
        // 50 % is 127 ($7F), 100 % is 255.
        assert_eq!(blend_ratio(50), [0xC7, 0xCF]);
        assert_eq!(blend_ratio(100), [0xCF, 0xCF]);
        assert_eq!(blend_ratio(0), [0xC0, 0xC0]);
        assert_eq!((reverse_rx(1), reverse_rx(2)), (Rx::Rig2, Rx::Rig1));
    }
}
