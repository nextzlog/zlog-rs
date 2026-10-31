use egui::{Event, Id, Key, Modifiers, RichText};
use zlog_core::{Band, DelphiTime, Mode, Power};
use zlog_ui::sequence::{accept_suggestions, Cursor, EnterFlow};
use zlog_ui::{EntryField, EntryState, EntryStyle, Section};

use crate::texts;

const ROW_HEIGHT: f32 = 18.0;

const FIELDS: [EntryField; 5] = [
    EntryField::Call,
    EntryField::RstSent,
    EntryField::RstRcvd,
    EntryField::Number,
    EntryField::Memo,
];

fn previous_field(field: EntryField) -> EntryField {
    let at = FIELDS.iter().position(|f| *f == field).unwrap_or(0);
    FIELDS[(at + FIELDS.len() - 1) % FIELDS.len()]
}

fn id(field: EntryField) -> Id {
    Id::unique(("entry", field as u8))
}

const TEMP_MAX: usize = 5;

pub const OPERATOR_ROLE: &str = "operator";
pub const CQ_REPEAT_ROLE: &str = "cq_repeat";
pub const CQ_HOLD_ROLE: &str = "cq_hold";
pub const SPEED_UP_ROLE: &str = "speed_up";
pub const SPEED_DOWN_ROLE: &str = "speed_down";
pub const KEYBOARD_ROLE: &str = "keyboard";
pub const BANK_ROLE: &str = "bank";

const SHIFT_ROLES: [(Key, &str); 4] = [
    (Key::Z, CQ_HOLD_ROLE),
    (Key::Y, SPEED_UP_ROLE),
    (Key::U, SPEED_DOWN_ROLE),
    (Key::F, BANK_ROLE),
];

const EDIT: Modifiers = Modifiers::CTRL;
const BACK: Modifiers = EDIT.plus(Modifiers::SHIFT);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOp {
    Home,
    End,
    Left,
    Right,
    DeleteAt,
    DeleteLeft,
    DeleteRight,
    Clear,
}

const EDIT_KEYS: [(Key, EditOp); 8] = [
    (Key::A, EditOp::Home),
    (Key::E, EditOp::End),
    (Key::B, EditOp::Left),
    (Key::F, EditOp::Right),
    (Key::D, EditOp::DeleteAt),
    (Key::H, EditOp::DeleteLeft),
    (Key::J, EditOp::DeleteRight),
    (Key::W, EditOp::Clear),
];

pub fn apply(op: EditOp, text: &str, at: usize) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let at = at.min(n);
    let joined = |a: &[char], b: &[char]| a.iter().chain(b).collect::<String>();
    match op {
        EditOp::Home => (text.into(), 0),
        EditOp::End => (text.into(), n),
        EditOp::Left => (text.into(), at.saturating_sub(1)),
        EditOp::Right => (text.into(), (at + 1).min(n)),
        EditOp::DeleteAt if at < n => (joined(&chars[..at], &chars[at + 1..]), at),
        EditOp::DeleteAt => (text.into(), at),
        EditOp::DeleteLeft if at > 0 => (joined(&chars[..at - 1], &chars[at..]), at - 1),
        EditOp::DeleteLeft => (text.into(), 0),
        EditOp::DeleteRight => (chars[..at].iter().collect(), at),
        EditOp::Clear => (String::new(), 0),
    }
}

pub fn decrease_r(rst: &str) -> Option<String> {
    let n: u32 = rst.trim().parse().ok()?;
    let place = if n < 100 { 10 } else { 100 };
    let r = n / place % 10;
    let r2 = if r <= 1 { 5 } else { r - 1 };
    Some((n - r * place + r2 * place).to_string())
}

