use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use zlog_core::{Command, Context, Link, LogEvent, Observer, Origin, Qso, QsoId};

use crate::protocol::{self, Message};
use crate::session::{Merge, OnConnect, Session};

pub struct LogLink {
    session: Arc<Mutex<Session>>,
    queued: Vec<Qso>,
}

impl LogLink {
    pub fn new(session: Arc<Mutex<Session>>) -> Self {
        LogLink {
            session,
            queued: Vec::new(),
        }
    }
}

struct Ids(HashSet<QsoId>);

impl Ids {
    fn of(cx: &Context<'_>) -> Self {
        Ids(cx.log().qsos.iter().map(QsoId::of).collect())
    }

    fn has(&self, q: &Qso) -> bool {
        self.0.contains(&QsoId::of(q))
    }
}

impl Observer for LogLink {
    fn name(&self) -> &str {
        "Z-Link"
    }

    fn on_event(&mut self, cx: &mut Context<'_>, event: &LogEvent) {
        // As zLog, every local edit is sent (also observers' reactions to it), never what
        // came from the network.
        if cx.caused_by_me() || cx.root_origin() != Origin::User {
            return;
        }
        let mut s = self.session.lock().unwrap();
        if !s.connected() {
            return;
        }
        match event {
            LogEvent::Inserted { index } => {
                let q = &cx.log().qsos[*index];
                // zLog sends BAND when the entry row changes band; when the panel is
                // closed nobody watches the entry row, so tell it here at the latest.
                if s.band_sent != Some(q.band) {
                    s.send_band(q.band);
                }
                s.write(&protocol::put_qso(q));
            }
            // zLog sends INSQSO from the edit dialog's insert action
            // (`TEditDialog`, `InsertQSO`); the book reports no insert-before by the
            // operator, so an inserted QSO is sent as PUTQSO above. LOCKQSO and
            // UNLOCKQSO go out from the panel ([`crate::ZLinkPanel`]), which sees the
            // edit dialog in its view.
            LogEvent::Updated { index, .. } => {
                s.write(&protocol::edit_qso_to(&cx.log().qsos[*index]));
            }
            // zLog sends EXDELQSO (`DeleteQsoEx`, followed by RENEW) when it deletes
            // QSOs a merge replaced; the book has no such path here, so only the
            // operator's deletes (DELQSO) are reachable.
            LogEvent::Deleted { qso, .. } => s.write(&protocol::del_qso(qso)),
            LogEvent::Opened { .. } | LogEvent::Sorted | LogEvent::Resynced | LogEvent::Closed => {}
        }
    }
}

impl Link for LogLink {
    fn poll(&mut self, cx: &mut Context<'_>) {
        let mut s = self.session.lock().unwrap();
        s.pump();
        for m in std::mem::take(&mut s.status) {
            cx.say(m);
        }
        if std::mem::take(&mut s.init_pending) {
            init(cx, &mut s);
        }
        if s.clearing {
            cx.resync();
            let ids: Vec<QsoId> = cx.log().qsos.iter().map(QsoId::of).collect();
            if ids.is_empty() {
                s.clearing = false;
                s.write(&protocol::send_log());
            } else {
                backup(cx, &mut s);
                for id in ids {
                    cx.send(Command::Delete(id));
                }
                return;
            }
        }
        if s.inbox.is_empty() {
            return;
        }
        let deletes = s
            .inbox
            .iter()
            .filter(|m| matches!(m, Message::DelQso(_)))
            .count();
        if deletes > crate::session::LARGE_DELETION {
            backup(cx, &mut s);
        }
        let mut ids = Ids::of(cx);
        let mut sent = false;
        while let Some(m) = s.inbox.pop_front() {
            macro_rules! send {
                ($c:expr) => {{
                    cx.send($c);
                    sent = true;
                }};
            }
            if matches!(
                m,
                Message::Renew
                    | Message::DelQso(_)
                    | Message::InsQso(_)
                    | Message::LockQso(_)
                    | Message::UnlockQso(_)
                    | Message::EditQsoTo(_)
            ) {
                for q in std::mem::take(&mut self.queued) {
                    if let Some(c) = insert_before(&mut ids, q) {
                        send!(c);
                    }
                }
            }
            match m {
                Message::BeginMergeOk => {
                    s.merge = Some(Merge::default());
                    s.write(&protocol::get_qso_ids());
                }
                Message::QsoIds(list) => {
                    if let Some(merge) = &mut s.merge {
                        for full in list {
                            if let std::collections::hash_map::Entry::Vacant(e) =
                                merge.ids.entry(full / 100)
                            {
                                e.insert(full);
                                merge.order.push(full / 100);
                            }
                        }
                    }
                }
                Message::EndQsoIds | Message::SendLog if sent => {
                    s.inbox.push_front(m);
                    break;
                }
                Message::EndQsoIds => {
                    cx.resync();
                    end_merge(cx, &mut s);
                }
                Message::SendLog => {
                    cx.resync();
                    for q in &cx.log().qsos {
                        s.write(&protocol::put_log(q));
                    }
                    s.write(&protocol::renew_after_log());
                }
                Message::PutQso(q) => {
                    if !q.callsign.is_empty() && !ids.has(&q) {
                        ids.0.insert(QsoId::of(&q));
                        send!(Command::Insert(q));
                    }
                }
                Message::PutLog(q) => {
                    if !q.callsign.is_empty() && !ids.has(&q) {
                        ids.0.insert(QsoId::of(&q));
                        send!(Command::Insert(q));
                    }
                }
                Message::InsQso(q) => {
                    if let Some(c) = insert_before(&mut ids, q) {
                        send!(c);
                    }
                }
                Message::InsQsoAt(q) => self.queued.push(q),
                Message::PutLogEx(q) => {
                    let id = QsoId::of(&q);
                    if ids.has(&q) {
                        send!(Command::Update(id, q));
                    } else {
                        ids.0.insert(id);
                        send!(Command::Insert(q));
                    }
                }
                Message::EditQsoTo(q) => {
                    if ids.has(&q) {
                        send!(Command::Update(QsoId::of(&q), q));
                    }
                }
                Message::DelQso(q) => {
                    if ids.has(&q) {
                        ids.0.remove(&QsoId::of(&q));
                        send!(Command::Delete(QsoId::of(&q)));
                    }
                }
                Message::LockQso(_) | Message::UnlockQso(_) => {}
                Message::Renew => send!(Command::SortByTime),
                Message::Freq(_)
                | Message::PutMessage { .. }
                | Message::BeginMergeNg
                | Message::Other(_) => {}
            }
        }
    }
}

