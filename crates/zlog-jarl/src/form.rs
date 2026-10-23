//! A Web form read from its page and filled in as a browser would: what zLog does
//! in its embedded browser (sets a field, checks a radio button, presses a button),
//! done on the HTML, so that what is sent can be shown before it is sent.
//!
//! Only what forms need is read: `<form>`, `<input>`, `<textarea>`, `<select>`
//! with its `<option>`s, and `<button>`.

/// One control of a form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Control {
    /// `input` type in lower case (`text`, `hidden`, `radio`, `submit`, ...),
    /// `textarea`, `select`, `button` (a `<button>` that submits) or `push` (a
    /// button that does not).
    pub kind: String,
    pub name: String,
    pub id: String,
    pub value: String,
    pub checked: bool,
    pub disabled: bool,
}

impl Control {
    /// Whether it is a button that submits the form; sent only when pressed.
    pub fn is_submit(&self) -> bool {
        matches!(self.kind.as_str(), "submit" | "image" | "button")
    }
}

/// A form of a page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Form {
    pub name: String,
    pub id: String,
    /// Absolute URL the form is sent to.
    pub action: String,
    /// `get` or `post`.
    pub method: String,
    /// Sent as `multipart/form-data` (its `enctype`).
    pub multipart: bool,
    pub controls: Vec<Control>,
    /// The submit button pressed, by index in `controls`.
    pub pressed: Option<usize>,
}

impl Form {
    /// The control whose `id` or else `name` is `key`.
    pub fn control_mut(&mut self, key: &str) -> Option<&mut Control> {
        let i = self
            .controls
            .iter()
            .position(|c| c.id == key)
            .or_else(|| self.controls.iter().position(|c| c.name == key))?;
        Some(&mut self.controls[i])
    }

    /// Set the value of the field `key` (by `id`, else `name`); false when the form
    /// has no such field.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        match self.control_mut(key) {
            Some(c) => {
                c.value = value.to_string();
                true
            }
            None => false,
        }
    }

    /// Check the radio button `id`, unchecking the others of its group, unless it
    /// is disabled (zLog's `CheckOn`: `if (e.disabled == false) e.checked = true`);
    /// false when it is missing or disabled.
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

    /// The radio button checked in the group `name`.
    pub fn checked(&self, name: &str) -> Option<&Control> {
        self.controls
            .iter()
            .find(|c| c.kind == "radio" && c.name == name && c.checked)
    }

    /// Press the submit button `key` (by `id`, else `name`); false when missing.
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

    /// The name and value pairs a browser sends, in document order: enabled,
    /// named controls, radio buttons and check boxes only when checked, and of the
    /// buttons only the one pressed.
    pub fn pairs(&self) -> Vec<(String, String)> {
        self.controls
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                !c.disabled
                    && !c.name.is_empty()
                    && match c.kind.as_str() {
                        "radio" | "checkbox" => c.checked,
                        "reset" | "push" | "file" => false,
                        _ if c.is_submit() => self.pressed == Some(*i),
                        _ => true,
                    }
            })
            .map(|(_, c)| {
                let value = match c.kind.as_str() {
                    "radio" | "checkbox" if c.value.is_empty() => "on".to_string(),
                    _ => c.value.clone(),
                };
                (c.name.clone(), value)
            })
            .collect()
    }

    /// The request sending [`Form::pairs`], the values in `encoding` (the page's
    /// charset, as a browser sends them), with the `cookies` of the session.
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
            let base = self.action.split('?').next().unwrap_or(&self.action);
            zlog_net::Request::get(format!("{base}?{query}"))
        };
        request.cookies(cookies)
    }
}

/// The forms of `html`, a page at `base` (for relative actions).
pub fn forms(html: &str, base: &str) -> Vec<Form> {
    let mut out: Vec<Form> = Vec::new();
    let mut rest = html;
    // A select whose options are being read, by index in the controls of the
    // form being read, and whether an option was selected.
    let mut select: Option<(usize, bool)> = None;
    // Whether the last form is still open: controls after `</form>` belong to none.
    let mut open_form = false;
    // Whether the next option is the first of its select.
    let mut first_option = false;
    while let Some(open) = rest.find('<') {
        let after = &rest[open + 1..];
        if after.starts_with("!--") {
            rest = after.find("-->").map_or("", |e| &after[e + 3..]);
            continue;
        }
        let Some(close) = tag_end(after) else {
            break;
        };
        let tag = &after[..close];
        rest = &after[close + 1..];
        let (name, attrs) = parse_tag(tag);
        let attr = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let has = |k: &str| attrs.iter().any(|(n, _)| n == k);
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
            "/form" => {
                select = None;
                open_form = false;
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
                    let end = find_ci(rest, "</textarea").unwrap_or(rest.len());
                    let text = &rest[..end];
                    // A line break right after the tag is not part of the value.
                    let text = text.strip_prefix("\r\n").or(text.strip_prefix('\n'));
                    value = decode(text.unwrap_or(&rest[..end]));
                    rest = &rest[end..];
                }
                if name == "select" {
                    select = Some((form.controls.len(), false));
                    first_option = true;
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
                let text_end = rest.find('<').unwrap_or(rest.len());
                let value = attr("value").unwrap_or_else(|| decode(rest[..text_end].trim()));
                // The first option, unless a later one is selected.
                let Some(control) = form.controls.get_mut(*i) else {
                    continue;
                };
                if !*chosen && (has("selected") || first_option) {
                    control.value = value;
                    *chosen = has("selected");
                }
                first_option = false;
            }
            "/select" => select = None,
            _ => {}
        }
    }
    out
}

