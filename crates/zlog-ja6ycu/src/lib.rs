//! JA6YCU's list of CFG and DAT files for zLog
//! (<https://ja6ycu.in.coocan.jp/ZLOG/index.html>), offered to the user-defined
//! contest window as a [`ContestList`].
//!
//! Only the page is fetched here, when the window first asks and on Refresh; the
//! files themselves are downloaded in the browser.

pub mod page;

use zlog_ui::{Change, ContestList, Date, ListedContest, ListedFile};

zlog_core::texts!();

/// The list page.
pub const PAGE_URL: &str = "https://ja6ycu.in.coocan.jp/ZLOG/index.html";

/// How long after the page's update date a file is offered again.
pub const RECENT_DAYS: i64 = 31;

/// Whether a file changed on `changed` should be fetched again on `today`: the
/// page updated the contest's files within [`RECENT_DAYS`], after the file was
/// last changed. Older updates are not offered, since the page dates rows only
/// to the day and many files were changed years ago.
pub fn outdated_on(today: Date, updated: Option<Date>, changed: Date) -> bool {
    let day = |(y, m, d): Date| chrono::NaiveDate::from_ymd_opt(y, m, d);
    let (Some(today), Some(updated), Some(changed)) =
        (day(today), updated.and_then(day), day(changed))
    else {
        return false;
    };
    (today - updated).num_days() <= RECENT_DAYS && changed < updated
}

/// What the page offers: its contests (each with the changes that name it), the
/// change history and the page of older changes.
pub struct Listing {
    pub contests: Vec<ListedContest>,
    pub changes: Vec<Change>,
    pub history_page: Option<String>,
}

/// Read the page.
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
            }
        })
        .collect();
    Ok(Listing {
        contests,
        changes,
        history_page,
    })
}

#[derive(Default)]
pub struct Ja6ycu {
    fetch: Option<zlog_net::Fetch>,
    list: Option<Result<Listing, String>>,
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

    fn refresh(&mut self) {
        self.list = None;
        self.fetch = Some(zlog_net::Fetch::get(PAGE_URL));
    }

    fn outdated(&self, contest: &ListedContest, _file: &ListedFile, changed: Date) -> bool {
        use chrono::Datelike;
        let t = chrono::Local::now();
        outdated_on((t.year(), t.month(), t.day()), contest.updated, changed)
    }

    fn contests(&mut self) -> Option<Result<&[ListedContest], String>> {
        if let Some(r) = self.fetch.as_mut().and_then(|f| f.poll().cloned()) {
            self.list = Some(r.and_then(|html| listing(&html)));
            self.fetch = None;
        }
        match self.list.as_ref()? {
            Ok(l) => Some(Ok(&l.contests)),
            Err(e) => Some(Err(e.clone())),
        }
    }
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

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
