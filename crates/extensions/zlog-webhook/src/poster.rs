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
        crate::NAME
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
                (None, Some(q)) if step.tag.is_none() || s.config.edits => Change::Logged(q),
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
