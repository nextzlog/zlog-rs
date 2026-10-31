use egui::{Context, RichText, Ui};
use zlog_tls::discovery::{Browser, Found};
use zlog_tls::{Peer, PeerId, Peers};

use crate::texts;

pub fn toggle_search(browser: &mut Option<Browser>, app: &str) {
    *browser = match browser.take() {
        Some(_) => None,
        None => Browser::start(app).ok(),
    };
}

pub fn search_button(ui: &mut Ui, browser: &mut Option<Browser>, app: &str, lang: &str) {
    let t = |k| texts().get(lang, k);
    let label = if browser.is_some() {
        "search.stop"
    } else {
        "search"
    };
    if ui.button(t(label)).on_hover_text(t("search.tip")).clicked() {
        toggle_search(browser, app);
    }
}

pub fn search_label(lang: &str) -> String {
    texts().get(lang, "search").into()
}

pub fn search_tip(lang: &str) -> String {
    texts().get(lang, "search.tip").into()
}

pub fn sorted_found(found: Vec<Found>, recent: &[String]) -> Vec<(Found, String)> {
    let mut list: Vec<(Found, String)> = found
        .into_iter()
        .filter_map(|f| {
            let at = f.endpoints().first().copied()?;
            Some((f, at.to_string()))
        })
        .collect();
    let rank = |a: &str| recent.iter().position(|r| r == a).unwrap_or(usize::MAX);
    list.sort_by(|(fa, a), (fb, b)| rank(a).cmp(&rank(b)).then(fa.name.cmp(&fb.name)));
    list
}

pub fn found_text(f: &Found, lang: &str) -> String {
    let t = texts();
    let name = f.get("name").unwrap_or(&f.name);
    let mut parts = vec![name.to_string()];
    if let Some(c) = f.get("call").filter(|c| !c.is_empty()) {
        parts.push(c.into());
    }
    if f.get("tls") == Some("1") {
        parts.push("TLS".into());
    }
    if let Some(sh) = f.get("shares").filter(|s| !s.is_empty()) {
        let items: Vec<String> = sh
            .split(',')
            .map(|k| t.get(lang, &format!("shares.{k}")).to_string())
            .collect();
        parts.push(items.join("/"));
    }
    match f.get("busy") {
        Some("1") => parts.push(t.get(lang, "found.busy").into()),
        Some(_) => parts.push(t.get(lang, "found.free").into()),
        None => {}
    }
    parts.join("  ")
}

pub struct FoundRow {
    pub found: Found,
    pub at: String,
    pub warning: Option<String>,
}

pub fn found_list(ui: &mut Ui, rows: &[FoundRow], lang: &str) -> Option<usize> {
    let t = |k| texts().get(lang, k);
    if rows.is_empty() {
        ui.weak(t("search.none"));
    }
    let mut chosen = None;
    for (i, r) in rows.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(t("search.use"))
                .on_hover_text(t("search.use.tip"))
                .clicked()
            {
                chosen = Some(i);
            }
            let text = found_text(&r.found, lang);
            let label = match &r.warning {
                Some(w) => RichText::new(format!("{text}  ({w})")).color(crate::palette::error(ui)),
                None => RichText::new(text),
            };
            ui.label(label).on_hover_text(&r.at);
        });
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(500));
    chosen
}

pub fn peer_label(p: &Peer) -> (String, String) {
    match p.id {
        PeerId::Cert(fp) => {
            let short: String = fp.to_string().chars().take(11).collect();
            (format!("{} {short}", p.name), fp.to_string())
        }
        PeerId::Ip(ip) => (format!("{} {ip}", p.name), ip.to_string()),
    }
}

pub fn peer_list(ui: &mut Ui, peers: &Peers, lang: &str) {
    let t = |k| texts().get(lang, k);
    let list = peers.list();
    if list.is_empty() {
        ui.weak(t("peers.none"));
    }
    for p in list {
        let (text, tip) = peer_label(&p);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(t("peers.remove"))
                .on_hover_text(t("peers.remove.tip"))
                .clicked()
            {
                peers.remove(&p.id);
            }
            ui.label(text).on_hover_text(tip);
        });
    }
}

