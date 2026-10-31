use std::collections::BTreeMap;

use zlog_core::qso::QslState;
use zlog_core::{Log, Qso, QsoId};
use zlog_net::{Exchange, Reply, Request};

use crate::form;
use crate::Transport;

pub const SEARCH_URL: &str = "https://www.jarl.com/Page/Search/MemberSearch.aspx?Language=Jp";

pub const BATCH: usize = 20;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Member {
    pub member: bool,
    pub transfer: bool,
}

pub fn callsigns(log: &Log) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for q in &log.qsos {
        let call = q.core_call().to_string();
        if zlog_cfg::common::is_domestic(&call) && !out.contains(&call) {
            out.push(call);
        }
    }
    out
}

pub fn search(page: &Reply, url: &str, calls: &[String]) -> Result<Request, String> {
    let mut forms = form::forms(
        &page.text(),
        if page.url.is_empty() { url } else { &page.url },
    );
    let form = forms
        .iter_mut()
        .find(|f| f.controls.iter().any(|c| c.id == "txtCallSign"))
        .ok_or("no txtCallSign on the member search page")?;
    form.set("txtCallSign", &calls.join(" "));
    if !form.press("btnSearch") {
        return Err("no btnSearch on the member search page".into());
    }
    if form.method.is_empty() {
        form.method = "post".into();
    }
    Ok(form.request(page.encoding(), &page.cookies))
}

pub fn results(html: &str) -> Vec<(String, String)> {
    (0..BATCH)
        .map_while(|i| {
            let call = element_text(html, &format!("ListView1_lblCallSign_{i}"))?;
            let result =
                element_text(html, &format!("ListView1_lblResult_{i}")).unwrap_or_default();
            Some((call, result))
        })
        .collect()
}

pub fn apply(answers: &mut BTreeMap<String, Member>, rows: &[(String, String)]) {
    for (call, result) in rows {
        if let Some(m) = answers.get_mut(call) {
            m.member |= result.contains('○');
            m.transfer |= result.contains("Yes");
        }
    }
}

pub fn changes(log: &Log, answers: &BTreeMap<String, Member>) -> Vec<(QsoId, Qso)> {
    log.qsos
        .iter()
        .filter(|q| q.qsl_state != QslState::NoQsl)
        .filter(|q| answers.get(q.core_call()).is_some_and(|m| !m.transfer))
        .map(|q| {
            let mut new = q.clone();
            new.qsl_state = QslState::NoQsl;
            (QsoId::of(q), new)
        })
        .collect()
}

