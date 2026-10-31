use egui::{Event, Id, Key, KeyboardShortcut, Modifiers, RichText};
use zlog_core::{Band, DelphiTime, Mode};
use zlog_ui::sequence::{accept_suggestions, not_logged, Cursor, EnterFlow};
use zlog_ui::{EntryField, EntryState, EntryStyle, LogOptions, Period};

zlog_core::texts!();

const FIELDS: [(EntryField, &str); 7] = [
    (EntryField::Date, "field.date"),
    (EntryField::Time, "field.time"),
    (EntryField::Call, "field.call"),
    (EntryField::RstSent, "field.rst_sent"),
    (EntryField::RstRcvd, "field.rst_rcvd"),
    (EntryField::Number, "field.number"),
    (EntryField::Memo, "field.memo"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Log,
    Space,
    Minutes(i32),
    Band(bool),
    Mode(bool),
    Accept,
    Clear,
    Go(EntryField),
}

pub fn bindings() -> Vec<(KeyboardShortcut, Action)> {
    let k = KeyboardShortcut::new;
    let back = Modifiers::CTRL | Modifiers::SHIFT;
    vec![
        (k(Modifiers::NONE, Key::Enter), Action::Log),
        (k(Modifiers::NONE, Key::Space), Action::Space),
        (k(Modifiers::NONE, Key::Plus), Action::Minutes(1)),
        (k(Modifiers::CTRL, Key::ArrowUp), Action::Minutes(1)),
        (k(Modifiers::CTRL, Key::ArrowDown), Action::Minutes(-1)),
        (k(back, Key::B), Action::Band(true)),
        (k(Modifiers::SHIFT, Key::B), Action::Band(false)),
        (k(back, Key::M), Action::Mode(true)),
        (k(Modifiers::ALT, Key::M), Action::Mode(false)),
        (k(Modifiers::ALT, Key::A), Action::Accept),
        (k(Modifiers::CTRL, Key::K), Action::Clear),
        (k(Modifiers::ALT, Key::D), Action::Go(EntryField::Date)),
        (k(Modifiers::ALT, Key::C), Action::Go(EntryField::Call)),
    ]
}

pub fn acts(action: Action, field: Option<EntryField>) -> bool {
    match action {
        Action::Space => EnterFlow::space_acts(field),
        Action::Minutes(_) => field != Some(EntryField::Memo),
        Action::Log | Action::Accept => field.is_some(),
        _ => true,
    }
}

pub fn paper_time(
    last: Option<DelphiTime>,
    date: &str,
    time: &str,
    now: DelphiTime,
) -> Option<DelphiTime> {
    let time = time.trim();
    let minutes = (1..=2).contains(&time.len()) && time.bytes().all(|b| b.is_ascii_digit());
    match last {
        Some(last) if minutes => {
            let m: i32 = time.parse().ok().filter(|m| *m < 60)?;
            let (_, _, _, h, mi, _) = last.to_ymd_hms();
            let base = zlog_ui::entry_time(date, &format!("{h:02}:{mi:02}"), now)?;
            let ahead = (m - mi as i32).rem_euclid(60);
            Some(add_minutes(base, ahead))
        }
        _ => zlog_ui::entry_time(date, time, now),
    }
}

pub fn add_minutes(t: DelphiTime, minutes: i32) -> DelphiTime {
    let (y, mo, d, h, mi, s) = t.to_ymd_hms();
    let start = DelphiTime::from_ymd_hms(y, mo, d, 0, 0, 0).0;
    let secs = (h * 3600 + mi * 60 + s) as i64 + minutes as i64 * 60;
    DelphiTime(start + secs as f64 / 86400.0)
}

pub fn fields_of(t: DelphiTime) -> (String, String) {
    let (y, mo, d, h, mi, _) = t.to_ymd_hms();
    (format!("{y:04}-{mo:02}-{d:02}"), format!("{h:02}:{mi:02}"))
}

fn cycle<T: PartialEq + Copy>(all: &[T], now: T, back: bool) -> T {
    let i = all.iter().position(|x| *x == now).unwrap_or(0);
    let n = all.len();
    all[if back { (i + n - 1) % n } else { (i + 1) % n }]
}

fn id(field: EntryField) -> Id {
    Id::unique(("entry_paper", field as u8))
}

#[derive(Default)]
pub struct PaperRow {
    focus: Option<EntryField>,
    select_all: Option<EntryField>,
    flow: EnterFlow,
    last: Option<DelphiTime>,
    logged: Option<String>,
}

impl EntryStyle for PaperRow {
    fn id(&self) -> &str {
        "paper"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn reserved_keys(&self) -> Vec<KeyboardShortcut> {
        bindings().into_iter().map(|(k, _)| k).collect()
    }

    fn keys(&mut self, ctx: &egui::Context, entry: &mut dyn EntryState) {
        let field = self.focused(ctx);
        for (key, action) in bindings() {
            if acts(action, field) && shortcut(ctx, key) {
                self.act(action, field, entry);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, entry: &mut dyn EntryState) {
        let lang = entry.language().to_string();
        let t = texts();
        let widths = widths(ui);
        let band_w = 64.0;
        ui.horizontal(|ui| {
            for ((_, key), w) in FIELDS.iter().zip(widths) {
                fixed(ui, w, RichText::new(t.get(&lang, key)).small());
            }
            fixed(
                ui,
                band_w,
                RichText::new(t.get(&lang, "field.band")).small(),
            );
            fixed(
                ui,
                band_w,
                RichText::new(t.get(&lang, "field.mode")).small(),
            );
        });
        ui.horizontal(|ui| {
            for ((field, _), w) in FIELDS.iter().zip(widths) {
                self.edit(ui, w, entry, *field);
            }
            let h = ui.spacing().interact_size.y;
            if ui
                .add_sized([band_w, h], egui::Button::new(entry.band().label()))
                .clicked()
            {
                self.act(Action::Band(false), None, entry);
            }
            if ui
                .add_sized([band_w, h], egui::Button::new(entry.mode().label()))
                .clicked()
            {
                self.act(Action::Mode(false), None, entry);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if let Some(l) = &self.logged {
                ui.label(RichText::new(l).strong());
                ui.separator();
            }
            ui.label(
                RichText::new(t.get(&lang, "help"))
                    .small()
                    .color(zlog_ui::palette::weak(ui)),
            );
        });
    }

    fn focused(&self, ctx: &egui::Context) -> Option<EntryField> {
        FIELDS
            .into_iter()
            .map(|(f, _)| f)
            .find(|f| ctx.memory(|m| m.has_focus(id(*f))))
    }

    fn focus(&mut self, field: EntryField) {
        self.focus = Some(field);
    }

    fn entry_changed(&mut self) {
        self.flow.new_qso();
    }
}

fn widths(ui: &egui::Ui) -> [f32; 7] {
    let fixed = [96.0, 56.0, 0.0, 44.0, 44.0, 0.0, 0.0];
    let gaps = ui.spacing().item_spacing.x * 9.0;
    let free = (ui.available_width() - fixed.iter().sum::<f32>() - 2.0 * 64.0 - gaps).max(240.0);
    let mut w = fixed;
    w[2] = free * 0.3;
    w[5] = free * 0.3;
    w[6] = free * 0.4;
    w
}

impl PaperRow {
    fn go(&mut self, cursor: Option<Cursor>) {
        if let Some(c) = cursor {
            self.focus = Some(c.field);
            if c.select_all {
                self.select_all = Some(c.field);
            }
        }
    }

    fn act(&mut self, action: Action, field: Option<EntryField>, entry: &mut dyn EntryState) {
        let c = match action {
            Action::Log => field.map(|f| self.log(entry, f)),
            Action::Space => self.flow.space(entry, field),
            Action::Minutes(n) => {
                let typed = self.typed_time(entry).or(self.last);
                let t = add_minutes(typed.unwrap_or_else(|| entry.now()), n);
                set_time(entry, t);
                None
            }
            Action::Band(back) => {
                entry.set_band(cycle(Band::ALL, entry.band(), back));
                None
            }
            Action::Mode(back) => {
                entry.set_mode(cycle(Mode::ALL, entry.mode(), back));
                None
            }
            Action::Accept => {
                accept_suggestions(entry, field);
                None
            }
            Action::Clear => Some(self.flow.clear(entry)),
            Action::Go(f) => Some(Cursor {
                field: f,
                select_all: matches!(f, EntryField::Date | EntryField::Time),
            }),
        };
        self.go(c);
    }

    fn typed_time(&self, entry: &dyn EntryState) -> Option<DelphiTime> {
        let (date, time) = (entry.text(EntryField::Date), entry.text(EntryField::Time));
        paper_time(self.last, date, time, entry.now())
    }

    fn log(&mut self, entry: &mut dyn EntryState, field: EntryField) -> Cursor {
        let at = self.typed_time(entry);
        let what = [
            entry.text(EntryField::Call).trim().to_ascii_uppercase(),
            entry.band().label().to_string(),
            entry.mode().label().to_string(),
            entry.text(EntryField::Number).trim().to_string(),
        ];
        let options = LogOptions {
            time: at,
            period: Period::Warn,
        };
        match entry.log_with(options) {
            Ok(()) => {
                let t = at.unwrap_or_else(|| entry.now());
                self.last = Some(t);
                set_time(entry, t);
                self.flow.new_qso();
                let (date, time) = fields_of(t);
                let n = entry.qso_count();
                let [call, band, mode, number] = &what;
                let when = format!("{date} {time}");
                let args: [&dyn std::fmt::Display; 6] = [&n, call, &when, band, mode, number];
                self.logged = Some(texts().format(entry.language(), "logged", &args));
                Cursor::to(EntryField::Call)
            }
            Err(why) => not_logged(why, field),
        }
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
        if response.gained_focus() && matches!(field, EntryField::Date | EntryField::Time) {
            self.select_all = Some(field);
        }
        if response.gained_focus() {
            entry.leave_log();
        }
    }
}

fn set_time(entry: &mut dyn EntryState, t: DelphiTime) {
    let (date, time) = fields_of(t);
    entry.set_text(EntryField::Date, date);
    entry.set_text(EntryField::Time, time);
}

fn fixed(ui: &mut egui::Ui, width: f32, text: RichText) {
    ui.add_sized([width, 12.0], egui::Label::new(text));
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
    b.entry_style(30, PaperRow::default);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DelphiTime {
        DelphiTime::from_ymd_hms(y, mo, d, h, mi, 0)
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn two_digits_are_the_minutes_running_on_over_midnight() {
        let now = at(2026, 10, 24, 12, 0);
        let last = Some(at(2026, 6, 6, 23, 58));
        let date = "2026-06-06";
        assert_eq!(
            paper_time(last, date, "59", now),
            Some(at(2026, 6, 6, 23, 59))
        );
        assert_eq!(
            paper_time(last, date, "02", now),
            Some(at(2026, 6, 7, 0, 2))
        );
        assert_eq!(paper_time(last, date, "58", now), last, "the same minute");
        assert_eq!(paper_time(last, date, "60", now), None);
        assert_eq!(
            paper_time(last, date, "21:05", now),
            Some(at(2026, 6, 6, 21, 5))
        );
        assert_eq!(
            paper_time(None, date, "2105", now),
            Some(at(2026, 6, 6, 21, 5))
        );
        assert_eq!(paper_time(None, "", "", now), Some(now));
    }

    #[test]
    fn a_minute_forward_crosses_midnight_and_back() {
        let t = add_minutes(at(2026, 6, 6, 23, 59), 1);
        assert_eq!(fields_of(t), ("2026-06-07".into(), "00:00".into()));
        assert_eq!(add_minutes(t, -1), at(2026, 6, 6, 23, 59));
    }

    #[test]
    fn keys_most_specific_first() {
        let b = bindings();
        let pos = |a| b.iter().position(|(_, x)| *x == a).unwrap();
        assert!(pos(Action::Band(true)) < pos(Action::Band(false)));
        assert!(pos(Action::Mode(true)) < pos(Action::Mode(false)));
        let back = Modifiers::CTRL | Modifiers::SHIFT;
        assert_eq!(
            b[pos(Action::Band(true))].0,
            KeyboardShortcut::new(back, Key::B)
        );
        assert_eq!(
            b[pos(Action::Mode(true))].0,
            KeyboardShortcut::new(back, Key::M)
        );
        assert!(
            !b.iter()
                .any(|(k, _)| k.modifiers.alt && [Key::B, Key::T].contains(&k.logical_key)),
            "Alt+B is backup and Alt+T the rig window, as in zLog"
        );
        assert_eq!(PaperRow::default().reserved_keys().len(), b.len());
        assert!(
            !acts(Action::Minutes(1), Some(EntryField::Memo)),
            "+ in the memo"
        );
        assert!(acts(Action::Minutes(1), Some(EntryField::Call)));
        assert!(!acts(Action::Log, None));
        assert!(!acts(Action::Space, Some(EntryField::Memo)));
    }

    #[test]
    fn bands_and_modes_go_round() {
        let first = Band::ALL[0];
        let last = *Band::ALL.last().unwrap();
        assert_eq!(cycle(Band::ALL, last, false), first);
        assert_eq!(cycle(Band::ALL, first, true), last);
        assert_eq!(cycle(Mode::ALL, Mode::ALL[0], false), Mode::ALL[1]);
    }
}