fn window(ctx: &Context, key: &str, lang: &str, body: impl FnOnce(&mut Ui)) {
    egui::Window::new(texts().get(lang, key))
        .collapsible(false)
        .resizable(false)
        .show(ctx, body);
}

fn yes_no(ui: &mut Ui, lang: &str, yes: &str, no: &str) -> Option<bool> {
    let t = |k| texts().get(lang, k);
    let mut answer = None;
    ui.horizontal(|ui| {
        if ui
            .button(t(yes))
            .on_hover_text(texts().get(lang, &format!("{yes}.tip")).to_string())
            .clicked()
        {
            answer = Some(true);
        }
        if ui.button(t(no)).clicked() {
            answer = Some(false);
        }
    });
    answer
}

pub fn code_window(ctx: &Context, code: &str, lang: &str) {
    window(ctx, "pair.title", lang, |ui| {
        ui.heading(code);
        ui.label(texts().get(lang, "pair.check"));
    });
}

pub fn repair_window(ctx: &Context, addr: &str, lang: &str) -> Option<bool> {
    let mut answer = None;
    window(ctx, "repair.title", lang, |ui| {
        ui.label(texts().format(lang, "repair.ask", &[&addr]));
        answer = yes_no(ui, lang, "repair.yes", "repair.no");
    });
    answer
}

pub fn approve_window(
    ctx: &Context,
    name: &str,
    addr: &str,
    code: Option<&str>,
    plain: bool,
    lang: &str,
) -> Option<bool> {
    let t = |k| texts().get(lang, k);
    let mut answer = None;
    window(ctx, "approve.title", lang, |ui| {
        ui.label(texts().format(lang, "approve.who", &[&name, &addr]));
        if let Some(code) = code {
            ui.heading(code);
            ui.label(t("approve.check"));
        }
        if plain {
            ui.label(t("approve.plain"));
        }
        answer = yes_no(ui, lang, "approve.yes", "approve.no");
    });
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(name: &str, ip: &str, txt: &[(&str, &str)]) -> Found {
        Found {
            name: name.into(),
            addrs: vec![ip.parse().unwrap()],
            port: 23,
            txt: txt
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn found_stations_list_the_last_used_first_and_read_alike() {
        let list = sorted_found(
            vec![
                found("A", "10.0.0.1", &[]),
                found("B", "10.0.0.2", &[]),
                found("C", "10.0.0.3", &[]),
            ],
            &["10.0.0.3:23".into(), "10.0.0.2:23".into()],
        );
        let names: Vec<_> = list.iter().map(|(f, _)| f.name.as_str()).collect();
        assert_eq!(names, ["C", "B", "A"]);
        let f = found(
            "x",
            "10.0.0.5",
            &[
                ("name", "Home"),
                ("call", "JA1ZLO"),
                ("tls", "1"),
                ("shares", "rig1,keyer"),
                ("busy", "0"),
            ],
        );
        assert_eq!(found_text(&f, "en"), "Home  JA1ZLO  TLS  Rig 1/keyer  free");
        assert_eq!(found_text(&found("y", "10.0.0.6", &[]), "en"), "y");
    }

    #[test]
    fn the_pairing_windows_answer() {
        use egui_kittest::kittest::Queryable;
        let mut h = egui_kittest::Harness::builder()
            .with_size([400.0, 300.0])
            .build_ui_state(
                |ui, answer: &mut Option<bool>| {
                    let ctx = &ui.ctx().clone();
                    if let Some(a) =
                        approve_window(ctx, "PC2", "10.0.0.2", Some("1234"), false, "en")
                    {
                        *answer = Some(a);
                    }
                },
                None,
            );
        h.run();
        assert!(h.query_by_label("1234").is_some());
        h.get_by_label("Approve").click();
        h.run();
        assert_eq!(*h.state(), Some(true));
    }
}
