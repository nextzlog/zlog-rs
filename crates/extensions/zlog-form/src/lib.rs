use egui::{Event, Id, Key, KeyboardShortcut, Modifiers, Rect, RichText, ViewportBuilder};
use zlog_ui::sequence::{accept_suggestions, Cursor, EnterFlow, EntryStatus};
use zlog_ui::{EntryField, EntryState, EntryStyle, Section};

zlog_core::texts!();

pub const VIEWPORT: &str = "entry_form";

const OPEN_SIZE: [f32; 2] = [420.0, 300.0];

const FIELDS: [(EntryField, &str); 5] = [
    (EntryField::Call, "field.call"),
    (EntryField::RstSent, "field.rst_sent"),
    (EntryField::RstRcvd, "field.rst_rcvd"),
    (EntryField::Number, "field.number"),
    (EntryField::Memo, "field.memo"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Enter,
    Space,
    Send(u8),
    Accept,
    Clear,
    ToggleCq,
}

const F_KEYS: [Key; 12] = [
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
];

pub fn bindings() -> Vec<(KeyboardShortcut, Action)> {
    let k = KeyboardShortcut::new;
    let mut b = vec![
        (k(Modifiers::NONE, Key::Enter), Action::Enter),
        (k(Modifiers::NONE, Key::Space), Action::Space),
        (k(Modifiers::ALT, Key::A), Action::Accept),
        (k(Modifiers::CTRL, Key::K), Action::Clear),
        (k(Modifiers::SHIFT, Key::O), Action::ToggleCq),
    ];
    b.extend(
        (1..)
            .zip(F_KEYS)
            .map(|(n, f)| (k(Modifiers::NONE, f), Action::Send(n))),
    );
    b
}

pub fn acts(action: Action, field: Option<EntryField>) -> bool {
    match action {
        Action::Space => EnterFlow::space_acts(field),
        Action::ToggleCq => field != Some(EntryField::Memo),
        Action::Enter | Action::Accept => field.is_some(),
        Action::Send(_) | Action::Clear => true,
    }
}

fn id(field: EntryField) -> Id {
    Id::unique(("entry_form", field as u8))
}

pub struct EntryForm {
    focus: Option<EntryField>,
    select_all: Option<EntryField>,
    focused: Option<EntryField>,
    flow: EnterFlow,
    open: bool,
    seen: Option<Rect>,
    place: Option<Rect>,
    fkeys: bool,
}

impl Default for EntryForm {
    fn default() -> Self {
        EntryForm {
            focus: Some(EntryField::Call),
            select_all: None,
            focused: None,
            flow: EnterFlow::default(),
            open: true,
            seen: None,
            place: None,
            fkeys: true,
        }
    }
}

impl EntryStyle for EntryForm {
    fn id(&self) -> &str {
        "form"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn reserved_keys(&self) -> Vec<KeyboardShortcut> {
        bindings().into_iter().map(|(k, _)| k).collect()
    }

    fn keys(&mut self, _ctx: &egui::Context, entry: &mut dyn EntryState) {
        self.messages_sent(entry);
        self.flow.forget_other_qso(entry);
    }

    fn ui(&mut self, ui: &mut egui::Ui, entry: &mut dyn EntryState) {
        self.floating_ui(ui, entry, &mut |ui, body| body(ui));
    }

    fn own_window(&self) -> Option<&str> {
        self.open.then_some(VIEWPORT)
    }

    fn close_window(&mut self) {
        self.open = false;
    }

    fn docked_ui(&mut self, ui: &mut egui::Ui, entry: &mut dyn EntryState) {
        self.open = true;
        let ctx = ui.ctx().clone();
        self.take_keys(&ctx, entry);
        self.window(ui, entry);
    }

    fn floating_ui(
        &mut self,
        ui: &mut egui::Ui,
        entry: &mut dyn EntryState,
        wrap: zlog_ui::entry::Wrap<'_>,
    ) {
        let lang = entry.language().to_string();
        if !self.open {
            if ui.button(texts().get(&lang, "show")).clicked() {
                self.open = true;
                self.focus = Some(EntryField::Call);
            }
            return;
        }
        if self.seen.is_none() {
            self.restore(ui.ctx());
        }
        let mut builder = ViewportBuilder::default()
            .with_title(texts().get(&lang, "title"))
            .with_inner_size(OPEN_SIZE)
            .with_min_inner_size([240.0, 160.0])
            .with_resizable(true);
        if let Some(r) = self.place {
            builder = builder.with_position(r.min).with_inner_size(r.size());
        }
        let ctx = ui.ctx().clone();
        let vid = zlog_ui::dialog::viewport_id(VIEWPORT);
        let closed = ctx.show_viewport_immediate(vid, builder, |ui, _| {
            let ctx = ui.ctx().clone();
            let (closed, rect) = ctx.input(|i| {
                let v = i.viewport();
                (v.close_requested(), seen_rect(v.outer_rect, v.inner_rect))
            });
            self.seen = rect.or(self.seen);
            wrap(ui, &mut |ui| {
                self.take_keys(&ctx, entry);
                self.window(ui, entry);
            });
            closed
        });
        if closed {
            self.open = false;
            self.place = self.seen;
        }
    }

    fn focused(&self, _ctx: &egui::Context) -> Option<EntryField> {
        self.focused
    }

    fn focus(&mut self, field: EntryField) {
        self.focus = Some(field);
    }

    fn load(&mut self, section: &Section) {
        self.flow.load(section);
        self.fkeys = zlog_ui::fkeys::shown(section);
        if self.seen.is_none() {
            self.place = load_place(section).or(self.place);
        }
    }

    fn save(&self, section: &mut Section) {
        if let Some(r) = self.seen.or(self.place) {
            save_place(section, r);
        }
    }

    fn uses_main_row(&self) -> bool {
        !self.open
    }

    fn entry_changed(&mut self) {
        self.flow.new_qso();
    }

    fn messages_sent(&mut self, entry: &mut dyn EntryState) {
        let c = self.flow.messages_sent(entry);
        self.go(c);
    }
}

fn seen_rect(outer: Option<Rect>, inner: Option<Rect>) -> Option<Rect> {
    zlog_ui::dialog::place_of(outer, inner)
}

const PLACE: &str = "form_window";

fn load_place(section: &Section) -> Option<Rect> {
    let v: Vec<f32> = section
        .get(PLACE)?
        .split(',')
        .map(|n| n.trim().parse().ok())
        .collect::<Option<_>>()?;
    let [x, y, w, h] = v[..] else {
        return None;
    };
    (w >= 100.0 && h >= 80.0).then(|| Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h)))
}

