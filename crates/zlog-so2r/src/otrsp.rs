//! OTRSP commands: ASCII lines ending in CR on a port of their own, 9600 bps 8N1
//! (`SetTxRigFlag_otrsp`, `SetRxRigFlag_otrsp`, `SetOtrspPortParam`).

/// The speed of the OTRSP port.
pub const BAUD: u32 = 9600;

/// Transmit on rig 1 or 2 (`TX1`, `TX2`).
pub fn tx(rig: u8) -> Vec<u8> {
    format!("TX{rig}\r").into_bytes()
}

/// Listen to rig 1 or 2 (`RX1`, `RX2`).
pub fn rx(rig: u8) -> Vec<u8> {
    format!("RX{rig}\r").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_as_zlog_sends_them() {
        assert_eq!(tx(2), b"TX2\r");
        assert_eq!(rx(1), b"RX1\r");
    }
}
