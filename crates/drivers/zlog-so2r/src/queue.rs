use egui::Ui;
use zlog_ui::{Command, Panel, View};

use crate::texts;

pub struct QueuePanel {
    queue: zlog_ui::queue::Shared,
}

impl Default for QueuePanel {
    fn default() -> Self {
        QueuePanel {
            queue: zlog_ui::queue::shared(),
        }
    }
}

impl Panel for QueuePanel {
    fn id(&self) -> &str {
        "so2r-queue"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "queue.title").into()
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Device
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Rig, 11)
    }

    fn default_size(&self) -> [f32; 2] {
        [360.0, 160.0]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        let t = |k| texts().get(language, k);
        vec![Command::window(t("queue.toolbar"), t("queue.toolbar.tip"))
            .icon(zlog_ui::icon::LIST_MAGNIFYING_GLASS)]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        let (so2r, lines) = {
            let q = self.queue.lock().unwrap();
            (q.so2r, q.lines())
        };
        if !so2r {
            zlog_ui::layout::status(ui, t("queue.off"), None);
        } else if lines.is_empty() {
            zlog_ui::layout::status(ui, t("queue.empty"), None);
        }
        zlog_ui::layout::rest(ui, "so2r_queue", |ui| {
            for line in lines {
                ui.monospace(line);
            }
        });
        if !self.queue.lock().unwrap().is_empty() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}
