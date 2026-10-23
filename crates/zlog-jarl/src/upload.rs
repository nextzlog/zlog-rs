//! Submitting the E-Log on JARL's Web form (`TformJarlWebUpload`,
//! `TformJarlWebUpload2`, opened by the Web upload button of zLog's E-Log dialog
//! for R2.1).
//!
//! zLog opens the upload page in an embedded browser (`ELogWebUploadURL`, or
//! `ELogWebUploadURLaa` for the All Asian), puts the E-Log into the field
//! `elogtext` of the form `form` and checks the radio button of the contest
//! (`rb_allja`, `rb_6d`, `rb_fd`, `rb_acag`, `rb_aacw`, `rb_aaph`, `rb_nyp`;
//! name `contest`, value without `rb_`) unless it is disabled; the operator then
//! sends it with the page's own button and reads the answer in the browser.
//!
//! Here the page is fetched and its form filled in the same way, then shown: the
//! address it goes to and every field, the E-Log in full. Nothing is sent until
//! the operator confirms; the server's answer is shown as text.

use egui::{RichText, Ui};
use zlog_core::Mode;
use zlog_net::{Exchange, Reply, Request};

use crate::form::Form;
use crate::{texts, Transport};

/// The upload page (`ELogWebUploadURL`).
pub const UPLOAD_URL: &str = "https://contest.jarl.org/upload/";

/// The upload page of the All Asian DX Contest (`ELogWebUploadURLaa`).
pub const UPLOAD_URL_AA: &str = "https://contest.jarl.org/upload-aa/";

/// The contests the page takes (`TWebUploadContest` but `wuOther`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contest {
    Allja,
    SixDown,
    FieldDay,
    Acag,
    /// All Asian DX, CW.
    AllAsianCw,
    /// All Asian DX, phone.
    AllAsianPhone,
    NewYearParty,
}

impl Contest {
    /// Value of the radio button (`CheckOn('contest', ...)`).
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

    /// `id` of the radio button (`CheckOn('rb_...')`).
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

/// The contest of the page for the contest named `name` (`buttonWebUploadClick`:
/// by contest class; the All Asian by the mode of the first QSO), `None` for the
/// others, for which zLog hides the button.
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

/// The radio group of the contests.
const GROUP: &str = "contest";

/// The form of the upload page filled in: the E-Log in `elogtext` and the
/// contest checked when the page allows it (else none, and the operator picks
/// one). An error when the page has no such form.
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
        // The IE form of zLog finds it by name and value.
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
    // The page's own send button, the first one.
    if let Some(i) = form.controls.iter().position(|c| c.is_submit()) {
        form.pressed = Some(i);
    }
    if form.method.is_empty() {
        form.method = "post".into();
    }
    Ok(form)
}

/// The text of an answer page: without tags, scripts and styles, blank lines
/// dropped.
pub fn reply_text(html: &str) -> String {
    let mut s = html.to_string();
    for tag in ["script", "style"] {
        while let Some(a) = s.to_ascii_lowercase().find(&format!("<{tag}")) {
            let end = s.to_ascii_lowercase()[a..]
                .find(&format!("</{tag}>"))
                .map_or(s.len(), |e| a + e + tag.len() + 3);
            s.replace_range(a..end, "");
        }
    }
    let s = s
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");
    crate::form::text(&s)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

enum Step {
    /// The upload page is being fetched.
    Page(Exchange),
    /// The form is shown; `confirm` once the operator pressed Send, asking again.
    Ready {
        confirm: bool,
    },
    Sending(Exchange),
    /// The server's answer: status and text.
    Sent(u16, String),
    Failed(String),
}

/// The submission of one E-Log, from fetching the page to the server's answer.
pub struct Upload {
    contest: Contest,
    elog: String,
    form: Option<Form>,
    page: Reply,
    step: Step,
    transport: Transport,
}

impl Upload {
    /// Fetch the upload page of `contest` for `elog`, the E-Log as text.
    pub fn new(contest: Contest, elog: String) -> Self {
        Upload::with_transport(contest, elog, Box::new(Exchange::send))
    }

    /// As [`Upload::new`], with the requests sent by `transport`.
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

    /// The form as it will be sent, once the page has arrived.
    pub fn form(&self) -> Option<&Form> {
        self.form.as_ref()
    }

    /// The request sending the form, as it stands.
    fn request(&self) -> Option<Request> {
        let form = self.form.as_ref()?;
        form.checked(GROUP)?;
        Some(form.request(self.page.encoding(), &self.page.cookies))
    }

    /// Take what has arrived.
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

    /// Send the form: only from the confirmation.
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
            // Fields the page asks the operator for, besides the E-Log.
            let editable = |k: &str| matches!(k, "text" | "email" | "tel" | "number" | "password");
            let labels: Vec<String> = form
                .controls
                .iter()
                .filter(|c| editable(&c.kind) && !c.name.is_empty())
                .map(|c| c.name.clone())
                .collect();
            if !labels.is_empty() {
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                let width = zlog_ui::form_field_width(ui, &refs);
                egui::Grid::new("jarl_upload_fields")
                    .num_columns(2)
                    .show(ui, |ui| {
                        for c in form
                            .controls
                            .iter_mut()
                            .filter(|c| editable(&c.kind) && !c.name.is_empty())
                        {
                            ui.label(&c.name);
                            let edit = egui::TextEdit::singleline(&mut c.value)
                                .password(c.kind == "password");
                            zlog_ui::sized_field(ui, edit, width);
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
                        // A password is sent but not shown.
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

    /// The bottom row: Send (then the confirmation) and Close; true to close.
    pub fn buttons(&mut self, ui: &mut Ui, language: &str) -> bool {
        let t = |k| texts().get(language, k);
        ui.horizontal(|ui| match self.step {
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
        .inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::member::tests::fake;

    /// The upload page as zLog's code expects it.
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
        // Resolved against the page answered after a redirect.
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
        // The page is Shift_JIS: the E-Log is sent in it.
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
    }
}
