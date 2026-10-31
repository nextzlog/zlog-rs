#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Control {
    pub kind: String,
    pub name: String,
    pub id: String,
    pub value: String,
    pub checked: bool,
    pub disabled: bool,
}

impl Control {
    pub fn is_submit(&self) -> bool {
        matches!(self.kind.as_str(), "submit" | "image" | "button")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Form {
    pub name: String,
    pub id: String,
    pub action: String,
    pub method: String,
    pub multipart: bool,
    pub controls: Vec<Control>,
    pub pressed: Option<usize>,
}

impl Form {
    pub fn control_mut(&mut self, key: &str) -> Option<&mut Control> {
        let i = self
            .controls
            .iter()
            .position(|c| c.id == key)
            .or_else(|| self.controls.iter().position(|c| c.name == key))?;
        Some(&mut self.controls[i])
    }

    pub fn set(&mut self, key: &str, value: &str) -> bool {
        match self.control_mut(key) {
            Some(c) => {
                c.value = value.to_string();
                c.checked |= c.kind == "select";
                true
            }
            None => false,
        }
    }

    pub fn check(&mut self, id: &str) -> bool {
        let Some(i) = self
            .controls
            .iter()
            .position(|c| c.kind == "radio" && c.id == id && !c.disabled)
        else {
            return false;
        };
        let group = self.controls[i].name.clone();
        for (j, c) in self.controls.iter_mut().enumerate() {
            if c.kind == "radio" && c.name == group {
                c.checked = i == j;
            }
        }
        true
    }

    pub fn checked(&self, name: &str) -> Option<&Control> {
        self.controls
            .iter()
            .find(|c| c.kind == "radio" && c.name == name && c.checked)
    }

    pub fn press(&mut self, key: &str) -> bool {
        self.pressed = self
            .controls
            .iter()
            .position(|c| c.is_submit() && c.id == key)
            .or_else(|| {
                self.controls
                    .iter()
                    .position(|c| c.is_submit() && c.name == key)
            });
        self.pressed.is_some()
    }

    pub fn pairs(&self) -> Vec<(String, String)> {
        self.controls
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                !c.disabled
                    && (!c.name.is_empty() || c.kind == "image")
                    && match c.kind.as_str() {
                        "radio" | "checkbox" | "select" => c.checked,
                        "reset" | "push" | "file" => false,
                        _ if c.is_submit() => self.pressed == Some(*i),
                        _ => true,
                    }
            })
            .flat_map(|(_, c)| match c.kind.as_str() {
                "image" => {
                    let prefix = match c.name.is_empty() {
                        true => String::new(),
                        false => format!("{}.", c.name),
                    };
                    vec![
                        (format!("{prefix}x"), "0".into()),
                        (format!("{prefix}y"), "0".into()),
                    ]
                }
                "radio" | "checkbox" if c.value.is_empty() => vec![(c.name.clone(), "on".into())],
                _ => vec![(c.name.clone(), c.value.clone())],
            })
            .collect()
    }

    pub fn request(
        &self,
        encoding: &'static encoding_rs::Encoding,
        cookies: &str,
    ) -> zlog_net::Request {
        let fields: Vec<(String, Vec<u8>)> = self
            .pairs()
            .into_iter()
            .map(|(n, v)| (n, encoding.encode(&v).0.into_owned()))
            .collect();
        let request = if self.method == "post" {
            if self.multipart {
                let (content_type, body) = zlog_net::multipart(&fields);
                zlog_net::Request::post(&self.action, content_type, body)
            } else {
                zlog_net::Request::post(
                    &self.action,
                    "application/x-www-form-urlencoded",
                    zlog_net::urlencoded(&fields),
                )
            }
        } else {
            let query = String::from_utf8_lossy(&zlog_net::urlencoded(&fields)).into_owned();
            let url = match url::Url::parse(&self.action) {
                Ok(mut url) => {
                    url.set_fragment(None);
                    url.set_query(Some(&query));
                    url.to_string()
                }
                Err(_) => {
                    let base = self.action.split(['?', '#']).next().unwrap_or_default();
                    format!("{base}?{query}")
                }
            };
            zlog_net::Request::get(url)
        };
        request.cookies(cookies)
    }
}

