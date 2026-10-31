use egui::{RichText, Ui};
use zlog_core::Mode;
use zlog_net::{Exchange, Reply, Request};

use crate::form::Form;
use crate::{texts, Transport};

pub const UPLOAD_URL: &str = "https://contest.jarl.org/upload/";

pub const UPLOAD_URL_AA: &str = "https://contest.jarl.org/upload-aa/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contest {
    Allja,
    SixDown,
    FieldDay,
    Acag,
    AllAsianCw,
    AllAsianPhone,
    NewYearParty,
}

impl Contest {
    pub fn value(self) -> &'static str {
        match self {
            Contest::Allja => "allja",
            Contest::SixDown => "6d",
            Contest::FieldDay => "fd",
            Contest::Acag => "acag",
            Contest::AllAsianCw => "aacw",
            Contest::AllAsianPhone => "aaph",
            Contest::NewYearParty => "nyp",
        }
    }

    pub fn radio_id(self) -> String {
        format!("rb_{}", self.value())
    }

    pub fn url(self) -> &'static str {
        match self {
            Contest::AllAsianCw | Contest::AllAsianPhone => UPLOAD_URL_AA,
            _ => UPLOAD_URL,
        }
    }
}

pub fn contest_of(name: &str, first_mode: Option<Mode>) -> Option<Contest> {
    Some(match name {
        "ALL JA" => Contest::Allja,
        "6m AND DOWN" => Contest::SixDown,
        "Field Day" => Contest::FieldDay,
        "ACAG" => Contest::Acag,
        "NEW YEAR PARTY" => Contest::NewYearParty,
        n if n.starts_with("All Asian DX Contest") => match first_mode {
            Some(Mode::Cw) => Contest::AllAsianCw,
            _ => Contest::AllAsianPhone,
        },
        _ => return None,
    })
}

const GROUP: &str = "contest";

pub fn prepare(page: &Reply, url: &str, contest: Contest, elog: &str) -> Result<Form, String> {
    let forms = crate::form::forms(
        &page.text(),
        if page.url.is_empty() { url } else { &page.url },
    );
    let has_text = |f: &&Form| f.controls.iter().any(|c| c.name == "elogtext");
    let mut form = forms
        .iter()
        .filter(has_text)
        .find(|f| f.name == "form" || f.id == "form")
        .or_else(|| forms.iter().find(has_text))
        .cloned()
        .ok_or("no elogtext on the upload page")?;
    form.set("elogtext", elog);
    if !form.check(&contest.radio_id()) {
        let id = form
            .controls
            .iter()
            .find(|c| c.kind == "radio" && c.name == GROUP && c.value == contest.value())
            .map(|c| c.id.clone());
        let checked = id.is_some_and(|id| !id.is_empty() && form.check(&id));
        if !checked {
            for c in form
                .controls
                .iter_mut()
                .filter(|c| c.kind == "radio" && c.name == GROUP)
            {
                c.checked = c.value == contest.value() && !c.disabled;
            }
        }
    }
    if let Some(i) = form.controls.iter().position(|c| c.is_submit()) {
        form.pressed = Some(i);
    }
    if form.method.is_empty() {
        form.method = "post".into();
    }
    Ok(form)
}

