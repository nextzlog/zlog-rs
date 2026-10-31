use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::Ui;
use zlog_tls::Token;
use zlog_ui::{EntryAction, Panel, Section, StatusItem, View};

use crate::hub::Note;
use crate::net::{Pairing, Shared};
use crate::protocol::Refused;
use crate::session::Session;
use crate::texts;

pub struct LinkNet {
    net: Arc<Shared>,
    session: Arc<Mutex<Session>>,
    actions: Vec<EntryAction>,
}

impl Default for LinkNet {
    fn default() -> Self {
        LinkNet::new(crate::net::shared(), crate::shared())
    }
}

fn note_key(n: &Note) -> &'static str {
    match n {
        Note::Refused(Refused::Long) => "note.long",
        Note::Refused(Refused::File) => "note.file",
        Note::Refused(_) => "note.malformed",
        Note::Truncated => "note.truncated",
        Note::Held => "note.held",
        Note::Duplicate(_) => "note.duplicate",
        Note::TxClash { .. } => "note.tx_clash",
        Note::MergeTimeout => "note.merge_timeout",
        Note::MergeBusy => "note.merge_busy",
        Note::Full => "note.full",
        Note::Approved => "note.approved",
        Note::Denied => "note.denied",
    }
}

pub fn note_text(who: &str, n: &Note, lang: &str) -> (String, bool) {
    let t = texts();
    let text = match n {
        Note::TxClash { tx, other } => t.format(lang, "note.tx_clash", &[&who, tx, other]),
        Note::Duplicate(id) => t.format(lang, "note.duplicate", &[&who, id]),
        n => t.format(lang, note_key(n), &[&who]),
    };
    (text, !matches!(n, Note::Approved | Note::Duplicate(_)))
}

fn warn_tls(k: &zlog_tls::AuditKind) -> bool {
    use zlog_tls::AuditKind::*;
    matches!(
        k,
        OutOfRange
            | Full
            | Denied
            | Unknown
            | AuthFailed { .. }
            | LockedOut { .. }
            | Locked
            | LineTooLong
            | RateLimited
    )
}

impl LinkNet {
    pub fn new(net: Arc<Shared>, session: Arc<Mutex<Session>>) -> Self {
        LinkNet {
            net,
            session,
            actions: Vec::new(),
        }
    }

    pub fn drain(&mut self, lang: &str) {
        let t = texts();
        let mut lines = Vec::new();
        {
            let mut n = self.net.lock();
            for (who, note) in std::mem::take(&mut n.notes) {
                let (text, warn) = note_text(&who, &note, lang);
                lines.push((text, warn, !matches!(note, Note::Duplicate(_))));
            }
            for a in std::mem::take(&mut n.audit) {
                let who = match a.name.is_empty() {
                    true => a.addr.to_string(),
                    false => format!("{} ({})", a.name, a.addr),
                };
                let kind = format!("{:?}", a.kind);
                let key = format!("audit.{}", kind.split([' ', '{']).next().unwrap_or(""));
                lines.push((t.format(lang, &key, &[&who]), warn_tls(&a.kind), true));
            }
            for (text, _, _) in &lines {
                n.hub.console.push_back(text.clone());
            }
            let over = n.hub.console.len().saturating_sub(crate::hub::SLOTS * 32);
            n.hub.console.drain(..over);
        }
        for (text, warning, shown) in lines {
            if shown {
                self.actions
                    .push(EntryAction::Say(StatusItem::new(text).warning(warning)));
            }
        }
    }

    pub fn status_items(&self, lang: &str) -> Vec<StatusItem> {
        let t = texts();
        let mut items = Vec::new();
        let (connected, secure) = {
            let s = self.session.lock().unwrap();
            (s.connected(), s.secure)
        };
        if connected && !secure {
            items.push(
                StatusItem::new(t.get(lang, "status.plaintext"))
                    .tooltip(t.get(lang, "status.plaintext.tip"))
                    .warning(true),
            );
        }
        let n = self.net.lock();
        if let Some(l) = &n.listener {
            let count = n.hub.approved();
            let key = match l.secure {
                true => "status.serving",
                false => "status.serving.plain",
            };
            items.push(
                StatusItem::new(t.format(lang, key, &[&count]))
                    .tooltip(l.addr().to_string())
                    .warning(!l.secure),
            );
            if !n.asks.is_empty() || !l.requests().is_empty() {
                items.push(StatusItem::new(t.get(lang, "status.approval")).warning(true));
            }
        }
        items
    }

