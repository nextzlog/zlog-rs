#[test]
#[ignore]
fn the_real_page() {
    let path = std::env::var("ZLOG_CFG_PAGE").expect("ZLOG_CFG_PAGE");
    let html = std::fs::read_to_string(path).unwrap();
    let l = zlog_ja6ycu::listing(&html).unwrap();
    println!("history page {:?}", l.history_page);
    for c in &l.changes {
        println!("change {:?} {}", c.date, c.text);
    }
    for c in l.contests.iter().filter(|c| !c.changes.is_empty()) {
        println!("contest {} <- {} changes", c.name, c.changes.len());
    }
    let e = zlog_ja6ycu::page::parse(&html).unwrap();
    let files: usize = e.iter().map(|e| e.files.len()).sum();
    for x in &e {
        let f: Vec<String> = x
            .files
            .iter()
            .map(|f| format!("{}{}", f.name, f.note))
            .collect();
        println!(
            "{} | {} | {:?} | {} | {}",
            x.date, x.name, f, x.note, x.updated
        );
    }
    println!("{} rows, {files} files", e.len());
}

use zlog_ja6ycu::page::{parse, Entry};

const PAGE: &str = include_str!("fixtures/page.html");

fn entries() -> Vec<Entry> {
    parse(PAGE).unwrap()
}

#[test]
fn rows_with_files_of_the_site_only() {
    let e = entries();
    let names: Vec<&str> = e.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "サンプル市郡区ファイル",
            "架空VUパーティー",
            "架空県コンテスト",
            "架空訓練コンテスト",
            "架空島コンテスト",
        ]
    );
    let fictvu = &e[1];
    assert_eq!(fictvu.date, "26-01-02");
    assert_eq!(fictvu.updated, "14-12-20");
    assert_eq!(
        fictvu.rules.as_deref(),
        Some("https://example.org/fictvu-rules/")
    );
    assert_eq!(
        fictvu.note,
        "コンテストナンバー欄にラストレターを入力してください。"
    );
    let urls: Vec<&str> = fictvu.files.iter().map(|f| f.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "http://ja6ycu.in.coocan.jp/ZLOG/fictvu.cfg",
            "http://ja6ycu.in.coocan.jp/ZLOG/fictvu.dat"
        ]
    );
    assert_eq!(
        (e[0].date.as_str(), e[0].updated.as_str()),
        ("", "[2024-01-26]")
    );
}

#[test]
fn several_files_a_cell_keep_their_notes() {
    let files: Vec<(String, String)> = entries()[2]
        .files
        .iter()
        .map(|f| (f.name.clone(), f.note.clone()))
        .collect();
    let want = [
        ("fictin.cfg", "（県内局）"),
        ("fictout.cfg", "（県外局）"),
        ("fictin.dat", "（県内局）"),
        ("fictout.dat", "（県外局）"),
    ]
    .map(|(a, b)| (a.to_string(), b.to_string()));
    assert_eq!(files, want);
}

#[test]
fn files_are_named_by_their_link_and_comments_are_skipped() {
    let e = entries();
    assert_eq!(e[3].files[1].name, "sharedvu.dat");
    assert_eq!(
        e[4].rules.as_deref(),
        Some("https://example.org/island.pdf")
    );
}

#[test]
fn a_page_without_files_is_an_error() {
    assert!(parse("<html><body><p>moved</p></body></html>").is_err());
}

#[test]
fn listed_contests_carry_dates_and_plain_notes() {
    let c = zlog_ja6ycu::listing(PAGE).unwrap().contests;
    assert_eq!(c[1].updated, Some((2014, 12, 20)));
    assert_eq!(c[2].files[0].note, "県内局");
    assert_eq!(c[0].updated, Some((2024, 1, 26)));
}

#[test]
fn history_lines_newest_first_and_matched_to_contests() {
    let l = zlog_ja6ycu::listing(PAGE).unwrap();
    let texts: Vec<&str> = l.changes.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "架空県コンテスト CFGファイル変更 TNX JA1ZLO",
            "架空VUパーティー DATファイル変更",
        ]
    );
    assert_eq!(l.changes[0].date, Some((2026, 10, 10)));
    assert_eq!(
        l.history_page.as_deref(),
        Some("http://ja6ycu.in.coocan.jp/ZLOG/history.html")
    );
    assert_eq!(l.contests[2].changes.len(), 1);
    assert_eq!(l.contests[1].changes[0].date, Some((2026, 2, 3)));
}