fn save_place(section: &mut Section, r: Rect) {
    let v = [r.min.x, r.min.y, r.width(), r.height()].map(|n| n.round().to_string());
    section.insert(PLACE.into(), v.join(","));
}

pub fn field_labels(lang: &str, exchange: Option<&str>) -> Vec<String> {
    FIELDS
        .iter()
        .map(|(f, k)| match (f, exchange) {
            (EntryField::Number, Some(x)) if !x.trim().is_empty() => x.to_string(),
            _ => texts().get(lang, k).to_string(),
        })
        .collect()
}

impl EntryForm {
    fn restore(&mut self, ctx: &egui::Context) {
        self.place = self.place.map(|r| zlog_ui::dialog::restored(ctx, r));
    }

    fn go(&mut self, cursor: Option<Cursor>) {
        if let Some(c) = cursor {
            self.focus = Some(c.field);
            if c.select_all {
                self.select_all = Some(c.field);
            }
        }
    }

    fn take_keys(&mut self, ctx: &egui::Context, entry: &mut dyn EntryState) {
        let field = FIELDS
            .into_iter()
            .map(|(f, _)| f)
            .find(|f| ctx.memory(|m| m.has_focus(id(*f))));
        self.focused = field;
        if ctx.memory(|m| m.focused().is_none()) && self.focus.is_none() {
            self.focus = Some(EntryField::Call);
        }
        for (key, action) in bindings() {
            if acts(action, field) && shortcut(ctx, key) {
                self.act(action, field, entry);
            }
        }
    }

    fn act(&mut self, action: Action, field: Option<EntryField>, entry: &mut dyn EntryState) {
        let c = match action {
            Action::Enter => field.and_then(|f| self.flow.enter(entry, f)),
            Action::Space => self.flow.space(entry, field),
            Action::Send(n) => {
                entry.send(n);
                None
            }
            Action::Accept => {
                accept_suggestions(entry, field);
                None
            }
            Action::Clear => Some(self.flow.clear(entry)),
            Action::ToggleCq => {
                entry.toggle_cq();
                None
            }
        };
        self.go(c);
    }

