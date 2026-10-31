use std::collections::{BTreeMap, BTreeSet};

use egui::Ui;
use zlog_ui::{Command, EntryAction, MenuGroup, Panel, RoleCommand, Section, View};

zlog_core::texts!();

pub fn text(language: &str, key: &str) -> String {
    texts().get(language, key).into()
}

pub const STEPS: [(&str, &[&str]); 7] = [
    ("step.1", &["open"]),
    ("step.2", &["station"]),
    ("step.7", &[]),
    ("step.3", &["history"]),
    ("step.4", &["score", "multi", "check"]),
    ("step.5", &["elog", "jarl_upload"]),
    ("step.6", &[]),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Ready(String),
    Disabled(String, String),
    Missing,
}

#[derive(Default)]
pub struct Guide {
    step: usize,
    commands: Vec<RoleCommand>,
    done: BTreeMap<String, BTreeSet<usize>>,
    log: String,
    actions: Vec<EntryAction>,
}

pub fn log_key(view: &View<'_>) -> String {
    let id = format!(
        "{}|{}",
        view.contest.name(),
        view.log.start_time().0.to_bits()
    );
    let hash = id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

impl Guide {
    pub fn link(&self, role: &str, language: &str) -> Link {
        let of_role = || self.commands.iter().filter(|c| c.role == role);
        if let Some(c) = of_role().find(|c| c.enabled) {
            return Link::Ready(c.label.clone());
        }
        match of_role().next() {
            Some(c) => Link::Disabled(
                c.label.clone(),
                c.disabled_reason
                    .clone()
                    .unwrap_or_else(|| texts().get(language, "unavailable").into()),
            ),
            None => Link::Missing,
        }
    }

    pub fn run(&mut self, role: &str) -> bool {
        let ready = matches!(self.link(role, "en"), Link::Ready(_));
        if ready {
            self.actions.push(EntryAction::Run(role.into()));
        }
        ready
    }

    pub fn step(&self) -> usize {
        self.step
    }

    pub fn show_step(&mut self, i: usize) {
        self.step = i.min(STEPS.len() - 1);
    }

    pub fn is_done(&self, i: usize) -> bool {
        self.done.get(&self.log).is_some_and(|s| s.contains(&i))
    }

    pub fn set_done(&mut self, i: usize, done: bool) {
        let steps = self.done.entry(self.log.clone()).or_default();
        if done {
            steps.insert(i);
        } else {
            steps.remove(&i);
        }
    }

    pub fn next(&mut self) {
        self.set_done(self.step, true);
        self.show_step(self.step + 1);
    }

    fn done_count(&self) -> usize {
        self.done.get(&self.log).map_or(0, BTreeSet::len)
    }
}

impl Panel for Guide {
    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::Dialog
    }

    fn id(&self) -> &str {
        "guide"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [560.0, 480.0]
    }

    fn menu(&self) -> MenuGroup {
        MenuGroup::Help
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        let t = |k| texts().get(language, k);
        let mut c = Command::window(t("title"), t("menu.tip")).menu_label(t("menu"));
        c.toolbar = false;
        vec![c]
    }

    fn role_commands(&mut self, commands: &[RoleCommand]) {
        if self.commands != commands {
            self.commands = commands.to_vec();
        }
    }

    fn tick(&mut self, view: &View<'_>) {
        self.log = log_key(view);
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let t = |k: &str| texts().get(lang, k).to_string();
        self.log = log_key(view);
        ui.label(texts().format(lang, "progress", &[&self.done_count(), &STEPS.len()]));
        ui.add_space(4.0);
        for (i, (key, _)) in STEPS.iter().enumerate() {
            ui.horizontal(|ui| {
                let mut done = self.is_done(i);
                if ui
                    .checkbox(&mut done, "")
                    .on_hover_text(t("done"))
                    .changed()
                {
                    self.set_done(i, done);
                }
                let title = format!("{}. {}", i + 1, t(key));
                if ui.selectable_label(self.step == i, title).clicked() {
                    self.step = i;
                }
            });
        }
        ui.separator();
        let (key, roles) = STEPS[self.step];
        ui.heading(format!("{}. {}", self.step + 1, t(key)));
        ui.add(egui::Label::new(t(&format!("{key}.text"))).wrap());
        ui.add_space(6.0);
        for role in roles {
            let link = self.link(role, lang);
            ui.horizontal_wrapped(|ui| match link {
                Link::Ready(label) => {
                    if ui.button(label).clicked() {
                        self.run(role);
                    }
                }
                Link::Disabled(label, why) => {
                    ui.add_enabled(false, egui::Button::new(label))
                        .on_disabled_hover_text(&why);
                    ui.weak(why);
                }
                Link::Missing => {
                    ui.add_enabled(false, egui::Button::new(*role))
                        .on_disabled_hover_text(t("missing"));
                    ui.weak(t("missing"));
                }
            });
        }
        let mut done = self.is_done(self.step);
        if ui.checkbox(&mut done, t("done")).changed() {
            self.set_done(self.step, done);
        }
    }

    fn take_actions(&mut self) -> Vec<EntryAction> {
        std::mem::take(&mut self.actions)
    }

    fn load(&mut self, section: &Section) {
        self.done = section
            .iter()
            .filter_map(|(k, v)| {
                let log = k.strip_prefix("done.")?;
                let steps = v
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .filter(|i| *i < STEPS.len())
                    .collect();
                Some((log.to_string(), steps))
            })
            .collect();
    }

    fn save(&self, section: &mut Section) {
        section.retain(|k, _| !k.starts_with("done."));
        for (log, steps) in self.done.iter().filter(|(_, s)| !s.is_empty()) {
            let list: Vec<String> = steps.iter().map(usize::to_string).collect();
            section.insert(format!("done.{log}"), list.join(","));
        }
    }

    fn capability_mut(&mut self, want: zlog_ui::WantMut) -> Option<zlog_ui::CapabilityMut<'_>> {
        match want {
            zlog_ui::WantMut::Buttons => Some(zlog_ui::CapabilityMut::Buttons(self)),
            _ => None,
        }
    }
}

impl zlog_ui::ButtonRow for Guide {
    fn buttons(&mut self, ui: &mut Ui, view: &View<'_>) -> bool {
        let t = |k| texts().get(view.language, k);
        let last = self.step + 1 == STEPS.len();
        zlog_ui::form::buttons(ui, |ui| {
            let next = if last { t("finish") } else { t("next") };
            let mut close = false;
            if ui.button(next).clicked() {
                close = last;
                self.next();
            }
            if ui
                .add_enabled(self.step > 0, egui::Button::new(t("back")))
                .clicked()
            {
                self.step -= 1;
            }
            close |=
                ui.button(t("close")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
            close
        })
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(510, |_| Guide::default());
}

#[cfg(test)]
mod tests;
