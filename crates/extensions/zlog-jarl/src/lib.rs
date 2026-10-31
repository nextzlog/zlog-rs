pub mod form;
pub mod member;
pub mod upload;

use std::collections::BTreeMap;

use egui::{RichText, Ui};
use zlog_net::{Exchange, Request};
use zlog_ui::{Command, EntryAction, MenuGroup, Panel, View};

use member::{Lookup, Member};

zlog_core::texts!();

pub type Transport = Box<dyn FnMut(Request) -> Exchange>;

pub struct MemberCheck {
    calls: Vec<String>,
    lookup: Option<Lookup>,
    actions: Vec<EntryAction>,
    applied: bool,
    transport: Transport,
}

impl Default for MemberCheck {
    fn default() -> Self {
        MemberCheck::with_transport(Box::new(Exchange::send))
    }
}

impl MemberCheck {
    pub fn with_transport(transport: Transport) -> Self {
        MemberCheck {
            calls: Vec::new(),
            lookup: None,
            actions: Vec::new(),
            applied: false,
            transport,
        }
    }

    fn answers(&self) -> Option<&BTreeMap<String, Member>> {
        self.lookup
            .as_ref()
            .filter(|l| !l.running() && l.error.is_none())
            .map(|l| &l.answers)
    }
}

impl Panel for MemberCheck {
    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::Dialog
    }

    fn id(&self) -> &str {
        "jarl_member"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "member.title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [480.0, 420.0]
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Search
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::LookUp, 0)
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        let mut c = Command::window(
            texts().get(language, "member.toolbar"),
            texts().get(language, "member.menu.tip"),
        )
        .icon(zlog_ui::icon::USERS);
        c.menu_label = Some(texts().get(language, "member.menu").into());
        vec![c]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn opened(&mut self, view: &View<'_>) {
        self.calls = member::callsigns(view.log);
        self.lookup = None;
        self.applied = false;
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = |k| texts().get(view.language, k);
        let lang = view.language;
        if let Some(l) = self.lookup.as_mut() {
            l.poll(&mut self.transport);
            if l.running() {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        ui.add(egui::Label::new(t("member.about")).wrap())
            .on_hover_text(format!("{} {}", t("member.url"), member::SEARCH_URL));
        ui.separator();
        match (&self.lookup, self.answers()) {
            (None, _) => {
                ui.label(texts().format(lang, "member.calls", &[&self.calls.len()]));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for c in &self.calls {
                            ui.monospace(c);
                        }
                    });
                });
            }
            (Some(l), None) => {
                if let Some(e) = &l.error {
                    ui.colored_label(
                        zlog_ui::palette::error(ui),
                        texts().format(lang, "member.failed", &[e]),
                    );
                } else {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(texts().format(lang, "member.progress", &[&l.done, &l.batches()]));
                    });
                }
            }
            (Some(_), Some(answers)) => {
                let changes = member::changes(view.log, answers);
                if self.applied {
                    ui.label(t("member.applied"));
                } else {
                    ui.label(texts().format(lang, "member.changes", &[&changes.len()]));
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("jarl_member").striped(true).show(ui, |ui| {
                        for k in [
                            "member.head.call",
                            "member.head.member",
                            "member.head.transfer",
                        ] {
                            ui.label(RichText::new(t(k)).strong());
                        }
                        ui.end_row();
                        let mark = |b: bool| if b { t("member.yes") } else { t("member.no") };
                        for (call, m) in answers {
                            ui.monospace(call);
                            ui.label(mark(m.member));
                            ui.label(mark(m.transfer));
                            ui.end_row();
                        }
                    });
                });
            }
        }
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn capability_mut(&mut self, want: zlog_ui::WantMut) -> Option<zlog_ui::CapabilityMut<'_>> {
        match want {
            zlog_ui::WantMut::Buttons => Some(zlog_ui::CapabilityMut::Buttons(self)),
            _ => None,
        }
    }
}

impl zlog_ui::ButtonRow for MemberCheck {
    fn buttons(&mut self, ui: &mut Ui, view: &View<'_>) -> bool {
        let t = |k| texts().get(view.language, k);
        zlog_ui::form::buttons(ui, |ui| {
            match self.answers() {
                None => {
                    let idle = self.lookup.as_ref().is_none_or(|l| !l.running());
                    let ask = ui.add_enabled(
                        idle && !self.calls.is_empty(),
                        egui::Button::new(t("member.ask")),
                    );
                    if ask.clicked() {
                        self.lookup = Some(Lookup::start(&self.calls, &mut self.transport));
                    }
                }
                Some(answers) => {
                    let changes = member::changes(view.log, answers);
                    let apply = ui.add_enabled(
                        !self.applied && !changes.is_empty(),
                        egui::Button::new(t("member.apply")),
                    );
                    if apply.clicked() {
                        self.actions.push(EntryAction::update_qsos(changes));
                        self.applied = true;
                    }
                }
            }
            ui.button(t("close")).clicked()
        })
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(560, |_| MemberCheck::default());
    b.driver::<dyn zlog_summary::WebSubmit>(10, || Box::new(upload::JarlWeb));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn nothing_is_sent_until_asked() {
        let (t, sent) = member::tests::fake(&[member::tests::PAGE, member::tests::ANSWER]);
        let mut m = MemberCheck::with_transport(t);
        m.calls = vec!["JA1ZLO".into(), "JA1YAA".into()];
        assert!(m.answers().is_none());
        assert!(sent.borrow().is_empty());
        m.lookup = Some(Lookup::start(&m.calls, &mut m.transport));
        while m.lookup.as_ref().unwrap().running() {
            m.lookup.as_mut().unwrap().poll(&mut m.transport);
        }
        let a = m.answers().unwrap();
        assert!(a["JA1ZLO"].transfer && !a["JA1YAA"].transfer);
        assert_eq!(sent.borrow().len(), 2);
    }
}