    fn window(&mut self, ui: &mut egui::Ui, entry: &mut dyn EntryState) {
        let lang = entry.language().to_string();
        egui::Panel::bottom(Id::unique("entry_form_status"))
            .resizable(false)
            .show(ui, |ui| self.status(ui, entry));
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let band = entry.band();
                        egui::ComboBox::from_id_salt("entry_form_band")
                            .selected_text(RichText::new(band.label()).strong())
                            .width(0.0)
                            .show_ui(ui, |ui| {
                                for b in entry.bands() {
                                    if ui.selectable_label(b == band, b.label()).clicked() {
                                        entry.set_band(b);
                                    }
                                }
                            });
                        let mode = entry.mode();
                        egui::ComboBox::from_id_salt("entry_form_mode")
                            .selected_text(RichText::new(mode.label()).strong())
                            .width(0.0)
                            .show_ui(ui, |ui| {
                                for m in entry.modes() {
                                    if ui.selectable_label(m == mode, m.label()).clicked() {
                                        entry.set_mode(m);
                                    }
                                }
                            });
                    });
                    let labels = field_labels(&lang, entry.exchange_label().as_deref());
                    let names: Vec<&str> = labels.iter().map(String::as_str).collect();
                    let width = zlog_ui::form_field_width(ui, &names);
                    egui::Grid::new("entry_form_fields")
                        .num_columns(2)
                        .show(ui, |ui| {
                            for (field, label) in FIELDS.iter().zip(&labels) {
                                ui.label(label.as_str());
                                self.edit(ui, width, entry, field.0);
                                ui.end_row();
                            }
                        });
                    ui.add_space(4.0);
                    self.buttons(ui, entry);
                });
        });
    }

    fn buttons(&mut self, ui: &mut egui::Ui, entry: &mut dyn EntryState) {
        if !self.fkeys {
            return;
        }
        let mut messages = entry.key_messages();
        messages.resize(12, String::new());
        if let Some(n) = zlog_ui::fkeys::message_keys(ui, &messages) {
            entry.send(n);
            self.focus = self.focused.or(Some(EntryField::Call));
        }
    }

    fn status(&self, ui: &mut egui::Ui, entry: &dyn EntryState) {
        let lang = entry.language();
        let t = texts();
        let s = EntryStatus::of(entry, &self.flow);
        ui.horizontal_wrapped(|ui| {
            if s.cq {
                ui.label(RichText::new(t.get(lang, "cq")).color(zlog_ui::palette::cq(ui)));
            } else {
                ui.label(RichText::new(t.get(lang, "sp")).color(zlog_ui::palette::sp(ui)));
            }
            if s.dupe {
                ui.separator();
                let dupe = RichText::new(t.get(lang, "dupe")).strong();
                ui.label(dupe.color(zlog_ui::palette::error(ui)));
            }
            if let Some(m) = &s.new_multi {
                ui.separator();
                let multi = RichText::new(t.format(lang, "new_multi", &[m]));
                ui.label(multi.color(zlog_ui::palette::good(ui)));
            }
            if let Some(next) = s.next {
                ui.separator();
                let name = zlog_ui::enter::message_name(lang, next.key());
                ui.label(t.format(lang, "next", &[&name]));
            }
            for g in &s.suggestions {
                ui.separator();
                ui.label(t.format(lang, "suggestion", &[&g.text]));
            }
        });
    }

    fn edit(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        entry: &mut dyn EntryState,
        field: EntryField,
    ) {
        let mut text = entry.text(field).to_string();
        if self.select_all == Some(field) {
            let mut state =
                egui::text_edit::TextEditState::load(ui.ctx(), id(field)).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::select_all_str(&text)));
            state.store(ui.ctx(), id(field));
            self.select_all = None;
        }
        let hint = entry.suggestion(field).map(|s| s.text.clone());
        let edit = egui::TextEdit::singleline(&mut text)
            .id(id(field))
            .hint_text(hint.unwrap_or_default())
            .font(egui::TextStyle::Monospace);
        let response = zlog_ui::sized_field(ui, edit, width);
        if text != entry.text(field) {
            entry.set_text(field, text);
        }
        if self.focus == Some(field) {
            response.request_focus();
            self.focus = None;
        }
    }
}