    pub fn dialogs_ui(&mut self, ctx: &egui::Context, lang: &str) {
        let (code, repair, ask, request) = {
            let n = self.net.lock();
            (
                n.code.clone(),
                n.repair.clone(),
                n.asks.first().cloned(),
                n.listener
                    .as_ref()
                    .and_then(|l| l.requests().into_iter().next()),
            )
        };
        if let Some(code) = code {
            zlog_ui::pairing::code_window(ctx, &code, lang);
        }
        if let Some(addr) = repair {
            if let Some(yes) = zlog_ui::pairing::repair_window(ctx, &addr, lang) {
                self.net.lock().repair = None;
                if yes {
                    let net = self.net.clone();
                    let _ = self
                        .session
                        .lock()
                        .unwrap()
                        .connect_secure(net, Pairing::Again);
                }
            }
        }
        if let Some(r) = request {
            let addr = r.addr.to_string();
            if let Some(a) = zlog_ui::pairing::approve_window(
                ctx,
                &r.name,
                &addr,
                r.code.as_deref(),
                false,
                lang,
            ) {
                if let Some(l) = &self.net.lock().listener {
                    l.decide(r.id, a);
                }
            }
        } else if let Some(a) = ask {
            let addr = a.addr.to_string();
            if let Some(yes) =
                zlog_ui::pairing::approve_window(ctx, &a.name, &addr, None, true, lang)
            {
                let mut n = self.net.lock();
                n.asks.retain(|x| x.id != a.id);
                n.decisions.push((a.id, yes));
                drop(n);
                zlog_core::wake::wake();
            }
        }
    }
}

impl Panel for LinkNet {
    fn id(&self) -> &str {
        "link-net"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "server.title").into()
    }

    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::None
    }

    fn section_kind(&self) -> zlog_ui::SectionKind {
        zlog_ui::SectionKind::Secret
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Network
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Link, 1)
    }

    fn status(&self, view: &View<'_>) -> Vec<StatusItem> {
        self.status_items(view.language)
    }

    fn tick_after(&self) -> Option<Duration> {
        let n = self.net.lock();
        (n.listener.is_some() || n.code.is_some()).then_some(Duration::from_millis(500))
    }

    fn tick(&mut self, view: &View<'_>) {
        self.drain(view.language);
    }

    fn load(&mut self, section: &Section) {
        self.net.lock().load(section);
    }

    fn save(&self, section: &mut Section) {
        self.net.lock().save(section);
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn dialogs(&mut self, ctx: &egui::Context, view: &View<'_>) {
        self.dialogs_ui(ctx, view.language);
    }
}

pub fn secrets_ui(ui: &mut Ui, net: &Shared, shown: &mut Option<Token>, lang: &str) {
    let t = |k| texts().get(lang, k);
    ui.horizontal(|ui| {
        let label = ui.label(t("client_token"));
        let mut n = net.lock();
        ui.add(egui::TextEdit::singleline(&mut n.client_token).password(true))
            .labelled_by(label.id);
    });
    let set = net.lock().token.is_some();
    ui.label(t(if set { "token.set" } else { "token.none" }));
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(t("token.new"))
            .on_hover_text(t("token.new.tip"))
            .clicked()
        {
            let token = Token::generate();
            net.lock().token = Some(token.hash());
            *shown = Some(token);
        }
        if set
            && ui
                .button(t("token.clear"))
                .on_hover_text(t("token.clear.tip"))
                .clicked()
        {
            net.lock().token = None;
            *shown = None;
        }
    });
    if let Some(token) = shown {
        ui.horizontal_wrapped(|ui| {
            ui.monospace(token.as_str());
            if ui
                .button(t("token.copy"))
                .on_hover_text(t("token.copy.tip"))
                .clicked()
            {
                ui.ctx().copy_text(token.to_string());
            }
        });
        ui.weak(t("token.once"));
    }
    ui.separator();
    ui.label(t("clients"));
    let clients = net.lock().clients.clone();
    zlog_ui::pairing::peer_list(ui, &clients, lang);
    ui.separator();
    ui.label(t("servers"));
    let servers = net.lock().servers.clone();
    zlog_ui::pairing::peer_list(ui, &servers, lang);
}