/// The end of a tag: the `>` outside quotes.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn find_ci(s: &str, needle: &str) -> Option<usize> {
    s.to_ascii_lowercase().find(needle)
}

/// The name of a tag in lower case and its attributes (names in lower case,
/// values decoded).
fn parse_tag(tag: &str) -> (String, Vec<(String, String)>) {
    let tag = tag.trim_end_matches('/');
    let name_end = tag.find(|c: char| c.is_whitespace()).unwrap_or(tag.len());
    let name = tag[..name_end].to_lowercase();
    let mut attrs = Vec::new();
    let mut s = &tag[name_end..];
    loop {
        s = s.trim_start();
        if s.is_empty() {
            break;
        }
        let key_end = s
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(s.len());
        let key = s[..key_end].to_lowercase();
        s = s[key_end..].trim_start();
        let mut value = String::new();
        if let Some(v) = s.strip_prefix('=') {
            let v = v.trim_start();
            let (raw, rest) = match v.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let end = v[1..].find(q).map_or(v.len(), |e| e + 1);
                    (&v[1..end], v.get(end + 1..).unwrap_or(""))
                }
                _ => {
                    let end = v.find(char::is_whitespace).unwrap_or(v.len());
                    (&v[..end], &v[end..])
                }
            };
            value = decode(raw);
            s = rest;
        }
        if !key.is_empty() {
            attrs.push((key, value));
        }
    }
    (name, attrs)
}

/// Decode the character references of HTML text.
pub fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let end = rest.find(';').filter(|e| *e <= 10);
        let named = end.and_then(|e| {
            let r = &rest[1..e];
            let c = match r {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => '\u{a0}',
                _ => {
                    let n = match r.strip_prefix("#x").or(r.strip_prefix("#X")) {
                        Some(h) => u32::from_str_radix(h, 16).ok(),
                        None => r.strip_prefix('#').and_then(|d| d.parse().ok()),
                    };
                    char::from_u32(n?)?
                }
            };
            Some((c, e))
        });
        match named {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The text of HTML: without tags, the character references decoded once, a
/// no-break space as a space.
pub fn text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode(&out).replace('\u{a0}', " ")
}

/// The absolute URL of `href` on the page `base`.
pub fn resolve(base: &str, href: &str) -> String {
    let href = href.trim();
    if href.contains("://") {
        return href.to_string();
    }
    let base = base.split('#').next().unwrap_or(base);
    if href.is_empty() {
        return base.to_string();
    }
    let origin_end = base
        .find("://")
        .and_then(|p| base[p + 3..].find('/').map(|e| p + 3 + e))
        .unwrap_or(base.len());
    let origin = &base[..origin_end];
    if let Some(path) = href.strip_prefix('/') {
        return format!("{origin}/{path}");
    }
    let path = &base[origin_end..];
    let path = path.split('?').next().unwrap_or(path);
    if href.starts_with('?') {
        return format!("{origin}{path}{href}");
    }
    let dir = &path[..path.rfind('/').map_or(0, |p| p + 1)];
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    let mut href = href;
    loop {
        if let Some(r) = href.strip_prefix("./") {
            href = r;
        } else if let Some(r) = href.strip_prefix("../") {
            parts.pop();
            href = r;
        } else {
            break;
        }
    }
    let dir: String = parts.iter().map(|p| format!("/{p}")).collect();
    format!("{origin}{dir}/{href}")
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
    fn character_references() {
        assert_eq!(decode("&lt;&#x41;&#66;&amp;amp; & x"), "<AB&amp; & x");
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
    fn text_decodes_once_and_survives_length_changing_case() {
        assert_eq!(text("<b>a&amp;lt;b</b>&nbsp;c"), "a&lt;b c");
        assert_eq!(
            crate::upload::reply_text("İİ<script>x</script>ok&amp;lt;"),
            "İİok&lt;"
        );
    }
}
