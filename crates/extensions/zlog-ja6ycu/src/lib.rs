pub mod page;

use std::path::{Path, PathBuf};

use zlog_ui::{Change, ContestList, Date, ListedContest, ListedFile};

zlog_core::texts!();

pub const PAGE_URL: &str = "https://ja6ycu.in.coocan.jp/ZLOG/index.html";

pub const RECENT_DAYS: i64 = 31;

pub fn outdated_on(today: Date, updated: Option<Date>, changed: Date) -> bool {
    let day = |(y, m, d): Date| chrono::NaiveDate::from_ymd_opt(y, m, d);
    let (Some(today), Some(updated), Some(changed)) =
        (day(today), updated.and_then(day), day(changed))
    else {
        return false;
    };
    (today - updated).num_days() <= RECENT_DAYS && changed < updated
}

pub struct Listing {
    pub contests: Vec<ListedContest>,
    pub changes: Vec<Change>,
    pub history_page: Option<String>,
}

pub fn listing(html: &str) -> Result<Listing, String> {
    let (history, history_page) = page::history(html);
    let changes: Vec<Change> = history
        .into_iter()
        .map(|(date, text)| Change {
            date: Some(date),
            text,
        })
        .collect();
    let squeeze = |s: &str| s.split_whitespace().collect::<String>();
    let contests = page::parse(html)?
        .into_iter()
        .map(|e| {
            let name = squeeze(&e.name);
            ListedContest {
                updated: page::date(&e.updated),
                changes: changes
                    .iter()
                    .filter(|c| !name.is_empty() && squeeze(&c.text).contains(&name))
                    .cloned()
                    .collect(),
                name: e.name,
                date: e.date,
                rules: e.rules,
                files: e
                    .files
                    .into_iter()
                    .map(|f| ListedFile {
                        name: f.name,
                        url: f.url,
                        note: f.note.trim_matches(['（', '）', '(', ')']).to_string(),
                    })
                    .collect(),
                note: e.note,
                ..ListedContest::default()
            }
        })
        .collect();
    Ok(Listing {
        contests,
        changes,
        history_page,
    })
}

pub struct Ja6ycu {
    fetch: Option<zlog_net::Fetch>,
    list: Option<Result<Listing, String>>,
    cache: Option<PathBuf>,
    loaded: bool,
    checked: Option<Date>,
}

impl Default for Ja6ycu {
    fn default() -> Self {
        Ja6ycu::with_cache(dirs::cache_dir().map(|d| d.join(zlog_ui::APP_DIR).join("ja6ycu.html")))
    }
}

fn changed(path: &Path) -> Option<Date> {
    use chrono::Datelike;
    let t: chrono::DateTime<chrono::Local> = std::fs::metadata(path).ok()?.modified().ok()?.into();
    Some((t.year(), t.month(), t.day()))
}

impl Ja6ycu {
    pub fn with_cache(cache: Option<PathBuf>) -> Self {
        Ja6ycu {
            fetch: None,
            list: None,
            cache,
            loaded: false,
            checked: None,
        }
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(path) = &self.cache else {
            return;
        };
        let Ok(html) = std::fs::read_to_string(path) else {
            return;
        };
        if let Ok(l) = listing(&html) {
            self.checked = changed(path);
            self.list = Some(Ok(l));
        }
    }

    fn fetched(&mut self, r: Result<String, String>) {
        let read = r.and_then(|html| listing(&html).map(|l| (html, l)));
        match read {
            Ok((html, l)) => {
                if let Some(path) = &self.cache {
                    let _ = path
                        .parent()
                        .map(std::fs::create_dir_all)
                        .transpose()
                        .and_then(|_| std::fs::write(path, html));
                }
                use chrono::Datelike;
                let t = chrono::Local::now();
                self.checked = Some((t.year(), t.month(), t.day()));
                self.list = Some(Ok(l));
            }
            Err(e) if !matches!(self.list, Some(Ok(_))) => self.list = Some(Err(e)),
            Err(_) => {}
        }
    }
}