pub fn reply_text(html: &str) -> String {
    let s = html
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");
    zlog_net::html_text(&s)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

enum Step {
    Page(Exchange),
    Ready { confirm: bool },
    Sending(Exchange),
    Sent(u16, String),
    Failed(String),
}

pub struct Upload {
    contest: Contest,
    elog: String,
    form: Option<Form>,
    page: Reply,
    step: Step,
    transport: Transport,
}

impl Upload {
    pub fn new(contest: Contest, elog: String) -> Self {
        Upload::with_transport(contest, elog, Box::new(Exchange::send))
    }

    pub fn with_transport(contest: Contest, elog: String, mut transport: Transport) -> Self {
        let step = Step::Page(transport(Request::get(contest.url())));
        Upload {
            contest,
            elog,
            form: None,
            page: Reply::default(),
            step,
            transport,
        }
    }

    fn request(&self) -> Option<Request> {
        let form = self.form.as_ref()?;
        form.checked(GROUP)?;
        Some(form.request(self.page.encoding(), &self.page.cookies))
    }

    pub fn poll(&mut self) {
        let e = match &mut self.step {
            Step::Page(e) | Step::Sending(e) => e,
            _ => return,
        };
        let Some(result) = e.poll().cloned() else {
            return;
        };
        let page = matches!(self.step, Step::Page(_));
        self.step = match (result, page) {
            (Err(e), _) => Step::Failed(e),
            (Ok(r), true) if r.status >= 400 => Step::Failed(format!("HTTP {}", r.status)),
            (Ok(r), true) => match prepare(&r, self.contest.url(), self.contest, &self.elog) {
                Ok(form) => {
                    self.form = Some(form);
                    self.page = r;
                    Step::Ready { confirm: false }
                }
                Err(e) => Step::Failed(e),
            },
            (Ok(r), false) => Step::Sent(r.status, reply_text(&r.text())),
        };
    }

    fn send(&mut self) {
        if let (Step::Ready { confirm: true }, Some(r)) = (&self.step, self.request()) {
            self.step = Step::Sending((self.transport)(r));
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, language: &str) {
        let t = |k| texts().get(language, k);
        self.poll();
        if matches!(self.step, Step::Page(_) | Step::Sending(_)) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        ui.horizontal(|ui| {
            ui.label(t("upload.url"));
            ui.monospace(self.contest.url());
        });
        ui.separator();
        match &self.step {
            Step::Page(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t("upload.loading"));
                });
                return;
            }
            Step::Sending(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t("upload.sending"));
                });
                return;
            }
            Step::Failed(e) => {
                ui.colored_label(
                    zlog_ui::palette::error(ui),
                    texts().format(language, "upload.failed", &[e]),
                );
                return;
            }
            Step::Sent(status, text) => {
                ui.label(texts().format(language, "upload.reply", &[status]));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut text.as_str()).desired_width(f32::INFINITY),
                    );
                });
                return;
            }
            Step::Ready { .. } => {}
        }
        let confirm = matches!(self.step, Step::Ready { confirm: true });
        let Some(form) = self.form.as_mut() else {
            return;
        };
        ui.add_enabled_ui(!confirm, |ui| {
            ui.label(t("upload.contest"));
            ui.horizontal_wrapped(|ui| {
                let radios: Vec<(String, String, bool, bool)> = form
                    .controls
                    .iter()
                    .filter(|c| c.kind == "radio" && c.name == GROUP)
                    .map(|c| (c.id.clone(), c.value.clone(), c.checked, c.disabled))
                    .collect();
                for (id, value, checked, disabled) in radios {
                    let r = ui.add_enabled(!disabled, egui::RadioButton::new(checked, &value));
                    if r.clicked() {
                        for c in form.controls.iter_mut() {
                            if c.kind == "radio" && c.name == GROUP {
                                c.checked = c.id == id && c.value == value;
                            }
                        }
                    }
                }
            });
            if form.checked(GROUP).is_none() {
                ui.colored_label(zlog_ui::palette::error(ui), t("upload.no_contest"));
            }
            let editable = |k: &str| matches!(k, "text" | "email" | "tel" | "number" | "password");
            let labels: Vec<String> = form
                .controls
                .iter()
                .filter(|c| editable(&c.kind) && !c.name.is_empty())
                .map(|c| c.name.clone())
                .collect();
            if !labels.is_empty() {
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                zlog_ui::form(ui, "jarl_upload_fields", &refs, |ui, fields| {
                    for c in form
                        .controls
                        .iter_mut()
                        .filter(|c| editable(&c.kind) && !c.name.is_empty())
                    {
                        ui.label(&c.name);
                        let edit =
                            egui::TextEdit::singleline(&mut c.value).password(c.kind == "password");
                        fields.text(ui, edit);
                        ui.end_row();
                    }
                });
            }
        });
        ui.separator();
        ui.label(RichText::new(t("upload.preview")).strong());
        ui.horizontal(|ui| {
            ui.label(t("upload.action"));
            ui.monospace(format!("{} {}", form.method.to_uppercase(), form.action));
        });
        let pairs = form.pairs();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("jarl_upload_pairs")
                .striped(true)
                .show(ui, |ui| {
                    for (name, value) in pairs.iter().filter(|(n, _)| n != "elogtext") {
                        ui.monospace(name);
                        let secret = form
                            .controls
                            .iter()
                            .any(|c| c.name == *name && c.kind == "password");
                        ui.monospace(if secret { "********" } else { value });
                        ui.end_row();
                    }
                });
            ui.monospace("elogtext");
            let text = pairs
                .iter()
                .find(|(n, _)| n == "elogtext")
                .map_or("", |(_, v)| v.as_str());
            ui.add(
                egui::TextEdit::multiline(&mut &*text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY),
            );
        });
    }

    pub fn buttons(&mut self, ui: &mut Ui, language: &str) -> bool {
        let t = |k| texts().get(language, k);
        zlog_ui::form::buttons(ui, |ui| match self.step {
            Step::Ready { confirm: false } => {
                let ready = self.request().is_some();
                if ui
                    .add_enabled(ready, egui::Button::new(t("upload.send")))
                    .clicked()
                {
                    self.step = Step::Ready { confirm: true };
                }
                ui.button(t("upload.close")).clicked()
            }
            Step::Ready { confirm: true } => {
                let (q, yes, no) = (
                    t("upload.confirm"),
                    t("upload.confirm_send"),
                    t("upload.back"),
                );
                match zlog_ui::dialog::confirm_row(ui, q, yes, no) {
                    Some(true) => self.send(),
                    Some(false) => self.step = Step::Ready { confirm: false },
                    None => {}
                }
                false
            }
            _ => ui.button(t("upload.close")).clicked(),
        })
    }
}

