use egui::{Color32, RichText, Ui};

use crate::{icon, texts};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortFault {
    Busy,
    Denied,
    Missing,
    NoResponse,
    Other,
}

impl PortFault {
    pub fn of(raw: &str) -> Self {
        let e = raw.to_ascii_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| e.contains(w));
        if has(&[
            "busy",
            "os error 16",
            "access is denied",
            "already in use",
            "in use",
            "exclusive",
        ]) {
            PortFault::Busy
        } else if has(&["permission denied", "os error 13", "not permitted"]) {
            PortFault::Denied
        } else if has(&[
            "no such file",
            "os error 2)",
            "cannot find",
            "not found",
            "notfound",
            "nodevice",
            "no such device",
            "os error 19",
            "brokenpipe",
            "notconnected",
        ]) {
            PortFault::Missing
        } else if has(&["noreply", "timedout", "timed out"]) {
            PortFault::NoResponse
        } else {
            PortFault::Other
        }
    }

    pub fn retry(self) -> bool {
        matches!(self, PortFault::Busy | PortFault::Missing)
    }

    fn key(self) -> &'static str {
        match self {
            PortFault::Busy => "fault.busy",
            PortFault::Denied => "fault.denied",
            PortFault::Missing => "fault.missing",
            PortFault::NoResponse => "fault.no_response",
            PortFault::Other => "fault.other",
        }
    }
}

pub fn text(language: &str, key: &str) -> String {
    texts().get(language, key).to_string()
}

pub fn explain(language: &str, raw: &str) -> String {
    let fault = PortFault::of(raw);
    let t = texts();
    match fault {
        PortFault::Other => t.format(language, fault.key(), &[&raw]),
        _ => t.get(language, fault.key()).to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetFault {
    Refused,
    TimedOut,
    NoHost,
    Offline,
    Secure,
    Closed,
    Other,
}

impl NetFault {
    pub fn of(raw: &str) -> Self {
        let e = raw.to_ascii_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| e.contains(w));
        if has(&["refused", "os error 111", "10061"]) {
            NetFault::Refused
        } else if has(&["timed out", "timedout", "os error 110", "10060"]) {
            NetFault::TimedOut
        } else if has(&[
            "lookup",
            "not known",
            "no such host",
            "11001",
            "resolve",
            "invalid address",
        ]) {
            NetFault::NoHost
        } else if has(&["offline", "unreachable", "os error 101", "10051"]) {
            NetFault::Offline
        } else if has(&["tls", "certificate", "handshake", "auth", "pair"]) {
            NetFault::Secure
        } else if has(&["closed", "reset", "os error 104", "10054", "broken pipe"]) {
            NetFault::Closed
        } else {
            NetFault::Other
        }
    }

    fn key(self) -> &'static str {
        match self {
            NetFault::Refused => "net.refused",
            NetFault::TimedOut => "net.timed_out",
            NetFault::NoHost => "net.no_host",
            NetFault::Offline => "net.offline",
            NetFault::Secure => "net.secure",
            NetFault::Closed => "net.closed",
            NetFault::Other => "fault.other",
        }
    }
}

