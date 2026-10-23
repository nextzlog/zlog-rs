/// The real page, when saved locally: `ZLOG_CFG_PAGE=index.html cargo test -p
/// zlog-ja6ycu -- --ignored --nocapture`.
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
            "全市全郡全区ファイル",
            "VUオール秋田QSOパーティー",
            "オール熊本コンテスト",
            "富山県非常無線通信訓練コンテスト",
            "長崎県コンテスト",
        ]
    );
    let akita = &e[1];
    assert_eq!(akita.date, "26-01-02");
    assert_eq!(akita.updated, "14-12-20");
    assert_eq!(
        akita.rules.as_deref(),
        Some("https://example.org/akita-rules/")
    );
    assert_eq!(
        akita.note,
        "コンテストナンバー欄にラストレターを入力してください。"
    );
    let urls: Vec<&str> = akita.files.iter().map(|f| f.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "http://ja6ycu.in.coocan.jp/ZLOG/akitavu.cfg",
            "http://ja6ycu.in.coocan.jp/ZLOG/akitavu.dat"
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
        ("kumamotoi.cfg", "（県内局）"),
        ("kumamotoo.cfg", "（県外局）"),
        ("kumamotoi.dat", "（県内局）"),
        ("kumamotoo.dat", "（県外局）"),
    ]
    .map(|(a, b)| (a.to_string(), b.to_string()));
    assert_eq!(files, want);
}

#[test]
fn files_are_named_by_their_link_and_comments_are_skipped() {
    let e = entries();
    assert_eq!(e[3].files[1].name, "kcjvu.dat");
    assert_eq!(
        e[4].rules.as_deref(),
        Some("https://example.org/nagasaki.pdf")
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
            "オール熊本コンテスト CFGファイル変更 TNX JA6ZZZ",
            "VUオール秋田QSOパーティー DATファイル変更",
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
