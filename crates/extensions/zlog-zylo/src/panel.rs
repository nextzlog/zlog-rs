use std::sync::{Arc, Mutex};

use zlog_ui::{MenuGroup, Panel, Section, View};

use crate::host::State;
use crate::manager::Manager;
use crate::protocol::{HookKind, ToHelper};
use crate::texts;

pub struct ZyloPanel {
    manager: Arc<Mutex<Manager>>,
    last_call: String,
    last_number: String,
}

impl Default for ZyloPanel {
    fn default() -> Self {
        Self::new(crate::manager::global())
    }
}

impl ZyloPanel {
    pub fn new(manager: Arc<Mutex<Manager>>) -> Self {
        ZyloPanel {
            manager,
            last_call: String::new(),
            last_number: String::new(),
        }
    }
}

fn typed(old: &str, new: &str) -> Vec<char> {
    new.strip_prefix(old)
        .map(|s| s.chars().collect())
        .unwrap_or_default()
}

pub fn state_text(lang: &str, state: Option<&State>, enabled: bool) -> String {
    let t = texts();
    match state {
        None if enabled => t.get(lang, "state.waiting").into(),
        None => t.get(lang, "state.off").into(),
        Some(State::Starting) => t.get(lang, "state.starting").into(),
        Some(State::Ready) => t.get(lang, "state.ready").into(),
        Some(State::Stopped) => t.get(lang, "state.off").into(),
        Some(State::Disabled(why)) => t.format(lang, "state.disabled", &[why]),
    }
}

impl Panel for ZyloPanel {
    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::None
    }

    fn id(&self) -> &str {
        "zylo"
    }

    fn on_exit(&mut self) {
        if let Ok(mut m) = self.manager.lock() {
            m.finish_all(crate::host::FINISH_GRACE);
        }
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Settings
    }

    fn tick_after(&self) -> Option<std::time::Duration> {
        let watched = !self.manager.lock().unwrap().targets().is_empty();
        watched.then_some(std::time::Duration::from_secs(5))
    }

    fn tick(&mut self, view: &View<'_>) {
        let mut m = self.manager.lock().unwrap();
        m.language = view.language.to_string();
        {
            let snapshot = m.snapshot();
            let mut s = snapshot.lock().unwrap();
            s.call = view.station.callsign.clone();
            s.band = view.band.label().to_string();
            s.mode = view.mode.label().to_string();
        }
        m.start_enabled();
        m.watch();
        let targets = m.targets();
        drop(m);
        let call = typed(&self.last_call, view.callsign);
        let number = typed(&self.last_number, view.number);
        if !call.is_empty() || !number.is_empty() {
            for h in &targets {
                for hook in h.hooks().iter().filter(|k| k.kind == HookKind::Editor) {
                    let keys = if hook.name.contains("Call") {
                        &call
                    } else if hook.name.contains("Number") {
                        &number
                    } else {
                        continue;
                    };
                    for c in keys {
                        h.send(ToHelper::Editor {
                            id: hook.id,
                            key: *c as i64,
                        });
                    }
                }
            }
        }
        self.last_call = view.callsign.to_string();
        self.last_number = view.number.to_string();
    }

    fn load(&mut self, section: &Section) {
        let mut m = self.manager.lock().unwrap();
        m.load(section);
    }

    fn save(&self, section: &mut Section) {
        self.manager.lock().unwrap().save(section);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_characters_added_at_the_end_are_typed() {
        assert_eq!(typed("JA", "JA1"), vec!['1']);
        assert_eq!(typed("", "AB"), vec!['A', 'B']);
        assert!(typed("JA1", "JA").is_empty());
        assert!(typed("JA", "JB").is_empty());
    }

    #[test]
    fn no_separate_window_remains() {
        let p = ZyloPanel::default();
        assert!(!p.window_kind().has_window());
        assert!(zlog_ui::panel_commands(&p, false, "en").is_empty());
    }

    #[test]
    fn states_have_texts() {
        for lang in ["en", "ja"] {
            assert!(state_text(lang, Some(&State::Disabled("x".into())), true).contains('x'));
            assert!(!state_text(lang, None, false).is_empty());
        }
    }

    #[test]
    fn exit_hook_finishes_every_plugin() {
        let fake = crate::host::fake::FakeSpawner::new(|_| vec![]);
        let seen = fake.seen.clone();
        let manager = Arc::new(Mutex::new(Manager::new(Arc::new(fake))));
        let name = format!("a.{}", crate::library::EXTENSION);
        let h = {
            let mut m = manager.lock().unwrap();
            m.set_enabled(&name, true);
            m.start_enabled();
            m.targets()[0].clone()
        };
        assert!(h.wait_ready(std::time::Duration::from_secs(5)));
        let mut panel = ZyloPanel::new(Arc::clone(&manager));
        panel.on_exit();
        assert_eq!(
            seen.lock().unwrap().last(),
            Some(&crate::protocol::ToHelper::Finish)
        );
        assert!(manager.lock().unwrap().targets().is_empty() && h.state() == State::Stopped);
    }
}
