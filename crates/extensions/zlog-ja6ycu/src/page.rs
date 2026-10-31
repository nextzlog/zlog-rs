pub const HOST: &str = "ja6ycu.in.coocan.jp";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub name: String,
    pub url: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entry {
    pub date: String,
    pub name: String,
    pub rules: Option<String>,
    pub files: Vec<File>,
    pub note: String,
    pub updated: String,
}

pub fn date(text: &str) -> Option<Date> {
    let t = text.trim().trim_start_matches('[').trim_end_matches(']');
    let parts: Vec<Option<u32>> = t.split('-').map(|p| p.trim().parse().ok()).collect();
    match parts[..] {
        [Some(y), Some(m), Some(d)] if (1..=12).contains(&m) && (1..=31).contains(&d) => {
            Some((if y < 100 { 2000 + y as i32 } else { y as i32 }, m, d))
        }
        _ => None,
    }
}

fn is_date(text: &str) -> bool {
    date(text).is_some()
}

fn starts_with_date(text: &str) -> bool {
    text.get(..8).is_some_and(|d| date(d).is_some())
}

pub fn history(html: &str) -> (Vec<(Date, String)>, Option<String>) {
    let html = strip_comments(html);
    let Some(start) = html.find("【変更履歴】") else {
        return (Vec::new(), None);
    };
    let block = &html[start..];
    let end = block.find("過去の変更履歴").unwrap_or(block.len());
    let (block, tail) = block.split_at(end);
    let older = block
        .rfind("<a ")
        .or_else(|| block.to_ascii_lowercase().rfind("<a "))
        .and_then(|i| attribute(&block[i..], "href"));
    let _ = tail;
    let lower = block.to_ascii_lowercase();
    let mut lines = Vec::new();
    let mut at = 0;
    for (i, _) in lower
        .match_indices("<br")
        .chain(std::iter::once((block.len(), "")))
    {
        let line = zlog_net::html_text(&block[at..i])
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        at = block[i..].find('>').map_or(block.len(), |j| i + j + 1);
        let Some(open) = line.rfind('[') else {
            continue;
        };
        let Some(date) = slash_date(line[open..].trim_matches(['[', ']'])) else {
            continue;
        };
        let text = line[..open].trim().to_string();
        if !text.is_empty() {
            lines.push((date, text));
        }
    }
    lines.reverse();
    (lines, older)
}

fn slash_date(text: &str) -> Option<Date> {
    let mut p = text.trim().split('/').map(|x| x.trim().parse::<u32>().ok());
    let (y, m, d) = (p.next()??, p.next()??, p.next()??);
    ((1..=12).contains(&m) && (1..=31).contains(&d) && p.next().is_none())
        .then_some((y as i32, m, d))
}

pub type Date = (i32, u32, u32);

pub fn parse(html: &str) -> Result<Vec<Entry>, String> {
    let html = strip_comments(html);
    let entries: Vec<Entry> = pieces(&html, "tr").into_iter().filter_map(entry).collect();
    if entries.is_empty() {
        Err("no CFG or DAT files found in the page".into())
    } else {
        Ok(entries)
    }
}

fn entry(row: &str) -> Option<Entry> {
    let mut e = Entry::default();
    for cell in pieces(row, "td") {
        let links = links(cell);
        let files: Vec<File> = links.iter().filter_map(file).collect();
        let text = text(cell);
        if !files.is_empty() {
            e.files.extend(files);
        } else if e.date.is_empty() && e.name.is_empty() && starts_with_date(&text) {
            e.date = text;
        } else if is_date(&text) {
            if e.date.is_empty() && e.name.is_empty() {
                e.date = text;
            } else {
                e.updated = text;
            }
        } else if e.name.is_empty() && !text.is_empty() {
            e.name = text;
            e.rules = links
                .into_iter()
                .map(|(href, _)| href)
                .find(|h| h.starts_with("http://") || h.starts_with("https://"));
        } else if !text.is_empty() {
            if !e.note.is_empty() {
                e.note.push(' ');
            }
            e.note.push_str(&text);
        }
    }
    if e.files.is_empty() {
        return None;
    }
    if e.name.is_empty() {
        e.name = e.files[0].name.clone();
    }
    Some(e)
}