fn shortcut(ctx: &egui::Context, key: KeyboardShortcut) -> bool {
    ctx.input_mut(|i| {
        let hit = i.consume_shortcut(&key);
        if hit {
            i.events.retain(|e| !matches!(e, Event::Text(_)));
        }
        hit
    })
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.entry_style(20, EntryForm::default);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn the_window_place_survives_a_restart() {
        let mut form = EntryForm::default();
        let mut section = Section::new();
        form.save(&mut section);
        assert!(section.is_empty(), "nothing seen, nothing saved");
        form.seen = Some(Rect::from_min_size(
            egui::pos2(10.0, 20.0),
            egui::vec2(400.0, 300.0),
        ));
        form.save(&mut section);
        assert_eq!(section[PLACE], "10,20,400,300");
        let mut next = EntryForm::default();
        next.load(&section);
        assert_eq!(next.place, form.seen);
        section.insert(PLACE.into(), "1,2,3".into());
        let mut bad = EntryForm::default();
        bad.load(&section);
        assert_eq!(bad.place, None);
        form.load(&Section::new());
        assert_eq!(form.place, None, "a window up is not moved");
    }

    #[test]
    fn a_decorated_window_keeps_its_inner_size_when_reopened() {
        let inner = Rect::from_min_size(egui::pos2(108.0, 150.0), egui::vec2(400.0, 300.0));
        let outer = Rect::from_min_max(egui::pos2(100.0, 120.0), egui::pos2(516.0, 458.0));
        let mut section = Section::new();
        save_place(&mut section, seen_rect(Some(outer), Some(inner)).unwrap());
        let place = load_place(&section).unwrap();
        assert_eq!((place.min, place.size()), (outer.min, inner.size()));
        assert_eq!(seen_rect(None, Some(inner)), Some(inner));
    }

    #[test]
    fn a_place_on_a_removed_monitor_is_brought_back() {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        let root = input.viewports.entry(egui::ViewportId::ROOT).or_default();
        root.monitor_size = Some(egui::vec2(1920.0, 1080.0));
        root.outer_rect = Some(Rect::from_min_size(
            egui::pos2(100.0, 100.0),
            egui::vec2(1000.0, 700.0),
        ));
        let size = egui::vec2(400.0, 300.0);
        let second = Rect::from_min_size(egui::pos2(2500.0, 300.0), size);
        let removed = Rect::from_min_size(egui::pos2(9000.0, 300.0), size);
        let mut places = Vec::new();
        let mut out = ctx.run_ui(input, |ui| {
            for saved in [second, removed] {
                let mut form = EntryForm {
                    place: Some(saved),
                    ..EntryForm::default()
                };
                form.restore(ui.ctx());
                places.push(form.place);
            }
        });
        out.textures_delta.clear();
        assert_eq!(places[0], Some(second), "a second monitor is kept");
        assert_eq!(
            places[1],
            Some(Rect::from_min_size(egui::pos2(148.0, 164.0), size))
        );
    }

    #[test]
    fn the_number_is_labelled_by_the_exchange_and_no_strip_while_open() {
        let plain = field_labels("en", None);
        let named = field_labels("en", Some("Prefecture + power"));
        assert_eq!(named[3], "Prefecture + power");
        assert_eq!(named[0], plain[0]);
        assert_ne!(plain[3], named[3]);
        assert_eq!(field_labels("en", Some(" ")), plain);
        let mut form = EntryForm::default();
        assert!(!form.uses_main_row());
        form.open = false;
        assert!(form.uses_main_row());
    }

    #[test]
    fn function_keys_send_their_messages() {
        let b = bindings();
        let send = |key| {
            b.iter()
                .find(|(k, _)| k.logical_key == key && k.modifiers == Modifiers::NONE)
                .map(|(_, a)| *a)
        };
        assert_eq!(send(Key::F1), Some(Action::Send(1)));
        assert_eq!(send(Key::F12), Some(Action::Send(12)));
        assert_eq!(send(Key::Enter), Some(Action::Enter));
        let reserved = EntryForm::default().reserved_keys();
        assert_eq!(reserved.len(), b.len());
        assert!(reserved.contains(&KeyboardShortcut::new(Modifiers::ALT, Key::A)));
    }

    #[test]
    fn the_memo_takes_space_and_capitals() {
        let memo = Some(EntryField::Memo);
        assert!(!acts(Action::Space, memo));
        assert!(!acts(Action::ToggleCq, memo));
        assert!(acts(Action::Enter, memo));
        assert!(acts(Action::Space, Some(EntryField::Call)));
        assert!(!acts(Action::Enter, None), "Enter needs a field");
        assert!(acts(Action::Send(1), None));
    }

    #[test]
    fn the_cursor_of_the_flow_moves_the_keyboard() {
        let mut f = EntryForm::default();
        assert_eq!(f.focus, Some(EntryField::Call), "the callsign at first");
        f.go(Some(Cursor {
            field: EntryField::Number,
            select_all: true,
        }));
        assert_eq!(
            (f.focus, f.select_all),
            (Some(EntryField::Number), Some(EntryField::Number))
        );
    }
}
