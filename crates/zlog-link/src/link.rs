//! The link between the log and Z-Server.
//!
//! QSOs from the server are applied with [`Command`]s in [`Link::poll`]; QSOs the
//! operator logs, edits or deletes here are sent from [`Observer::on_event`]. Changes
//! the link made itself are never sent back, nor are changes made by other
//! extensions (zLog sends only from its entry row and edit dialogs).
//!
//! QSOs are matched by their ID (`Reserve3` without the edit counter), as zLog's
//! `TLog.CheckQSOID` and `TQSO.SameQSOID` do.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use zlog_core::{Command, Context, Link, LogEvent, Observer, Origin, Qso, QsoId};

use crate::protocol::{self, Message};
use crate::session::{Merge, OnConnect, Session};

pub struct LogLink {
    session: Arc<Mutex<Session>>,
    /// INSQSOAT QSOs waiting for zLog's `Log.ProcessQue` (see [`Message::InsQsoAt`]).
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

/// The QSO IDs of the log as the commands sent so far will leave it.
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
        // Forward the operator's changes and local observers' reactions to them
        // (say, one filling `entity`), as zLog sends every local edit; never what
        // came from the network (this link's poll, or another link's) nor reactions to
        // it: the sender's peers run their own observers, and the book keeps the edit
        // counter of such reactions ([`zlog_core::Book`]), so nothing diverges.
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
        // Commands sent here are not limited in number (only the book's reactions to
        // them are), so a whole download is applied in one poll.
        if s.clearing {
            // The log is replaced by the server's.
            cx.resync();
            let ids: Vec<QsoId> = cx.log().qsos.iter().map(QsoId::of).collect();
            if ids.is_empty() {
                s.clearing = false;
                s.write(&protocol::send_log());
            } else {
                for id in ids {
                    cx.send(Command::Delete(id));
                }
                return;
            }
        }
        if s.inbox.is_empty() {
            return;
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
            // zLog's commands that call `Log.ProcessQue` first apply the queue, which
            // holds the INSQSOAT QSOs received before them.
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
                // These read the whole log, which must not have changes pending.
                Message::EndQsoIds | Message::SendLog if sent => {
                    s.inbox.push_front(m);
                    break;
                }
                // The merge and SENDLOG hand the log to other stations; what was kept
                // about it before (undo steps) would edit QSOs they hold otherwise.
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
                // `LogQSO` only for a QSO with a callsign and a new ID.
                Message::PutQso(q) => {
                    if !q.callsign.is_empty() && !ids.has(&q) {
                        ids.0.insert(QsoId::of(&q));
                        send!(Command::Insert(q));
                    }
                }
                // A QSO whose ID is already here is not inserted twice. Departure from
                // zLog (`ProcessPutLog` adds it whatever it is): a QSO without a
                // callsign is skipped, as for `PUTQSO`, so a damaged line cannot add
                // an empty row to the log.
                Message::PutLog(q) => {
                    if !q.callsign.is_empty() && !ids.has(&q) {
                        ids.0.insert(QsoId::of(&q));
                        send!(Command::Insert(q));
                    }
                }
                // zLog inserts before the QSO named in `Reserve2` (`ProcessInsert`)
                // and drops the QSO when there is none; the book appends it then.
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
                // The QSO keeps the sender's edit counter, as in zLog: the book keeps
                // the `Reserve3` of an update from a link when its ID is unchanged.
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
                // The book has no QSO locks (zLog locks a QSO while one station edits
                // it); the edit that follows arrives as EDITQSOTO.
                Message::LockQso(_) | Message::UnlockQso(_) => {}
                // `Log.SortBy(soTime)`; the book rescores after every command already.
                Message::Renew => send!(Command::SortByTime),
                Message::Freq(_)
                | Message::PutMessage { .. }
                | Message::BeginMergeNg
                | Message::Other(_) => {}
            }
        }
    }
}

/// The insert of an INSQSO or INSQSOAT QSO, unless its ID is here already.
fn insert_before(ids: &mut Ids, q: Qso) -> Option<Command> {
    if ids.has(&q) {
        return None;
    }
    ids.0.insert(QsoId::of(&q));
    Some(Command::InsertBefore(QsoId(q.reserve2 / 100), q))
}

/// `TZLinkForm.InitProcess` and `TZServerInquiry`: tell the server who is here,
/// then merge or download.
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

/// `TZLinkForm.EndQsoIDsProc`: send what the server lacks or has older, ask for what
/// only the server has or has newer.
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
            // The server's is newer: ask for it below.
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
