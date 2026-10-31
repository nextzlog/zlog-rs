use super::*;

pub fn nothing_typed(e: &crate::entry::Entry) -> bool {
    e.callsign.is_empty() && e.nr_rcvd.is_empty() && e.memo.is_empty()
}

pub fn row_command(
    ctx: &egui::Context,
    all: &[commands::Listed],
    rows: bool,
    empty_entry: bool,
    activate: bool,
) -> Option<Result<(commands::Source, String), String>> {
    if !rows && !empty_entry {
        return None;
    }
    let enter = egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Enter);
    let activated = activate
        .then(|| commands::with_row_key(all, &enter))
        .flatten();
    let (source, c) = activated.or_else(|| commands::row_key_pressed(ctx, all, !rows))?;
    match (&c.disabled_reason, c.enabled) {
        (_, true) => Some(Ok((source, c.id.clone()))),
        (Some(why), false) => Some(Err(why.clone())),
        (None, false) => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn route_keys(
    ctx: &egui::Context,
    session: &mut Session,
    view: &mut View,
    mut entry: Option<&mut (dyn EntryStyle + 'static)>,
    panels: &mut [PanelSlot],
    options: &Options,
    spots: &[zlog_ui::Spot],
    rig: RigNow,
) {
    let to_call = pressed(ctx, egui::Modifiers::NONE, egui::Key::Escape) && view.selected.is_none();
    if keys_for_panels(ctx, view, entry.as_deref(), panels, options) {
        if let Some(e) = entry.as_deref_mut().filter(|_| to_call) {
            e.focus(EntryField::Call);
        }
        let pv = panel_view(
            focus(ctx, entry.as_deref()),
            editing(panels),
            view.selected,
            session,
            spots,
            rig,
        );
        for n in 1..=12u8 {
            let Some(key) = egui::Key::from_name(&format!("F{n}")) else {
                continue;
            };
            for (modifiers, shift) in [
                (egui::Modifiers::NONE, false),
                (egui::Modifiers::SHIFT, true),
            ] {
                let key_of_panels = zlog_ui::EntryKey::Function { n, shift };
                if pressed(ctx, modifiers, key)
                    && panels
                        .iter_mut()
                        .any(|s| s.panel.on_key(key_of_panels, &pv))
                {
                    ctx.input_mut(|i| zlog_ui::keys::consume_exact(i, modifiers, key));
                }
            }
        }
        if pressed(ctx, egui::Modifiers::NONE, egui::Key::Escape)
            && panels.iter_mut().fold(false, |took, s| {
                s.panel.on_key(zlog_ui::EntryKey::Escape, &pv) | took
            })
        {
            ctx.input_mut(|i| {
                zlog_ui::keys::consume_exact(i, egui::Modifiers::NONE, egui::Key::Escape)
            });
        }
    }
    let hz = rig.hz;
    let focused = focus(ctx, entry.as_deref());
    main_window::handle_keys(ctx, session, view, entry.as_deref_mut(), modal(panels), hz);
    if let Some(played) = send_messages(view, session, panels, focused, spots, rig) {
        if let Some(style) = entry {
            style.messages_sent(&mut crate::row::Row { session, view, hz });
        }
        if !played {
            session.status = Status::warning(tr("status.not_sent"));
        }
        ctx.request_repaint();
    }
}

pub fn send_messages(
    view: &mut View,
    session: &Session,
    panels: &mut [PanelSlot],
    focused: Option<EntryField>,
    spots: &[zlog_ui::Spot],
    rig: RigNow,
) -> Option<bool> {
    let sends = std::mem::take(&mut view.sends);
    if sends.is_empty() {
        return None;
    }
    let pv = panel_view(focused, editing(panels), view.selected, session, spots, rig);
    let mut played = true;
    for n in sends {
        let key = zlog_ui::EntryKey::Reserved { n };
        played &= panels.iter_mut().any(|s| s.panel.on_key(key, &pv));
    }
    Some(played)
}

pub fn no_text_to_undo(in_entry: bool, empty: bool, text_focused: bool, options: bool) -> bool {
    (in_entry && empty) || (!text_focused && !options)
}

pub(super) fn keys_for_panels(
    ctx: &egui::Context,
    view: &View,
    entry: Option<&dyn EntryStyle>,
    panels: &[PanelSlot],
    options: &Options,
) -> bool {
    let modal = modal(panels) || view.start_time.is_some() || options.open;
    let other_text = ctx.memory(|m| m.focused()).is_some() && focus(ctx, entry).is_none();
    !modal && !other_text
}

pub(super) fn pressed(ctx: &egui::Context, modifiers: egui::Modifiers, key: egui::Key) -> bool {
    ctx.input(|i| {
        i.events.iter().any(|e| {
            matches!(e, egui::Event::Key { key: k, pressed: true, modifiers: m, .. }
                if *k == key && m.matches_exact(modifiers))
        })
    })
}

pub(super) fn focus(ctx: &egui::Context, entry: Option<&dyn EntryStyle>) -> Option<EntryField> {
    entry.and_then(|e| e.focused(ctx))
}

pub(super) fn editing(panels: &[PanelSlot]) -> Option<zlog_core::QsoId> {
    panels.iter().find_map(|s| s.panel.editing())
}

pub(super) fn modal(panels: &[PanelSlot]) -> bool {
    panels.iter().any(|s| s.panel.modal())
}

const REFOCUS_PASSES: u8 = 3;

#[derive(Default)]
pub struct EntryFocus {
    last: Option<EntryField>,
    seen: Option<EntryField>,
    restore: Option<(EntryField, u8)>,
    blocked: bool,
}

impl EntryFocus {
    pub fn want(&mut self, field: EntryField) {
        self.restore = Some((field, REFOCUS_PASSES));
    }

    pub fn moved(&mut self) {
        if self.restore.is_none() {
            self.want(self.last.or(self.seen).unwrap_or(EntryField::Call));
        }
    }

    pub fn after_frame(
        &mut self,
        ctx: &egui::Context,
        entry: Option<&mut (dyn EntryStyle + 'static)>,
        blocked: bool,
        selected: bool,
    ) {
        let Some(entry) = entry else {
            return;
        };
        if blocked {
            if !self.blocked {
                self.moved();
            }
            self.blocked = true;
            return;
        }
        self.blocked = false;
        let now = entry.focused(ctx);
        let other = now.is_none() && ctx.memory(|m| m.focused()).is_some();
        if now.is_some() || selected {
            self.restore = None;
        } else if let Some((field, left)) = self.restore {
            self.restore = left.checked_sub(1).map(|l| (field, l));
            if !other {
                entry.focus(field);
                ctx.request_repaint();
            }
        }
        if now.is_some() {
            self.last = now;
            self.seen = now;
        } else if other || selected {
            self.last = None;
        }
    }
}
