use crate::app::panels::{panel_view, RigNow};
use crate::session::{Session, Setup};

pub struct PanelHost {
    pub panel: Box<dyn zlog_ui::Panel>,
    pub session: Session,
    pub spots: Vec<zlog_ui::Spot>,
    pub body: egui::Rect,
    pub content: egui::Rect,
}

impl PanelHost {
    pub fn open(id: &str, setup: Setup) -> Result<Self, String> {
        let mut panel = crate::registry::panels()
            .into_iter()
            .find(|p| p.id() == id)
            .ok_or(format!("no panel {id}"))?;
        panel.load(&zlog_ui::Section::new());
        Ok(PanelHost {
            panel,
            session: Session::open(setup, crate::registry::registry())?,
            spots: Vec::new(),
            body: egui::Rect::NOTHING,
            content: egui::Rect::NOTHING,
        })
    }

    pub fn sample(&mut self) {
        use chrono::{Datelike, Timelike};
        let now = chrono::Utc::now() - chrono::Duration::minutes(50);
        for (i, (call, band, mode, nr)) in [
            ("JA1YWX", zlog_core::Band::B7, zlog_core::Mode::Cw, "10M"),
            ("JA1ZLO", zlog_core::Band::B14, zlog_core::Mode::Ssb, "100H"),
            ("JA1YWX", zlog_core::Band::B21, zlog_core::Mode::Cw, "10M"),
        ]
        .into_iter()
        .enumerate()
        {
            let t = now + chrono::Duration::minutes(10 * i as i64);
            let time = zlog_core::DelphiTime::from_ymd_hms(
                t.year(),
                t.month(),
                t.day(),
                t.hour(),
                t.minute(),
                t.second(),
            );
            let qso = zlog_core::Qso {
                callsign: call.into(),
                band,
                mode,
                nr_rcvd: nr.into(),
                time,
                ..Default::default()
            };
            self.session.execute(zlog_core::Command::Insert(qso));
        }
        self.session.entry.callsign = "JA1YWX".into();
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        {
            let view = panel_view(
                None,
                None,
                None,
                &self.session,
                &self.spots,
                RigNow::default(),
            );
            let panel = &mut self.panel;
            let out = egui::ScrollArea::both()
                .id_salt("layout_body")
                .auto_shrink(false)
                .show(ui, |ui| {
                    zlog_ui::layout::pane(ui, |ui| {
                        let inner = ui.max_rect();
                        panel.ui(ui, &view);
                        if let Some(b) = zlog_ui::buttons_of(panel.as_mut()) {
                            ui.separator();
                            b.buttons(ui, &view);
                        }
                        (inner, ui.min_rect())
                    })
                });
            (self.body, self.content) = out.inner;
            panel.dialogs(ui.ctx(), &view);
        }
    }
}