pub fn add_minutes(t: DelphiTime, minutes: i32) -> DelphiTime {
    DelphiTime(t.0 + f64::from(minutes) / 1440.0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    texts: Vec<String>,
    band: Band,
    mode: Mode,
    power: Power,
}

impl Kept {
    fn of(entry: &dyn EntryState) -> Self {
        Kept {
            texts: FIELDS.iter().map(|f| entry.text(*f).to_string()).collect(),
            band: entry.band(),
            mode: entry.mode(),
            power: entry.power(),
        }
    }

    fn restore(&self, entry: &mut dyn EntryState) {
        entry.set_band(self.band);
        entry.set_mode(self.mode);
        entry.set_power(self.power);
        for (f, t) in FIELDS.iter().zip(&self.texts) {
            entry.set_text(*f, t.clone());
        }
    }
}

#[derive(Default)]
pub struct ZlogRow {
    focus: Option<EntryField>,
    last_field: Option<EntryField>,
    select_all: Option<EntryField>,
    flow: EnterFlow,
    edit_op: Option<(EntryField, EditOp)>,
    kept: Vec<Kept>,
    navigator: bool,
}

pub const NAVIGATOR: &str = "navigator";

pub fn navigator_on(section: &Section) -> bool {
    section.get(NAVIGATOR).is_none_or(|v| v != "false")
}

fn navigator_ui(ui: &mut egui::Ui, flow: &EnterFlow, entry: &dyn EntryState, shown: &mut bool) {
    let t = texts();
    let lang = entry.language();
    let Some((m, key)) = flow.guide(entry) else {
        return;
    };
    let name = zlog_ui::enter::message_name(lang, m.key());
    ui.label(egui::RichText::new(t.format(lang, "nav.next", &[&key, &name])).strong())
        .on_hover_text(t.get(lang, "nav.tip"));
    if ui
        .small_button("\u{00D7}")
        .on_hover_text(t.get(lang, "nav.hide"))
        .clicked()
    {
        *shown = false;
    }
}

fn shortcut(ctx: &egui::Context, modifiers: Modifiers, key: Key) -> bool {
    ctx.input_mut(|i| zlog_ui::keys::consume_shortcut(i, modifiers, key))
}

fn typed(ctx: &egui::Context, text: &str, key: Key) -> bool {
    ctx.input_mut(|i| {
        let hit = zlog_ui::keys::consume_typed(i, text, key);
        if hit {
            i.events.retain(|e| !matches!(e, Event::Text(_)));
        }
        hit
    })
}

pub fn shift_keys_act(field: Option<EntryField>) -> bool {
    field != Some(EntryField::Memo)
}

pub fn refocus_after_click(last: Option<EntryField>) -> EntryField {
    last.unwrap_or(EntryField::Call)
}

impl EntryStyle for ZlogRow {
    fn id(&self) -> &str {
        "zlog"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn reserved_keys(&self) -> Vec<egui::KeyboardShortcut> {
        let shift = [
            Key::B,
            Key::M,
            Key::P,
            Key::O,
            Key::X,
            Key::R,
            Key::S,
            Key::T,
            Key::Z,
            Key::Y,
            Key::U,
            Key::F,
            Key::Enter,
            Key::Tab,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::Space,
        ]
        .map(|k| (Modifiers::SHIFT, k));
        let alt = [
            Key::A,
            Key::C,
            Key::N,
            Key::M,
            Key::R,
            Key::O,
            Key::K,
            Key::W,
        ]
        .map(|k| (Modifiers::ALT, k));
        let plain = [
            Key::Space,
            Key::Enter,
            Key::Tab,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::Semicolon,
        ]
        .map(|k| (Modifiers::NONE, k));
        let ctrl = EDIT_KEYS
            .map(|(k, _)| k)
            .into_iter()
            .chain([Key::K, Key::U, Key::G, Key::O, Key::P, Key::Z])
            .map(|k| (EDIT, k));
        let back = [Key::B, Key::M, Key::P].map(|k| (BACK, k));
        shift
            .into_iter()
            .chain(back)
            .chain(alt)
            .chain(plain)
            .chain(ctrl)
            .map(|(m, k)| egui::KeyboardShortcut::new(m, k))
            .collect()
    }

    fn keys(&mut self, ctx: &egui::Context, entry: &mut dyn EntryState) {
        self.messages_sent(entry);
        self.flow.forget_other_qso(entry);
        let field = self.focused(ctx);
        if field.is_some() {
            self.last_field = field;
        } else if zlog_ui::keys::text_focused(ctx) {
            return;
        }
        let shift = shift_keys_act(field);
        if shortcut(ctx, BACK, Key::B) {
            entry.cycle_band(true);
        }
        if shortcut(ctx, BACK, Key::M) {
            entry.cycle_mode(true);
        }
        if shortcut(ctx, BACK, Key::P) {
            entry.cycle_power(true);
        }
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::B) {
            entry.cycle_band(false);
        }
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::M) {
            entry.cycle_mode(false);
        }
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::P) {
            entry.cycle_power(false);
        }
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::O) {
            entry.toggle_cq();
        }
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::X) {
            entry.toggle_rig();
        }
        let rst = if field == Some(EntryField::RstSent) {
            EntryField::RstSent
        } else {
            EntryField::RstRcvd
        };
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::R) {
            if let Some(t) = decrease_r(entry.text(rst)) {
                entry.set_text(rst, t);
            }
        }
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::S) {
            entry.bump_s(rst, false);
        }
        if shift && shortcut(ctx, Modifiers::SHIFT, Key::T) {
            set_time_now(entry);
        }
        for (key, minutes) in [(Key::O, -1), (Key::P, 1)] {
            if shortcut(ctx, EDIT, key) {
                shift_time(entry, minutes);
            }
        }
        if let Some(f) = field {
            for (key, op) in EDIT_KEYS {
                if shortcut(ctx, EDIT, key) {
                    self.edit_op = Some((f, op));
                }
            }
        }
        if shortcut(ctx, EDIT, Key::U) {
            self.keep(entry);
        }
        if shortcut(ctx, EDIT, Key::G) {
            let c = self.recall(entry, field.unwrap_or(EntryField::Call));
            self.go(Some(c));
        }
        if shortcut(ctx, Modifiers::ALT, Key::O) {
            entry.run_role(OPERATOR_ROLE);
        }
        for (key, role) in SHIFT_ROLES {
            if shift && shortcut(ctx, Modifiers::SHIFT, key) {
                entry.run_role(role);
            }
        }
        if shortcut(ctx, EDIT, Key::Z) {
            entry.run_role(CQ_REPEAT_ROLE);
        }
        if shortcut(ctx, Modifiers::ALT, Key::K) {
            entry.run_role(KEYBOARD_ROLE);
        }
        if let Some(f) = field.filter(|_| shortcut(ctx, Modifiers::SHIFT, Key::Tab)) {
            self.focus = Some(previous_field(f));
        }
        if field.is_some() && shortcut(ctx, Modifiers::NONE, Key::Tab) {
            let c = self.flow.tab(entry);
            self.go(c);
        }
        if let Some(f) = field {
            if shortcut(ctx, Modifiers::NONE, Key::ArrowDown)
                || (f != EntryField::Memo && typed(ctx, ";", Key::Semicolon))
            {
                let c = self.flow.complete(entry, f);
                self.go(c);
            }
            if shortcut(ctx, Modifiers::SHIFT, Key::Enter) {
                let c = self.flow.force_enter(entry, f);
                self.go(c);
            }
        }
        for (key, up) in [(Key::ArrowUp, true), (Key::ArrowDown, false)] {
            match field {
                Some(f @ (EntryField::RstSent | EntryField::RstRcvd))
                    if shortcut(ctx, Modifiers::SHIFT, key) =>
                {
                    entry.bump_s(f, up)
                }
                _ => {}
            }
        }
        if field.is_some() && shortcut(ctx, Modifiers::ALT, Key::A) {
            accept_suggestions(entry, field);
        }
        for (key, target) in [
            (Key::C, EntryField::Call),
            (Key::N, EntryField::Number),
            (Key::M, EntryField::Memo),
            (Key::R, EntryField::RstRcvd),
        ] {
            if shortcut(ctx, Modifiers::ALT, key) {
                self.focus = Some(target);
            }
        }
        if shortcut(ctx, EDIT, Key::K) || shortcut(ctx, Modifiers::ALT, Key::W) {
            let c = self.flow.clear(entry);
            self.go(Some(c));
        }
        if EnterFlow::space_acts(field)
            && (shortcut(ctx, Modifiers::NONE, Key::Space)
                || shortcut(ctx, Modifiers::SHIFT, Key::Space))
        {
            let c = self.flow.space(entry, field);
            self.go(c);
        }
        if let Some(f) = field {
            if shortcut(ctx, Modifiers::NONE, Key::Enter) {
                let c = self.flow.enter(entry, f);
                self.go(c);
            }
        }
        if let Some(f) = field {
            if entry.qso_count() > 0 && shortcut(ctx, Modifiers::NONE, Key::ArrowUp) {
                entry.enter_log();
                ctx.memory_mut(|m| m.surrender_focus(id(f)));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, entry: &mut dyn EntryState) {
        let widths = entry.columns().to_vec();
        let width = |col: usize| widths.get(col).copied().unwrap_or(60.0);
        let (_, mo, d, h, mi, _) = typed_time(entry).to_ymd_hms();
        let sent = entry.sent();
        let operator = entry.operator().to_string();
        ui.horizontal(|ui| {
            fixed_label(ui, width(0), "");
            fixed_label(ui, width(1), &format!("{mo:02}/{d:02}"));
            fixed_label(ui, width(2), &format!("{h:02}:{mi:02}"));
            self.edit(ui, width(3), entry, EntryField::Call);
            self.edit(ui, width(4), entry, EntryField::RstSent);
            fixed_label(ui, width(5), &sent);
            self.edit(ui, width(6), entry, EntryField::RstRcvd);
            self.edit(ui, width(7), entry, EntryField::Number);
            fixed_label(ui, width(8), "");
            let mut clicked = false;
            let band = entry.band();
            let bands = entry.bands();
            match choice(ui, width(9), "band", band.label(), &bands, band, |b| {
                b.label().into()
            }) {
                Some(Choice::Pick(b)) => {
                    entry.set_band(b);
                    clicked = true;
                }
                Some(Choice::Step) => {
                    entry.cycle_band(false);
                    clicked = true;
                }
                None => {}
            }
            let mode = entry.mode();
            let modes = entry.modes();
            match choice(ui, width(10), "mode", mode.label(), &modes, mode, |m| {
                m.label().into()
            }) {
                Some(Choice::Pick(m)) => {
                    entry.set_mode(m);
                    clicked = true;
                }
                Some(Choice::Step) => {
                    entry.cycle_mode(false);
                    clicked = true;
                }
                None => {}
            }
            let power = entry.power();
            let powers = entry.powers();
            let code = power.code().map(String::from).unwrap_or_default();
            let lang = entry.language().to_string();
            let name = |p: Power| power_name(&lang, p);
            match choice(ui, width(11), "power", &code, &powers, power, name) {
                Some(Choice::Pick(p)) => {
                    entry.set_power(p);
                    clicked = true;
                }
                Some(Choice::Step) => {
                    entry.cycle_power(false);
                    clicked = true;
                }
                None => {}
            }
            if clicked {
                self.focus = Some(refocus_after_click(self.last_field));
            }
            fixed_label(ui, width(12), &operator);
            self.edit(ui, width(13), entry, EntryField::Memo);
            if self.navigator {
                navigator_ui(ui, &self.flow, entry, &mut self.navigator);
            } else if let Some(next) = self.flow.next_message(entry) {
                let t = texts();
                let lang = entry.language();
                let message = zlog_ui::enter::message_name(lang, next);
                ui.label(t.format(lang, "enter.next", &[&message]));
            }
        });
    }

    fn focused(&self, ctx: &egui::Context) -> Option<EntryField> {
        FIELDS
            .into_iter()
            .find(|f| ctx.memory(|m| m.has_focus(id(*f))))
    }

    fn focus(&mut self, field: EntryField) {
        self.focus = Some(field);
    }

    fn load(&mut self, section: &Section) {
        self.flow.load(section);
        self.navigator = navigator_on(section);
    }

    fn save(&self, section: &mut Section) {
        section.insert(NAVIGATOR.into(), self.navigator.to_string());
    }

    fn entry_changed(&mut self) {
        self.flow.new_qso();
    }

    fn caret_to_end(&mut self, field: EntryField) {
        self.edit_op = Some((field, EditOp::End));
    }

    fn messages_sent(&mut self, entry: &mut dyn EntryState) {
        let c = self.flow.messages_sent(entry);
        self.go(c);
    }
}