pub struct JarlWeb;

impl zlog_summary::WebSubmit for JarlWeb {
    fn accepts(&self, name: &str, first_mode: Option<Mode>) -> bool {
        contest_of(name, first_mode).is_some()
    }

    fn open(
        &self,
        name: &str,
        first_mode: Option<Mode>,
        elog: &[u8],
    ) -> Option<Box<dyn zlog_summary::SubmitForm>> {
        let contest = contest_of(name, first_mode)?;
        let text = encoding_rs::SHIFT_JIS.decode(elog).0.into_owned();
        Some(Box::new(Upload::new(contest, text)))
    }
}

impl zlog_summary::SubmitForm for Upload {
    fn ui(&mut self, ui: &mut Ui, language: &str) {
        Upload::ui(self, ui, language)
    }

    fn buttons(&mut self, ui: &mut Ui, language: &str) -> bool {
        Upload::buttons(self, ui, language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::member::tests::fake;

    const PAGE: &str = r#"<html><head><meta charset="Shift_JIS"></head><body>
<form name="form" method="post" action="confirm.php">
<input type="hidden" name="mode" value="check">
<input type="radio" name="contest" id="rb_allja" value="allja">
<input type="radio" name="contest" id="rb_6d" value="6d" disabled>
<input type="radio" name="contest" id="rb_nyp" value="nyp">
<textarea name="elogtext"></textarea>
<input type="submit" value="Upload">
</form></body></html>"#;

    #[test]
    fn contests_as_zlog_maps_them() {
        assert_eq!(contest_of("ALL JA", None), Some(Contest::Allja));
        assert_eq!(contest_of("Field Day", None).unwrap().radio_id(), "rb_fd");
        let aa = contest_of("All Asian DX Contest (Asia)", Some(Mode::Cw)).unwrap();
        assert_eq!((aa.value(), aa.url()), ("aacw", UPLOAD_URL_AA));
        assert_eq!(
            contest_of("All Asian DX Contest (Asia)", Some(Mode::Ssb)),
            Some(Contest::AllAsianPhone)
        );
        assert_eq!(contest_of("CQ WW DX", None), None);
        assert_eq!(Contest::NewYearParty.url(), UPLOAD_URL);
    }

    #[test]
    fn the_form_is_filled_in_and_a_disabled_contest_left_unchecked() {
        let page = Reply {
            status: 200,
            body: PAGE.as_bytes().to_vec(),
            ..Reply::default()
        };
        let f = prepare(&page, UPLOAD_URL, Contest::Allja, "<SUMMARYSHEET>").unwrap();
        assert_eq!(f.action, "https://contest.jarl.org/upload/confirm.php");
        let moved = Reply {
            url: "https://contest.jarl.org/v2/index.php".into(),
            ..page.clone()
        };
        let f2 = prepare(&moved, UPLOAD_URL, Contest::Allja, "x").unwrap();
        assert_eq!(f2.action, "https://contest.jarl.org/v2/confirm.php");
        assert_eq!(
            f.pairs(),
            [
                ("mode".to_string(), "check".to_string()),
                ("contest".into(), "allja".into()),
                ("elogtext".into(), "<SUMMARYSHEET>".into()),
            ]
        );
        let f = prepare(&page, UPLOAD_URL, Contest::SixDown, "x").unwrap();
        assert!(f.checked(GROUP).is_none());
        let none = Reply::default();
        assert!(prepare(&none, UPLOAD_URL, Contest::Allja, "x").is_err());
    }

    #[test]
    fn nothing_is_sent_before_the_confirmation_and_the_answer_is_kept() {
        let (t, sent) = fake(&[PAGE, "<html><script>x()</script><p>受付ました</p></html>"]);
        let mut u = Upload::with_transport(Contest::NewYearParty, "ログ".into(), t);
        u.poll();
        assert!(matches!(u.step, Step::Ready { confirm: false }));
        u.send();
        assert_eq!(sent.borrow().len(), 1, "not sent without the confirmation");
        u.step = Step::Ready { confirm: true };
        u.send();
        u.poll();
        let sent = sent.borrow();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].url, UPLOAD_URL);
        let (_, body) = sent[1].post.clone().unwrap();
        assert_eq!(
            String::from_utf8(body).unwrap(),
            "mode=check&contest=nyp&elogtext=%83%8D%83O"
        );
        match &u.step {
            Step::Sent(200, text) => assert_eq!(text, "受付ました"),
            _ => panic!("no answer"),
        }
    }

    #[test]
    fn answer_text() {
        assert_eq!(
            reply_text("<STYLE>p{}</STYLE><p>a<br>b</p>\n\n<p>&lt;c&gt;</p>"),
            "a\nb\n<c>"
        );
        assert_eq!(
            reply_text("<script>x</script >ok<SCRIPT>y</SCRIPT\n>done"),
            "okdone"
        );
    }
}