fn backup(cx: &mut Context<'_>, s: &mut Session) {
    let Some(path) = s.backup.clone() else {
        return;
    };
    let dir = path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("zlink-backup");
    let stem = path
        .file_stem()
        .map_or("log".into(), |s| s.to_string_lossy().to_string());
    let time = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let file = dir.join(format!("{stem}-{time}.ZLOX"));
    let saved = std::fs::create_dir_all(&dir)
        .map_err(|e| e.to_string())
        .and_then(|_| {
            cx.log()
                .save(&file, zlog_core::Format::Zlox)
                .map_err(|e| e.to_string())
        });
    match saved {
        Ok(()) => {
            cx.say(format!("Z-Link: {}", file.display()));
            s.backed_up = Some(file);
        }
        Err(e) => cx.say(format!("Z-Link: {e}")),
    }
}

fn insert_before(ids: &mut Ids, q: Qso) -> Option<Command> {
    if ids.has(&q) {
        return None;
    }
    ids.0.insert(QsoId::of(&q));
    Some(Command::InsertBefore(QsoId(q.reserve2 / 100), q))
}

fn init(cx: &Context<'_>, s: &mut Session) {
    let last = cx.log().qsos.last();
    if let Some(band) = s.entry_band.or(last.map(|q| q.band)) {
        s.send_band(band);
    }
    s.send_operator();
    let pc = s.settings.pc_name.clone();
    s.write(&protocol::pc_name(&pc));
    let empty = cx.log().qsos.is_empty();
    match s.settings.on_connect {
        OnConnect::Auto if empty => s.clearing = true,
        OnConnect::Auto | OnConnect::Merge => {
            let lb = s.settings.line_break.code();
            s.write(&protocol::begin_merge(lb));
        }
        OnConnect::Download => s.clearing = true,
        OnConnect::ConnectOnly => {}
    }
}

fn end_merge(cx: &Context<'_>, s: &mut Session) {
    let Some(mut merge) = s.merge.take() else {
        return;
    };
    let qsos = &cx.log().qsos;
    for q in qsos {
        let id = q.reserve3 / 100;
        match merge.ids.get(&id).copied() {
            Some(full) if full == q.reserve3 => {
                merge.ids.remove(&id);
            }
            Some(full) if full > q.reserve3 => {}
            Some(_) => {
                merge.ids.remove(&id);
                s.write(&protocol::edit_qso_to(q));
            }
            None => s.write(&protocol::put_log(q)),
        }
    }
    if !qsos.is_empty() {
        s.write(&protocol::renew_after_log());
    }
    let wanted: Vec<i32> = merge
        .order
        .iter()
        .filter_map(|id| merge.ids.get(id).copied())
        .collect();
    if !wanted.is_empty() {
        for line in protocol::get_log_qso_id(&wanted) {
            s.write(&line);
        }
        s.write(&protocol::send_renew());
    }
    s.write(&protocol::end_merge());
}