fn typed_time(entry: &dyn EntryState) -> DelphiTime {
    let now = entry.now();
    zlog_ui::entry_time(
        entry.text(EntryField::Date),
        entry.text(EntryField::Time),
        now,
    )
    .unwrap_or(now)
}

fn set_time(entry: &mut dyn EntryState, t: DelphiTime) {
    let (y, mo, d, h, mi, s) = t.to_ymd_hms();
    entry.set_text(EntryField::Date, format!("{y:04}-{mo:02}-{d:02}"));
    entry.set_text(EntryField::Time, format!("{h:02}:{mi:02}:{s:02}"));
}

pub fn shift_time(entry: &mut dyn EntryState, minutes: i32) {
    if entry.caps().time {
        let t = add_minutes(typed_time(entry), minutes);
        set_time(entry, t);
    }
}

pub fn set_time_now(entry: &mut dyn EntryState) {
    if entry.caps().time {
        entry.set_text(EntryField::Date, String::new());
        entry.set_text(EntryField::Time, String::new());
    }
}

impl ZlogRow {
    pub fn keep(&mut self, entry: &dyn EntryState) {
        self.kept.insert(0, Kept::of(entry));
        self.kept.truncate(TEMP_MAX);
    }

    pub fn recall(&mut self, entry: &mut dyn EntryState, field: EntryField) -> Cursor {
        if !self.kept.is_empty() {
            self.kept[0].restore(entry);
            set_time_now(entry);
            self.kept.rotate_left(1);
        }
        Cursor::to(field)
    }

