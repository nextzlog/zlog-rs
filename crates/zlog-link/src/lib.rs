//! Z-Link: a Z-Server client wire-compatible with zLog for Windows, so that this
//! zLog joins a multi-operator network with Windows stations.
//!
//! - [`wire`]: the text of a QSO on the wire (`TQSO.QSOinText`), Shift_JIS.
//! - [`protocol`]: the `#ZLOG#` commands (`TZLinkForm.ProcessCommand` and the senders).
//! - [`session`]: the TCP connection, shared by the panel and the link.
//! - [`link`]: applies QSOs from the server to the log and sends the local ones.
//! - [`panel`]: the window with connection settings and chat.
//! - [`freq`]: the stations' running frequencies, and their window.
//!
//! The panel lives for the whole program, while links are recreated whenever a log
//! is opened, so both reach the one [`Session`] through [`shared`].
//!
//! Not supported: SSL with login (zLog's `UseSecure`), file transfer
//! (`GETFILE`/`PUTFILE`), cluster and band scope relays (`SPOT`, `BSDATA`,
//! `SENDCLUSTER`, ...), wanted lists and the scratch sheet; such lines are shown in
//! the console only.

use std::sync::{Arc, Mutex, OnceLock};

use zlog_core::{Extension, Link};

pub mod freq;
pub mod link;
pub mod panel;
pub mod protocol;
pub mod session;
pub mod wire;

pub use freq::FreqPanel;
pub use link::LogLink;
pub use panel::ZLinkPanel;
pub use session::Session;

zlog_core::texts!();

/// The process-wide session.
pub fn shared() -> Arc<Mutex<Session>> {
    static SESSION: OnceLock<Arc<Mutex<Session>>> = OnceLock::new();
    SESSION.get_or_init(Default::default).clone()
}

/// The package: Z-Link, one link on the shared session.
pub struct ZLink;

impl Extension for ZLink {
    fn name(&self) -> &str {
        "Z-Link"
    }

    fn links(&mut self) -> Vec<Box<dyn Link>> {
        vec![Box::new(LogLink::new(shared()))]
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
