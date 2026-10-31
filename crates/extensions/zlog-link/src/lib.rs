use std::sync::{Arc, Mutex, OnceLock};

use zlog_core::{Extension, Link};

pub mod freq;
pub mod hub;
pub mod link;
pub mod net;
pub mod panel;
pub mod protocol;
pub mod role;
pub mod secure;
pub mod serve;
pub mod session;
pub mod wire;

pub use freq::FreqPanel;
pub use link::LogLink;
pub use panel::ZLinkPanel;
pub use secure::LinkNet;
pub use serve::ServerLink;
pub use session::Session;

zlog_core::texts!();

pub fn shared() -> Arc<Mutex<Session>> {
    static SESSION: OnceLock<Arc<Mutex<Session>>> = OnceLock::new();
    SESSION.get_or_init(Default::default).clone()
}

pub struct ZLink;

impl Extension for ZLink {
    fn name(&self) -> &str {
        "Z-Link"
    }

    fn links(&mut self) -> Vec<Box<dyn Link>> {
        vec![
            Box::new(LogLink::new(shared())),
            Box::new(ServerLink::new(net::shared(), shared())),
        ]
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.extension(zlog_ui::builder::order::LINKS, || ZLink);
    b.page(500, |_| panel::LinkOptions::default());
    b.panels(460, |_| {
        vec![
            Box::new(ZLinkPanel::default()),
            Box::new(FreqPanel::default()),
            Box::new(LinkNet::default()),
        ]
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