fn links(cell: &str) -> Vec<(String, String)> {
    let lower = cell.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = find_tag(&lower[at..], "a") {
        let start = at + i;
        let Some(end) = lower[start..].find("</a").map(|j| start + j) else {
            break;
        };
        let open_end = lower[start..end].find('>').map_or(end, |j| start + j);
        let href = attribute(&cell[start..open_end], "href").unwrap_or_default();
        let close = lower[end..].find('>').map_or(cell.len(), |j| end + j + 1);
        let rest = &cell[close..];
        let next = rest.find('<').unwrap_or(rest.len());
        out.push((href, text(&rest[..next])));
        at = close;
    }
    out
}

fn file((href, after): &(String, String)) -> Option<File> {
    let lower = href.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"))?;
    if !rest.starts_with(&format!("{HOST}/")) {
        return None;
    }
    if !(lower.ends_with(".cfg") || lower.ends_with(".dat")) {
        return None;
    }
    let name = href.rsplit('/').next()?.to_string();
    if name.len() <= 4 {
        return None;
    }
    Some(File {
        name,
        url: href.clone(),
        note: after.clone(),
    })
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let i = lower.find(&format!("{name}="))? + name.len() + 1;
    let v = &tag[i..];
    let (quote, v) = match v.chars().next()? {
        q @ ('"' | '\'') => (Some(q), &v[1..]),
        _ => (None, v),
    };
    let end = match quote {
        Some(q) => v.find(q),
        None => v.find(|c: char| c.is_whitespace() || c == '>'),
    }
    .unwrap_or(v.len());
    Some(v[..end].trim().replace("&amp;", "&"))
}

fn find_tag(lower: &str, name: &str) -> Option<usize> {
    let open = format!("<{name}");
    lower.match_indices(&open).map(|(i, _)| i).find(|&i| {
        lower[i + open.len()..]
            .chars()
            .next()
            .is_some_and(|c| c == '>' || c.is_whitespace())
    })
}

fn pieces<'a>(html: &'a str, name: &str) -> Vec<&'a str> {
    let lower = html.to_ascii_lowercase();
    let mut starts = Vec::new();
    let mut at = 0;
    while let Some(i) = find_tag(&lower[at..], name) {
        starts.push(at + i);
        at += i + 1;
    }
    starts.push(html.len());
    starts.windows(2).map(|w| &html[w[0]..w[1]]).collect()
}

fn strip_comments(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find("<!--") {
        out.push_str(&rest[..i]);
        rest = rest[i..].find("-->").map_or("", |j| &rest[i + j + 3..]);
    }
    out.push_str(rest);
    out
}

fn text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let end = ["</td", "</tr", "</table", "<table"]
        .iter()
        .filter_map(|t| lower.find(t))
        .min()
        .unwrap_or(html.len());
    zlog_net::html_text(&html[..end])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_as_the_page_writes_them() {
        assert_eq!(date("14-12-20"), Some((2014, 12, 20)));
        assert_eq!(date("[2024-01-26]"), Some((2024, 1, 26)));
        assert!(!is_date("オール秋田"));
        assert!(!is_date("26-13-01"));
        assert!(!is_date("1-2"));
        assert!(starts_with_date("26-02-01〜10"));
        assert!(!starts_with_date("オール"));
        assert_eq!(slash_date("2026/2/3"), Some((2026, 2, 3)));
        assert_eq!(slash_date("TNX"), None);
    }

    #[test]
    fn files_only_from_the_site_and_named_by_their_url() {
        let f = |h: &str| file(&(h.to_string(), String::new()));
        let x = f("http://ja6ycu.in.coocan.jp/ZLOG/kcjvu.dat").unwrap();
        assert_eq!(x.name, "kcjvu.dat");
        assert!(f("http://www010.upp.so-net.ne.jp/intruder/zlog/dx/ardx.dat").is_none());
        assert!(f("http://ja6ycu.in.coocan.jp/ZLOG/KT_TIME_HOSOKU.jpg").is_none());
        assert!(f("http://ja6ycu.in.coocan.jp.example.com/a.cfg").is_none());
        assert!(f("https://ja6ycu.in.coocan.jp/ZLOG/.cfg").is_none());
    }
}
