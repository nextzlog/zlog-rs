pub mod driver;
pub mod engine;
mod panel;
pub mod remote;

pub use crate::panel::{Ft8Options, Ft8Panel, SECTION};

zlog_core::texts!();

pub fn register(b: &mut zlog_ui::Builder) {
    for (n, proto) in driver::Proto::ALL.into_iter().enumerate() {
        b.driver::<dyn zlog_ui::device::ModemDrv>(30 + n as i32, move || {
            Box::new(driver::FtModemDrv(proto))
        });
    }
    b.page(320, |ctx| {
        Ft8Options::new(zlog_rtty::with_remote(ctx.new_drivers().take()))
    });
    b.panel(445, |ctx| {
        Ft8Panel::new(
            zlog_rtty::with_remote(ctx.new_drivers().take()),
            ctx.ports.clone(),
            zlog_ui::tx::hub(),
        )
        .hosting(remote::offer(), zlog_ui::tx::hub())
    });
}