impl ContestList for Ja6ycu {
    fn id(&self) -> &str {
        "ja6ycu"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn page(&self) -> &str {
        PAGE_URL
    }

    fn changes(&self) -> &[Change] {
        match &self.list {
            Some(Ok(l)) => &l.changes,
            _ => &[],
        }
    }

    fn history_page(&self) -> Option<&str> {
        match &self.list {
            Some(Ok(l)) => l.history_page.as_deref(),
            _ => None,
        }
    }

    fn last_checked(&self) -> Option<Date> {
        self.checked
    }

    fn stale(&self, today: Date) -> bool {
        let checked = self.checked.or_else(|| {
            let path = self.cache.as_ref()?;
            (!self.loaded).then(|| changed(path)).flatten()
        });
        checked.is_none_or(|d| zlog_ui::days_between(d, today) > zlog_ui::LIST_STALE_DAYS)
    }

    fn refresh(&mut self) {
        self.load();
        if !matches!(self.list, Some(Ok(_))) {
            self.list = None;
        }
        self.fetch = Some(zlog_net::Fetch::get(PAGE_URL));
    }

    fn outdated(
        &self,
        _folder: &std::path::Path,
        contest: &ListedContest,
        _file: &ListedFile,
        changed: Date,
    ) -> bool {
        use chrono::Datelike;
        let t = chrono::Local::now();
        outdated_on((t.year(), t.month(), t.day()), contest.updated, changed)
    }

    fn contests(&mut self) -> Option<Result<&[ListedContest], String>> {
        self.load();
        if let Some(r) = self.fetch.as_mut().and_then(|f| f.poll().cloned()) {
            self.fetch = None;
            self.fetched(r);
        }
        match self.list.as_ref()? {
            Ok(l) => Some(Ok(&l.contests)),
            Err(e) => Some(Err(e.clone())),
        }
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.contest_list(10, Ja6ycu::default);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_recent_updates_after_the_file_are_offered() {
        let today = (2026, 10, 22);
        assert!(outdated_on(today, Some((2026, 10, 10)), (2025, 1, 1)));
        assert!(!outdated_on(today, Some((2026, 10, 10)), (2026, 10, 11)));
        assert!(!outdated_on(today, Some((2026, 8, 1)), (2025, 1, 1)));
        assert!(!outdated_on(today, None, (2025, 1, 1)));
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zlog-ja6ycu-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("ja6ycu.html")
    }

    const PAGE: &str = include_str!("../tests/fixtures/page.html");

    #[test]
    fn the_copy_is_shown_without_fetching_and_kept_on_failure() {
        let path = scratch("copy");
        let mut l = Ja6ycu::with_cache(Some(path.clone()));
        assert!(l.stale((2026, 10, 24)), "never fetched");
        assert!(l.contests().is_none());
        l.fetched(Ok(PAGE.to_string()));
        let n = l.contests().unwrap().unwrap().len();
        assert!(n > 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), PAGE);
        l.fetched(Err("offline".into()));
        assert_eq!(l.contests().unwrap().unwrap().len(), n);
        let mut again = Ja6ycu::with_cache(Some(path));
        let today = changed_today();
        assert!(!again.stale(today));
        assert_eq!(again.contests().unwrap().unwrap().len(), n);
        assert_eq!(again.last_checked(), Some(today));
        let (y, m, d) = today;
        assert!(again.stale((y + 1, m, d)));
    }

    fn changed_today() -> Date {
        use chrono::Datelike;
        let t = chrono::Local::now();
        (t.year(), t.month(), t.day())
    }

    #[test]
    fn without_a_copy_a_failure_is_shown() {
        let mut l = Ja6ycu::with_cache(None);
        l.fetched(Err("offline".into()));
        assert_eq!(l.contests().unwrap().err().as_deref(), Some("offline"));
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