    fn go(&mut self, cursor: Option<Cursor>) {
        if let Some(c) = cursor {
            self.focus = Some(c.field);
            if c.select_all {
                self.select_all = Some(c.field);
            }
        }
    }
}

impl ZlogRow {
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
        if let Some((_, op)) = self.edit_op.filter(|(f, _)| *f == field) {
            self.edit_op = None;
            let mut state =
                egui::text_edit::TextEditState::load(ui.ctx(), id(field)).unwrap_or_default();
            let n = text.chars().count();
            let at = state
                .cursor
                .char_range()
                .map_or(n, |r| r.primary.index.min(r.secondary.index).into());
            let (t, at) = apply(op, &text, at);
            text = t;
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(at),
                )));
            state.store(ui.ctx(), id(field));
        }
        let hint = entry.suggestion(field).map(|s| s.text.clone());
        let response = ui.add_sized(
            [width, ROW_HEIGHT + 2.0],
            egui::TextEdit::singleline(&mut text)
                .id(id(field))
                .hint_text(hint.unwrap_or_default())
                .lock_focus(true)
                .font(egui::TextStyle::Monospace),
        );
        if text != entry.text(field) {
            entry.set_text(field, text);
        }
        if self.focus == Some(field) {
            response.request_focus();
            self.focus = None;
        }
        if response.gained_focus() {
            entry.leave_log();
        }
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            self.last_field = Some(EntryField::Call);
            ui.memory_mut(|m| m.request_focus(id(EntryField::Call)));
        }
    }
}