pub fn forms(html: &str, base: &str) -> Vec<Form> {
    use zlog_net::html5gum::Token;
    let mut out: Vec<Form> = Vec::new();
    let mut tokens = zlog_net::html_tokens(html).peekable();
    let mut select: Option<(usize, bool)> = None;
    let mut open_form = false;
    let mut defaulted = false;
    let mut group_disabled = false;
    while let Some(token) = tokens.next() {
        let tag = match token {
            Token::StartTag(tag) => tag,
            Token::EndTag(tag) => {
                match &tag.name[..] {
                    b"form" => {
                        select = None;
                        open_form = false;
                    }
                    b"select" => select = None,
                    b"optgroup" => group_disabled = false,
                    _ => {}
                }
                continue;
            }
            _ => continue,
        };
        let attr = |k: &str| {
            tag.attributes
                .get(k.as_bytes())
                .map(|v| String::from_utf8_lossy(v).into_owned())
        };
        let has = |k: &str| tag.attributes.contains_key(k.as_bytes());
        let mut text = || {
            let mut text = String::new();
            while let Some(Token::String(s)) = tokens.peek() {
                text.push_str(&String::from_utf8_lossy(s));
                tokens.next();
            }
            text
        };
        let name = String::from_utf8_lossy(&tag.name).into_owned();
        match name.as_str() {
            "form" => {
                select = None;
                open_form = true;
                out.push(Form {
                    name: attr("name").unwrap_or_default(),
                    id: attr("id").unwrap_or_default(),
                    action: resolve(base, &attr("action").unwrap_or_default()),
                    method: attr("method").unwrap_or_default().to_lowercase(),
                    multipart: attr("enctype")
                        .is_some_and(|e| e.eq_ignore_ascii_case("multipart/form-data")),
                    ..Form::default()
                })
            }
            "input" | "textarea" | "select" | "button" => {
                let Some(form) = out.last_mut().filter(|_| open_form) else {
                    continue;
                };
                let kind = match name.as_str() {
                    "input" => match attr("type").map(|t| t.to_lowercase()).as_deref() {
                        Some("button") => "push".into(),
                        Some(t) => t.into(),
                        None => "text".into(),
                    },
                    "button" => match attr("type").map(|t| t.to_lowercase()).as_deref() {
                        Some("reset") => "reset".into(),
                        Some("button") => "push".into(),
                        _ => "button".into(),
                    },
                    other => other.into(),
                };
                let mut value = attr("value").unwrap_or_default();
                if name == "textarea" {
                    let text = text();
                    value = text.strip_prefix('\n').unwrap_or(&text).to_string();
                }
                if name == "select" {
                    select = Some((form.controls.len(), false));
                    defaulted = false;
                    group_disabled = false;
                }
                form.controls.push(Control {
                    kind,
                    name: attr("name").unwrap_or_default(),
                    id: attr("id").unwrap_or_default(),
                    value,
                    checked: has("checked"),
                    disabled: has("disabled"),
                });
            }
            "option" => {
                let (Some(form), Some((i, chosen))) = (out.last_mut(), select.as_mut()) else {
                    continue;
                };
                let value = attr("value").unwrap_or_else(|| text().trim().to_string());
                let Some(control) = form.controls.get_mut(*i) else {
                    continue;
                };
                let disabled = has("disabled") || group_disabled;
                if has("selected") || (!*chosen && !defaulted && !disabled) {
                    control.value = value;
                    control.checked = !disabled;
                    *chosen |= has("selected");
                    defaulted = true;
                }
            }
            "optgroup" => group_disabled = has("disabled"),
            _ => {}
        }
    }
    out
}

