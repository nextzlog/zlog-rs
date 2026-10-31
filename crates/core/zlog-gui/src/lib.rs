mod app;
mod commands;
#[cfg(any(test, feature = "testing"))]
mod ellipsis;
mod entry;
mod fonts;
mod i18n;
#[cfg(feature = "testing")]
mod layout;
mod main_window;
mod menus;
mod options;
mod registry;
mod row;
mod session;
mod settings;
mod wizard;

#[cfg(feature = "testing")]
pub mod testing {
    pub mod app {
        pub use crate::app::*;
        pub mod files {
            pub use crate::app::files::*;
        }
        pub mod keys {
            pub use crate::app::keys::*;
        }
        pub mod panels {
            pub use crate::app::panels::*;
        }
    }
    pub mod commands {
        pub use crate::commands::*;
    }
    pub mod ellipsis {
        pub use crate::ellipsis::*;
    }
    pub mod entry {
        pub use crate::entry::*;
    }
    pub mod i18n {
        pub use crate::i18n::*;
    }
    pub mod main_window {
        pub use crate::main_window::View;
    }
    pub mod layout {
        pub use crate::layout::*;
    }
    pub mod menus {
        pub use crate::menus::group_key;
    }
    pub mod options {
        pub use crate::options::Options;
    }
    pub mod registry {
        pub use crate::registry::{
            drivers, entry_style, install, migrations, panels, registry, settings_files,
        };
    }
    pub mod row {
        pub use crate::row::Row;
    }
    pub mod session {
        pub use crate::session::*;
    }
    pub mod settings {
        pub use crate::settings::*;
    }
}

pub use eframe::Result;
use zlog_ui::dialog;

pub fn run(register: registry::Register) -> Result {
    registry::install(register);
    if let Some(run) = std::env::args()
        .nth(1)
        .and_then(|a| registry::subcommand(&a))
    {
        run();
        return Ok(());
    }
    i18n::set_from_environment();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([480.0, 320.0])
            .with_title(zlog_ui::product::NAME),
        ..Default::default()
    };
    eframe::run_native(
        "zLog",
        options,
        Box::new(|cc| {
            fonts::install(&cc.egui_ctx);
            let ctx = cc.egui_ctx.clone();
            zlog_core::wake::set(move || ctx.request_repaint());
            // zLog uses the light Windows look.
            cc.egui_ctx.set_theme(egui::Theme::Light);
            Ok(Box::new(app::ZLogApp::new()))
        }),
    )
}