fn fixed_label(ui: &mut egui::Ui, width: f32, text: &str) {
    ui.add_sized(
        [width, ROW_HEIGHT + 2.0],
        egui::Label::new(RichText::new(text).monospace()),
    );
}

enum Choice<T> {
    Pick(T),
    Step,
}

fn choice<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    width: f32,
    salt: &str,
    text: &str,
    items: &[T],
    current: T,
    name: impl Fn(T) -> String,
) -> Option<Choice<T>> {
    let response = ui.add_sized(
        [width, ROW_HEIGHT + 2.0],
        egui::Button::new(RichText::new(text).monospace()),
    );
    if items.is_empty() {
        return response.clicked().then_some(Choice::Step);
    }
    let mut picked = None;
    egui::Popup::menu(&response)
        .id(Id::unique(("entry.choice", salt)))
        .show(|ui| {
            for item in items {
                let label = RichText::new(name(*item)).monospace();
                if ui.selectable_label(*item == current, label).clicked() {
                    picked = Some(Choice::Pick(*item));
                    ui.close();
                }
            }
        });
    picked
}

fn power_name(lang: &str, power: Power) -> String {
    let key = match power.code() {
        Some('P') => "enter.power.p",
        Some('L') => "enter.power.l",
        Some('M') => "enter.power.m",
        Some('H') => "enter.power.h",
        _ => return power.label().into(),
    };
    texts().get(lang, key).to_string()
}

#[cfg(test)]
mod keys;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_keys_type_capitals_in_the_memo() {
        assert!(!shift_keys_act(Some(EntryField::Memo)));
        for f in [
            None,
            Some(EntryField::Call),
            Some(EntryField::Number),
            Some(EntryField::RstSent),
        ] {
            assert!(shift_keys_act(f), "{f:?}");
        }
    }

    #[test]
    fn click_on_a_box_returns_to_the_last_field() {
        assert_eq!(
            refocus_after_click(Some(EntryField::Number)),
            EntryField::Number
        );
        assert_eq!(
            refocus_after_click(Some(EntryField::Memo)),
            EntryField::Memo
        );
        assert_eq!(refocus_after_click(None), EntryField::Call);
    }

    #[test]
    fn the_cursor_of_the_flow_moves_the_keyboard() {
        let mut r = ZlogRow::default();
        r.go(None);
        assert_eq!(r.focus, None);
        r.go(Some(Cursor {
            field: EntryField::Number,
            select_all: true,
        }));
        assert_eq!(
            (r.focus, r.select_all),
            (Some(EntryField::Number), Some(EntryField::Number))
        );
    }
}
