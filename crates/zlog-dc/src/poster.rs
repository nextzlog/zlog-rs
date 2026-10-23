//! The observer: queues a line for every QSO the operator logs, and for their
//! corrections and deletions when chosen, once their step is done, in the queue of
//! every destination switched on. It only reads
//! the log and never sends a command.
//!
//! Only the operator's steps are seen ([`Observer::on_step`]), so the QSOs received
//! from other stations (Z-Link) are left to those stations and the QSOs of a file
//! being opened are not posted. A QSO is posted as logged only when the operator
//! logged it with a command of their own, not when a batch of an extension (an
//! undo, an edit dialog, a plugin) added it; nothing of an import is posted.

use std::sync::Arc;

use zlog_core::{Context, Log, LogEvent, Observer, Step, IMPORT};

use crate::format::{line, Change};
use crate::site::Site;
use crate::Hub;

pub struct Poster {
    hub: Arc<Hub>,
}

impl Poster {
    pub fn new(hub: Arc<Hub>) -> Self {
        Poster { hub }
    }
}

impl Observer for Poster {
    fn name(&self) -> &str {
        "Discord"
    }

    fn on_event(&mut self, _cx: &mut Context<'_>, _event: &LogEvent) {}

    fn on_step(&mut self, _log: &Log, step: &Step) {
        if step.tag == Some(IMPORT) {
            return;
        }
        let mut s = self.hub.state.lock().unwrap();
        let sites: Vec<Site> = Site::ALL
            .into_iter()
            .filter(|site| s.config.target(*site).enabled)
            .collect();
        if sites.is_empty() {
            return;
        }
        let mut posted = false;
        for c in &step.changes {
            let change = match (&c.before, &c.after) {
                (None, Some(q)) if step.tag.is_none() => Change::Logged(q),
                (Some(before), Some(after)) if s.config.edits => {
                    Change::Corrected { before, after }
                }
                (Some(q), None) if s.config.edits => Change::Deleted(q),
                _ => continue,
            };
            let text = line(&change, &s.station, &s.contest, &s.language);
            for site in &sites {
                s.outbox_mut(*site).push(text.clone());
            }
            posted = true;
        }
        if posted {
            self.hub.wake.notify_all();
        }
    }
}
