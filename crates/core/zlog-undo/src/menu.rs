use zlog_core::{Change, Qso, Step};
use zlog_ui::{Command, EntryAction, MenuGroup, Panel, StatusItem, View};

use crate::history::{Refusal, Shared};
use crate::texts;

pub struct UndoMenu {
    history: Shared,
    language: String,
    actions: Vec<EntryAction>,
    linked: bool,
}

impl Default for UndoMenu {
    fn default() -> Self {
        UndoMenu::new(crate::shared())
    }
}

impl UndoMenu {
    pub fn new(history: Shared) -> Self {
        UndoMenu {
            history,
            language: "en".into(),
            actions: Vec::new(),
            linked: true,
        }
    }

    pub fn linked(mut self, linked: bool) -> Self {
        self.linked = linked;
        self
    }
}

fn side(redo: bool) -> &'static str {
    if redo {
        "redo"
    } else {
        "undo"
    }
}

pub fn what(step: &Step, redo: bool, language: &str) -> String {
    fn done(c: &Change, redo: bool) -> (bool, Option<&Qso>) {
        if redo {
            (c.after.is_some(), c.before.as_ref())
        } else {
            (c.before.is_some(), c.after.as_ref())
        }
    }
    let kind = |c: &Change| match done(c, redo) {
        (false, _) => "log",
        (true, Some(_)) => "edit",
        (true, None) => "delete",
    };
    let first = if redo {
        step.changes.last()
    } else {
        step.changes.first()
    };
    let Some(first) = first else {
        return String::new();
    };
    if step.tag == Some(zlog_core::IMPORT) {
        return texts().format(language, "what.import", &[&step.changes.len()]);
    }
    let kind = if step.changes.iter().all(|c| kind(c) == kind(first)) {
        kind(first)
    } else {
        "change"
    };
    let (_, left) = done(first, redo);
    let call = left
        .or(first.before.as_ref())
        .or(first.after.as_ref())
        .map_or("", |q| q.callsign.as_str());
    let t = texts();
    match step.changes.len() - 1 {
        0 => t.format(language, &format!("what.{kind}"), &[&call]),
        more => t.format(language, &format!("what.{kind}.more"), &[&call, &more]),
    }
}

pub fn reason(step: Option<&Step>, refusal: Refusal, redo: bool, language: &str) -> String {
    let t = texts();
    let side = side(redo);
    let m = match refusal {
        Refusal::Empty => return t.get(language, &format!("{side}.none")).into(),
        Refusal::Changed(m) => m,
    };
    let key = match m {
        zlog_core::Mismatch::Edited(_) => "edited",
        zlog_core::Mismatch::Deleted(_) => "deleted",
        zlog_core::Mismatch::Taken(_) => "taken",
    };
    let call = step
        .and_then(|s| s.changes.iter().find(|c| c.id == m.id()))
        .and_then(|c| c.after.as_ref().or(c.before.as_ref()))
        .map_or(String::new(), |q| q.callsign.clone());
    t.format(language, &format!("{side}.{key}"), &[&call])
}

impl Panel for UndoMenu {
    fn id(&self) -> &str {
        "undo"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::None
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Edit
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        let t = texts();
        let h = self.history.lock().unwrap();
        let step = |redo: bool| {
            let side = side(redo);
            let label = match h.next(redo) {
                Some(s) => t.format(
                    language,
                    &format!("{side}.step"),
                    &[&what(s, redo, language)],
                ),
                None => t.get(language, side).into(),
            };
            let glyph = if redo {
                zlog_ui::icon::ARROW_CLOCKWISE
            } else {
                zlog_ui::icon::ARROW_COUNTER_CLOCKWISE
            };
            let c = Command::new(
                side,
                t.get(language, side),
                t.get(language, &format!("{side}.tip")),
            )
            .menu_label(label)
            .icon(glyph)
            .toolbar_key(format!("undo.{side}"))
            .section(zlog_ui::MenuSection::Undo, i32::from(redo));
            match h.blocked(redo) {
                Some(r) => c.disabled_because(reason(h.next(redo), r, redo, language)),
                None => c,
            }
        };
        let mut skip = Command::new("skip", t.get(language, "skip"), t.get(language, "skip.tip"))
            .section(zlog_ui::MenuSection::Undo, 2);
        if h.skippable().is_none() {
            skip = skip.disabled_because(t.get(language, "skip.none"));
        }
        match self.linked {
            true => vec![step(false), step(true), skip],
            false => vec![step(false), step(true)],
        }
    }

    fn run_command(&mut self, id: &str) {
        let t = texts();
        let lang = self.language.as_str();
        let mut h = self.history.lock().unwrap();
        match id {
            "undo" | "redo" => {
                let redo = id == "redo";
                let Some(what) = h.next(redo).map(|s| what(s, redo, lang)) else {
                    return;
                };
                if let Some(batch) = h.batch(redo) {
                    let done = t.format(lang, &format!("{id}.done"), &[&what]);
                    self.actions.push(EntryAction::Edit {
                        batch,
                        done: Some(done),
                    });
                }
            }
            "skip" => {
                if let Some(redo) = h.skippable() {
                    h.skip(redo);
                    let said = StatusItem::new(t.get(lang, "skipped"));
                    self.actions.push(EntryAction::Say(said));
                }
            }
            _ => {}
        }
    }

    fn tick(&mut self, view: &View<'_>) {
        self.language = view.language.to_string();
        let mut h = self.history.lock().unwrap();
        if h.stale() {
            h.refresh(&view.log.qsos);
        }
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }
}