pub fn explain_net(language: &str, raw: &str) -> String {
    let fault = NetFault::of(raw);
    let t = texts();
    match fault {
        NetFault::Other => t.format(language, fault.key(), &[&raw]),
        _ => t.get(language, fault.key()).to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkState {
    Connected,
    Off,
    Connecting,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub key: String,
    pub label: String,
    pub state: LinkState,
    pub error: Option<String>,
    pub page: Option<String>,
    pub connect: Option<String>,
    pub disconnect: Option<String>,
}

impl Connection {
    pub fn new(key: &str, label: impl Into<String>, state: LinkState) -> Self {
        Connection {
            key: key.into(),
            label: label.into(),
            state,
            error: None,
            page: None,
            connect: None,
            disconnect: None,
        }
    }

    pub fn error(mut self, error: Option<String>) -> Self {
        self.error = error.filter(|e| !e.is_empty());
        self
    }

    pub fn page(mut self, page: &str) -> Self {
        self.page = Some(page.into());
        self
    }

    pub fn commands(mut self, connect: &str, disconnect: &str) -> Self {
        self.connect = Some(connect.into());
        self.disconnect = Some(disconnect.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionPick {
    Run(String),
    Settings(String),
    Stop,
}

pub fn glyph(state: LinkState) -> (&'static str, Color32) {
    match state {
        LinkState::Connected => (icon::PLUGS_CONNECTED, Color32::from_rgb(0x2e, 0x9e, 0x4f)),
        LinkState::Off => (icon::PLUGS, Color32::GRAY),
        LinkState::Connecting => (icon::HOURGLASS, Color32::from_rgb(0xd0, 0x8c, 0x00)),
        LinkState::Failed => (icon::WARNING, Color32::from_rgb(0xd0, 0x30, 0x30)),
    }
}

fn state_key(state: LinkState) -> &'static str {
    match state {
        LinkState::Connected => "connection.connected",
        LinkState::Off => "connection.off",
        LinkState::Connecting => "connection.connecting",
        LinkState::Failed => "connection.failed",
    }
}

pub fn worst(items: &[Connection]) -> LinkState {
    items
        .iter()
        .map(|c| c.state)
        .max()
        .unwrap_or(LinkState::Off)
}

const ICON_WIDTH: f32 = 28.0;

const WIDE: f32 = 420.0;

pub fn collapsed(available: f32, count: usize) -> bool {
    count > 1 && available < WIDE
}

pub fn width(count: usize, collapsed: bool) -> f32 {
    ICON_WIDTH * if collapsed { 1.0 } else { count as f32 }
}

pub fn indicator(
    ui: &mut Ui,
    items: &[(usize, Connection)],
    language: &str,
    collapsed: bool,
) -> Option<(usize, ConnectionPick)> {
    if items.is_empty() {
        return None;
    }
    let t = texts();
    let mut chosen = None;
    let groups: Vec<Vec<&(usize, Connection)>> = if collapsed {
        vec![items.iter().collect()]
    } else {
        items.iter().rev().map(|i| vec![i]).collect()
    };
    for group in groups {
        let shown: Vec<Connection> = group.iter().map(|(_, c)| c.clone()).collect();
        let state = worst(&shown);
        let (g, color) = glyph(state);
        let name = match group.as_slice() {
            [(_, c)] => c.label.clone(),
            _ => t.get(language, "connection.all").to_string(),
        };
        let summary = shown
            .iter()
            .map(|c| format!("{}: {}", c.label, t.get(language, state_key(c.state))))
            .collect::<Vec<_>>()
            .join("\n");
        let button = egui::Button::new(RichText::new(g).family(icon::family()).color(color));
        let (r, picked) = egui::containers::menu::MenuButton::from_button(button)
            .ui(ui, |ui| popover(ui, &group, language));
        r.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Button,
                true,
                format!("{name}: {}", t.get(language, state_key(state))),
            )
        });
        if picked.is_none() {
            r.on_hover_text(summary);
        }
        if let Some(p) = picked.and_then(|i| i.inner) {
            chosen = Some(p);
        }
    }
    chosen
}

fn popover(
    ui: &mut Ui,
    group: &[&(usize, Connection)],
    language: &str,
) -> Option<(usize, ConnectionPick)> {
    let t = texts();
    let mut chosen = None;
    ui.set_max_width(260.0);
    for (n, (slot, c)) in group.iter().enumerate() {
        if n > 0 {
            ui.separator();
        }
        let (g, color) = glyph(c.state);
        ui.horizontal(|ui| {
            ui.label(RichText::new(g).family(icon::family()).color(color));
            ui.strong(&c.label);
            ui.label(t.get(language, state_key(c.state)));
        });
        if let Some(e) = &c.error {
            ui.add(egui::Label::new(RichText::new(e).color(ui.visuals().warn_fg_color)).wrap());
        }
        ui.horizontal_wrapped(|ui| {
            let mut tool = |glyph: &str, key: &str, pick: ConnectionPick| {
                let tip_key = format!("{key}.tip");
                let tip = t.get(language, &tip_key).to_string();
                let label = t.get(language, key);
                let r = ui
                    .button((
                        RichText::new(glyph).family(icon::family()),
                        RichText::new(label),
                    ))
                    .on_hover_text(tip);
                if r.clicked() {
                    chosen = Some((*slot, pick));
                    ui.close();
                }
            };
            if let Some(id) = &c.connect {
                tool(
                    icon::ARROW_CLOCKWISE,
                    "connection.reconnect",
                    ConnectionPick::Run(id.clone()),
                );
            }
            if let (Some(id), true) = (&c.disconnect, c.state != LinkState::Off) {
                tool(
                    icon::PLUGS_CONNECTED,
                    "connection.disconnect",
                    ConnectionPick::Run(id.clone()),
                );
            }
            if let Some(page) = &c.page {
                tool(
                    icon::GEAR,
                    "connection.settings",
                    ConnectionPick::Settings(page.clone()),
                );
            }
        });
    }
    ui.separator();
    let stop = ui
        .button((
            RichText::new(icon::STOP).family(icon::family()),
            RichText::new(t.get(language, "connection.stop")),
        ))
        .on_hover_text(t.get(language, "connection.stop.tip"));
    if stop.clicked() {
        chosen = group.first().map(|(slot, _)| (*slot, ConnectionPick::Stop));
        ui.close();
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_errors_are_classified() {
        let cases = [
            ("Device or resource busy", PortFault::Busy),
            ("Access is denied.", PortFault::Busy),
            ("Permission denied (os error 13)", PortFault::Denied),
            ("No such file or directory (os error 2)", PortFault::Missing),
            (
                "The system cannot find the file specified.",
                PortFault::Missing,
            ),
            ("NoReply", PortFault::NoResponse),
            ("TimedOut", PortFault::NoResponse),
            ("BrokenPipe", PortFault::Missing),
            ("something odd", PortFault::Other),
        ];
        for (raw, want) in cases {
            assert_eq!(PortFault::of(raw), want, "{raw}");
        }
        assert!(PortFault::Busy.retry() && PortFault::Missing.retry());
        assert!(!PortFault::Denied.retry() && !PortFault::NoResponse.retry());
    }

    #[test]
    fn explanations_name_the_cause_and_the_next_step() {
        for lang in ["en", "ja"] {
            for raw in ["busy", "Permission denied", "NoDevice", "NoReply"] {
                let e = explain(lang, raw);
                assert!(!e.contains(raw), "{lang} {raw}: {e}");
                assert!(e.len() > 20, "{lang} {raw}: {e}");
            }
            assert!(explain(lang, "odd thing").contains("odd thing"));
        }
        assert!(explain("en", "Device or resource busy").contains("another program"));
    }

    #[test]
    fn network_errors_are_classified() {
        let cases = [
            ("Connection refused (os error 111)", NetFault::Refused),
            ("connection timed out", NetFault::TimedOut),
            ("failed to lookup address information", NetFault::NoHost),
            ("offline", NetFault::Offline),
            ("tls handshake failed", NetFault::Secure),
            ("closed", NetFault::Closed),
            ("weird", NetFault::Other),
        ];
        for (raw, want) in cases {
            assert_eq!(NetFault::of(raw), want, "{raw}");
        }
        for lang in ["en", "ja"] {
            assert!(explain_net(lang, "Connection refused").len() > 30);
            assert!(explain_net(lang, "weird").contains("weird"));
        }
    }

    fn items() -> Vec<(usize, Connection)> {
        vec![
            (
                3,
                Connection::new("rig", "Rig 1", LinkState::Failed)
                    .error(Some("busy".into()))
                    .page("rig")
                    .commands("rig1.connect", "rig1.disconnect"),
            ),
            (
                5,
                Connection::new("keyer", "CW keyer", LinkState::Connected)
                    .commands("keyer.connect", "keyer.disconnect"),
            ),
        ]
    }

    type Picked = (bool, Option<(usize, ConnectionPick)>);

    fn harness(width: f32) -> egui_kittest::Harness<'static, Picked> {
        let mut h = egui_kittest::Harness::builder()
            .with_size([width, 400.0])
            .build_ui_state(
                move |ui, (ready, picked): &mut Picked| {
                    if !*ready {
                        let mut fonts = egui::FontDefinitions::default();
                        crate::icon::add_font(&mut fonts);
                        ui.ctx().set_fonts(fonts);
                        *ready = true;
                        return;
                    }
                    let all = items();
                    let narrow = collapsed(ui.available_width(), all.len());
                    if let Some(p) = indicator(ui, &all, "en", narrow) {
                        *picked = Some(p);
                    }
                },
                (false, None),
            );
        h.run();
        h
    }

    #[test]
    fn one_icon_per_device_when_wide_and_one_when_narrow() {
        use egui_kittest::kittest::Queryable;
        let h = harness(800.0);
        assert!(h.query_by_label("Rig 1: Failed").is_some());
        assert!(h.query_by_label("CW keyer: Connected").is_some());
        let h = harness(280.0);
        assert!(h.query_by_label("Rig 1: Failed").is_none());
        assert!(h.query_by_label("Connections: Failed").is_some());
    }

    #[test]
    fn the_popover_offers_reconnect_disconnect_settings_and_stop() {
        use egui_kittest::kittest::Queryable;
        let picks = [
            ("Reconnect", ConnectionPick::Run("rig1.connect".into())),
            ("Disconnect", ConnectionPick::Run("rig1.disconnect".into())),
            ("Settings", ConnectionPick::Settings("rig".into())),
            ("Stop TX", ConnectionPick::Stop),
        ];
        for (label, want) in picks {
            let mut h = harness(800.0);
            h.get_by_label("Rig 1: Failed").click();
            h.run();
            assert!(h.query_by_label("busy").is_some(), "last error");
            h.get_by_label_contains(label).click();
            h.run();
            assert_eq!(h.state().1, Some((3, want)), "{label}");
        }
    }

    #[test]
    fn the_worst_state_wins_and_states_differ_in_shape_and_colour() {
        let c = |s| Connection::new("k", "K", s);
        assert_eq!(
            worst(&[c(LinkState::Connected), c(LinkState::Failed)]),
            LinkState::Failed
        );
        assert_eq!(worst(&[c(LinkState::Connected)]), LinkState::Connected);
        let all = [
            LinkState::Connected,
            LinkState::Off,
            LinkState::Connecting,
            LinkState::Failed,
        ];
        for a in all {
            for b in all.iter().filter(|b| **b != a) {
                assert_ne!(glyph(a).0, glyph(*b).0);
                assert_ne!(glyph(a).1, glyph(*b).1);
            }
        }
    }
}