pub fn resolve(base: &str, href: &str) -> String {
    url::Url::parse(base)
        .and_then(|b| b.join(href.trim()))
        .map_or_else(|_| href.trim().to_string(), String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<html><body>
<!-- <form name="old"> -->
<form name="form" method="POST" action="./confirm.php" enctype="multipart/form-data">
<input type="hidden" name="token" value="a&amp;b">
<input type="radio" name="contest" id="rb_allja" value="allja">
<input type="radio" name="contest" id="rb_6d" value="6d" checked>
<input type="radio" name="contest" id="rb_fd" value="fd" disabled>
<textarea name="elogtext" rows=10>
old</textarea>
<select name="kind"><option value="a">A</option><option selected>B</option></select>
<input type=checkbox name="agree">
<input type="submit" name="send" value="送信">
<button type="reset">clear</button>
</form></body></html>"#;

    #[test]
    fn the_form_is_read_and_filled_in_as_a_browser_would() {
        let mut f = forms(PAGE, "https://contest.jarl.org/upload/").remove(0);
        assert_eq!(f.name, "form");
        assert_eq!(f.action, "https://contest.jarl.org/upload/confirm.php");
        assert_eq!(f.method, "post");
        assert!(f.multipart);
        assert!(f.set("elogtext", "x\r\ny"));
        assert!(!f.set("missing", ""));
        assert!(f.check("rb_allja"));
        assert!(!f.check("rb_fd"), "disabled");
        assert_eq!(f.checked("contest").unwrap().value, "allja");
        assert!(f.press("send"));
        let pairs = f.pairs();
        let names: Vec<&str> = pairs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["token", "contest", "elogtext", "kind", "send"]);
        assert_eq!(pairs[0].1, "a&b");
        assert_eq!(pairs[1].1, "allja");
        assert_eq!(pairs[3].1, "B");
    }

    #[test]
    fn a_textarea_keeps_its_text() {
        let mut f = forms(PAGE, "https://x/").remove(0);
        assert_eq!(f.control_mut("elogtext").unwrap().value, "old");
    }

    #[test]
    fn requests_in_the_charset_of_the_page() {
        let mut f = forms(PAGE, "https://x/a").remove(0);
        f.multipart = false;
        f.controls.retain(|c| c.name == "elogtext");
        f.set("elogtext", "テ");
        let r = f.request(encoding_rs::SHIFT_JIS, "S=1");
        assert_eq!(
            r.post,
            Some((
                "application/x-www-form-urlencoded".into(),
                b"elogtext=%83e".to_vec()
            ))
        );
        assert_eq!(
            (r.url.as_str(), r.cookies.as_str()),
            ("https://x/confirm.php", "S=1")
        );
        f.method = "get".into();
        assert_eq!(
            f.request(encoding_rs::UTF_8, "").url,
            "https://x/confirm.php?elogtext=%E3%83%86"
        );
    }

    #[test]
    fn relative_actions() {
        let base = "https://www.jarl.com/Page/Search/MemberSearch.aspx?Language=Jp";
        assert_eq!(
            resolve(base, "./MemberSearch.aspx?Language=Jp"),
            "https://www.jarl.com/Page/Search/MemberSearch.aspx?Language=Jp"
        );
        assert_eq!(resolve(base, "/a"), "https://www.jarl.com/a");
        assert_eq!(resolve(base, "../b"), "https://www.jarl.com/Page/b");
        assert_eq!(resolve(base, ""), base);
        assert_eq!(
            resolve(base, "?x=1"),
            "https://www.jarl.com/Page/Search/MemberSearch.aspx?x=1"
        );
        assert_eq!(resolve("https://h", "a"), "https://h/a");
    }

    #[test]
    fn get_forms_drop_the_fragment_of_the_action() {
        let mut f = forms(
            r#"<form action="a.php?old=1#top"><input name="q" value="1"></form>"#,
            "https://h/",
        )
        .remove(0);
        f.method = "get".into();
        assert_eq!(f.request(encoding_rs::UTF_8, "").url, "https://h/a.php?q=1");
    }

    #[test]
    fn image_buttons_send_coordinates_and_disabled_options_are_skipped() {
        let html = r#"<form action="a"><select name="s"><option value="no" disabled>-</option>
<option value="yes">Y</option></select><input type="image" name="go" value="v"></form>"#;
        let mut f = forms(html, "https://h/").remove(0);
        assert_eq!(f.pairs(), vec![("s".to_string(), "yes".to_string())]);
        assert!(f.press("go"));
        let pairs = f.pairs();
        assert_eq!(
            pairs[1..],
            [("go.x".into(), "0".into()), ("go.y".into(), "0".into())]
        );
    }

    #[test]
    fn selects_follow_the_html_selectedness_rules() {
        let html = r#"<form action="a">
<select name="a"><option value="1">1</option><option value="2" selected disabled>2</option></select>
<select name="b"><optgroup label="g" disabled><option value="1">1</option></optgroup>
<option value="2">2</option></select>
<select name="c"><option value="1" selected>1</option><option value="2" selected>2</option></select>
<select name="d"></select>
<input type="image" id="go"></form>"#;
        let mut f = forms(html, "https://h/").remove(0);
        let pair = |n: &str, v: &str| (n.to_string(), v.to_string());
        assert_eq!(f.pairs(), vec![pair("b", "2"), pair("c", "2")]);
        assert!(f.press("go"));
        assert!(f.set("d", "x"));
        assert_eq!(
            f.pairs(),
            vec![
                pair("b", "2"),
                pair("c", "2"),
                pair("d", "x"),
                pair("x", "0"),
                pair("y", "0")
            ]
        );
    }

    #[test]
    fn character_references() {
        assert_eq!(
            zlog_net::html_text("&lt;&#x41;&#66;&amp;amp; & x"),
            "<AB&amp; & x"
        );
    }

    #[test]
    fn controls_after_the_form_end_and_empty_first_options() {
        let html = r#"<form action="a"><select name="s"><option value="">-</option>
<option value="x">X</option></select></form><input name="outside" value="1">
<form action="b"><select name="t"><option value="y">Y</option></form>
<option value="z">Z</option>"#;
        let f = forms(html, "https://h/");
        assert_eq!(f[0].pairs(), vec![("s".to_string(), String::new())]);
        assert_eq!(f[1].pairs(), vec![("t".to_string(), "y".to_string())]);
    }

    #[test]
    fn text_and_scripts_with_brackets_and_quotes_do_not_hide_forms() {
        let html = r#"<p>a < b isn't</p><script>if (a<b && c>'d') {}</script>
<form action="x"><input name="n" value="1"></form>"#;
        let f = forms(html, "https://h/");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].pairs(), vec![("n".to_string(), "1".to_string())]);
    }

    #[test]
    fn text_decodes_once_and_survives_length_changing_case() {
        assert_eq!(zlog_net::html_text("<b>a&amp;lt;b</b>&nbsp;c"), "a&lt;b c");
        assert_eq!(
            crate::upload::reply_text("İİ<script>x</script>ok&amp;lt;"),
            "İİok&lt;"
        );
    }
}