fn element_text(html: &str, id: &str) -> Option<String> {
    let at = [
        format!("id=\"{id}\""),
        format!("id='{id}'"),
        format!("id={id}>"),
    ]
    .iter()
    .filter_map(|p| {
        html.match_indices(p.as_str())
            .map(|(i, _)| i)
            .find(|&i| i > 0 && html.as_bytes()[i - 1].is_ascii_whitespace())
    })
    .min()?;
    let start = at + html[at..].find('>')? + 1;
    let tag_start = html[..at].rfind('<')?;
    let name: String = html[tag_start + 1..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    let end = html[start..]
        .to_ascii_lowercase()
        .find(&format!("</{}", name.to_ascii_lowercase()))
        .map_or(html.len(), |e| start + e);
    Some(zlog_net::html_text(&html[start..end]).trim().to_string())
}

enum Step {
    Page(Exchange),
    Search(Exchange),
}

pub struct Lookup {
    batches: Vec<Vec<String>>,
    batch: usize,
    step: Option<Step>,
    pub answers: BTreeMap<String, Member>,
    pub done: usize,
    pub error: Option<String>,
}

impl Lookup {
    pub fn start(calls: &[String], transport: &mut Transport) -> Self {
        let batches: Vec<Vec<String>> = calls.chunks(BATCH).map(<[String]>::to_vec).collect();
        let step = (!batches.is_empty()).then(|| Step::Page(transport(Request::get(SEARCH_URL))));
        Lookup {
            batches,
            batch: 0,
            step,
            answers: calls
                .iter()
                .map(|c| (c.clone(), Member::default()))
                .collect(),
            done: 0,
            error: None,
        }
    }

    pub fn batches(&self) -> usize {
        self.batches.len()
    }

    pub fn running(&self) -> bool {
        self.step.is_some()
    }

    pub fn poll(&mut self, transport: &mut Transport) {
        let Some(step) = self.step.as_mut() else {
            return;
        };
        let page = matches!(step, Step::Page(_));
        let (Step::Page(e) | Step::Search(e)) = step;
        let Some(result) = e.poll().cloned() else {
            return;
        };
        let reply = match result {
            Ok(r) if r.status < 400 => r,
            Ok(r) => return self.fail(format!("HTTP {}", r.status)),
            Err(e) => return self.fail(e),
        };
        if page {
            match search(&reply, SEARCH_URL, &self.batches[self.batch]) {
                Ok(request) => self.step = Some(Step::Search(transport(request))),
                Err(e) => self.fail(e),
            }
        } else {
            apply(&mut self.answers, &results(&reply.text()));
            self.done += 1;
            self.batch += 1;
            self.step = (self.batch < self.batches.len())
                .then(|| Step::Page(transport(Request::get(SEARCH_URL))));
        }
    }

    fn fail(&mut self, error: String) {
        self.error = Some(error);
        self.step = None;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    pub(crate) const PAGE: &str = r#"<html><head><meta charset="utf-8"></head><body>
<form method="post" action="./MemberSearch.aspx?Language=Jp" id="form1">
<input type="hidden" name="__VIEWSTATE" id="__VIEWSTATE" value="vs+/=" />
<textarea name="txtCallSign" id="txtCallSign"></textarea>
<input type="submit" name="btnSearch" value="検索" id="btnSearch" />
<input type="submit" name="btnClear" value="クリア" id="btnClear" />
</form></body></html>"#;

    pub(crate) const ANSWER: &str = r#"<table>
<tr><td><span id="ListView1_lblCallSign_0">JA1ZLO</span></td><td><span id="ListView1_lblResult_0">○ (QSL転送: Yes)</span></td></tr>
<tr><td><span id="ListView1_lblCallSign_1">JA1YAA</span></td><td><span id="ListView1_lblResult_1">○ (QSL転送: No)</span></td></tr>
</table>"#;

    fn qso(call: &str) -> Qso {
        Qso {
            callsign: call.into(),
            reserve3: call.len() as i32 * 100 + call.as_bytes()[call.len() - 1] as i32,
            ..Qso::default()
        }
    }

    fn reply(html: &str) -> Reply {
        Reply {
            status: 200,
            cookies: "ASP.NET_SessionId=s".into(),
            body: html.as_bytes().to_vec(),
            ..Reply::default()
        }
    }

    #[test]
    fn domestic_core_calls_once_each() {
        let log = Log {
            qsos: ["JA1ZLO/1", "K1ABC", "JA1ZLO", "7K1XYZ", "JA1YAA"]
                .into_iter()
                .map(qso)
                .collect(),
            ..Log::default()
        };
        assert_eq!(callsigns(&log), ["JA1ZLO", "7K1XYZ", "JA1YAA"]);
    }

    #[test]
    fn the_search_fills_the_page_in_as_zlog_does() {
        let calls = ["JA1ZLO".to_string(), "JA1YAA".to_string()];
        let r = search(&reply(PAGE), SEARCH_URL, &calls).unwrap();
        assert_eq!(r.url, SEARCH_URL);
        assert_eq!(r.cookies, "ASP.NET_SessionId=s");
        let (ct, body) = r.post.unwrap();
        assert_eq!(ct, "application/x-www-form-urlencoded");
        assert_eq!(
            String::from_utf8(body).unwrap(),
            "__VIEWSTATE=vs%2B%2F%3D&txtCallSign=JA1ZLO+JA1YAA&btnSearch=%E6%A4%9C%E7%B4%A2"
        );
        assert!(search(&reply("<html></html>"), SEARCH_URL, &calls).is_err());
    }

    #[test]
    fn only_the_id_attribute_names_the_element() {
        let html = r#"<label data-id="txtA">wrong</label><span aria-id='txtA'>no</span>
<span class="c" id="txtA">right</span>"#;
        assert_eq!(element_text(html, "txtA").as_deref(), Some("right"));
        assert_eq!(
            element_text("<b id=txtB>x</b>", "txtB").as_deref(),
            Some("x")
        );
        assert_eq!(element_text(r#"<i data-id="txtC">x</i>"#, "txtC"), None);
    }

    #[test]
    fn results_mark_members_and_forwarding() {
        let mut answers: BTreeMap<String, Member> = ["JA1ZLO", "JA1YAA", "JA1XXX"]
            .iter()
            .map(|c| (c.to_string(), Member::default()))
            .collect();
        let rows = results(ANSWER);
        assert_eq!(rows.len(), 2);
        apply(&mut answers, &rows);
        assert_eq!(
            answers["JA1ZLO"],
            Member {
                member: true,
                transfer: true
            }
        );
        assert_eq!(
            answers["JA1YAA"],
            Member {
                member: true,
                transfer: false
            }
        );
        assert_eq!(answers["JA1XXX"], Member::default());
        let log = Log {
            qsos: ["JA1ZLO", "JA1YAA/3", "JA1XXX", "K1ABC"]
                .into_iter()
                .map(qso)
                .collect(),
            ..Log::default()
        };
        let calls: Vec<String> = changes(&log, &answers)
            .into_iter()
            .map(|(_, q)| {
                assert_eq!(q.qsl_state, QslState::NoQsl);
                q.callsign
            })
            .collect();
        assert_eq!(calls, ["JA1YAA/3", "JA1XXX"]);
    }

    pub(crate) fn fake(answers: &'static [&'static str]) -> (Transport, Rc<RefCell<Vec<Request>>>) {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let log = sent.clone();
        let mut n = 0;
        let t: Transport = Box::new(move |r: Request| {
            log.borrow_mut().push(r);
            let body = answers[n % answers.len()];
            n += 1;
            Exchange::ready(Ok(reply(body)))
        });
        (t, sent)
    }

    #[test]
    fn batches_of_twenty_each_with_the_page_fetched_again() {
        let (mut t, sent) = fake(&[PAGE, ANSWER]);
        let calls: Vec<String> = (0..25).map(|i| format!("JA1A{i:02}")).collect();
        let mut l = Lookup::start(&calls, &mut t);
        assert_eq!(l.batches(), 2);
        while l.running() {
            l.poll(&mut t);
        }
        assert_eq!((l.done, l.error.as_ref()), (2, None));
        let sent = sent.borrow();
        assert_eq!(sent.len(), 4);
        assert!(sent[0].post.is_none() && sent[2].post.is_none());
        let body = String::from_utf8(sent[3].post.clone().unwrap().1).unwrap();
        assert!(body.contains("txtCallSign=JA1A20+JA1A21+JA1A22+JA1A23+JA1A24&"));
    }

    #[test]
    fn a_page_without_the_form_stops_the_check() {
        let (mut t, _) = fake(&["<html>maintenance</html>"]);
        let mut l = Lookup::start(&["JA1ZLO".into()], &mut t);
        l.poll(&mut t);
        assert!(!l.running());
        assert!(l.error.is_some());
    }
}
