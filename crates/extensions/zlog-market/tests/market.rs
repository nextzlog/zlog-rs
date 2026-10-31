use std::path::PathBuf;

use std::sync::atomic::{AtomicUsize, Ordering};

use zlog_market::{checksum, md5_entries, md5_for, md5_hex, parse, write_atomic, MarketList, Sum};
use zlog_ui::ContestList;

const MARKET: &str = include_str!("fixtures/market.json");

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("zlog-market-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn offline(_: zlog_net::Request) -> zlog_net::Exchange {
    zlog_net::Exchange::ready(Err("offline".into()))
}

fn served(_: zlog_net::Request) -> zlog_net::Exchange {
    zlog_net::Exchange::ready(Ok(zlog_net::Reply {
        status: 200,
        body: MARKET.into(),
        headers: vec![("etag".into(), "\"m1\"".into())],
        ..Default::default()
    }))
}

fn unchanged(r: zlog_net::Request) -> zlog_net::Exchange {
    let etag = r
        .headers
        .iter()
        .any(|(n, v)| n == "If-None-Match" && v == "\"m1\"");
    zlog_net::Exchange::ready(match etag {
        true => Ok(zlog_net::Reply {
            status: 304,
            ..Default::default()
        }),
        false => Err("no validator sent".into()),
    })
}

fn names_of(files: &[zlog_ui::ListedFile]) -> Vec<&str> {
    files.iter().map(|f| f.name.as_str()).collect()
}

#[test]
fn sections_of_the_fixture() {
    let m = parse(MARKET).unwrap();
    assert_eq!(m.files["cfg"].len(), 7);
    assert_eq!(m.files["dat"].len(), 2);
    assert_eq!(m.files["txt"].len(), 1);
    assert_eq!(m.pkg["sample-a"].tag, "Sample A");
    assert!(m.pkg["sample-a"].doc.starts_with("#### Install"));
    assert!(m.pkg["sample-a"].stable);
    assert!(!m.pkg["sample-unstable"].stable);
    assert_eq!(
        m.files["cfg"]["sample-a"].sum,
        Sum::Hex("7cbe79dcdda139705668dd4d1759f7f1".into())
    );
    assert_eq!(
        m.files["cfg"]["sample-unstable"].sum,
        Sum::Url("https://example.com/sample-unstable/sample-unstable.cfg.md5".into())
    );
    assert_eq!(m.files["cfg"]["unused"].sum, Sum::Missing);
}

#[test]
fn one_row_per_package_with_a_cfg_and_all_its_files_but_dlls() {
    let l = parse(MARKET).unwrap().listing(|_| None, "none");
    let names: Vec<&str> = l.contests.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(l.contests.len(), 6, "{names:?}");
    assert!(
        !names.contains(&"DLL Only"),
        "DLL-only packages are left out"
    );
    assert_eq!(names[4..], ["Sample Unstable", "unused"], "{names:?}");
    let hs = l.contests.iter().find(|c| c.name == "Sample A").unwrap();
    assert_eq!(names_of(&hs.files), ["sample-a.cfg"]);
    assert_eq!(hs.rules.as_deref(), Some("https://example.com/sample-a"));
    assert!(hs.note.contains("Sample A") && !hs.doc.is_empty());
    let cw = l.contests.iter().find(|c| c.name == "Drill").unwrap();
    assert_eq!(names_of(&cw.files), ["drill.cfg", "drill.dat", "calls.txt"]);
    let isb = l
        .contests
        .iter()
        .find(|c| c.name == "Shared Sections")
        .unwrap();
    assert_eq!(
        names_of(&isb.files),
        ["shared.dat", "shared-in.cfg", "shared-out.cfg"]
    );
    let exp: Vec<&str> = l
        .contests
        .iter()
        .filter(|c| c.experimental)
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(exp, ["Sample Unstable"]);
}

const TOML_STYLE: &str = r#"{
  "pkg": {
    "beta": {"tag": "Beta", "doc": "https://example.org/beta.md", "use": ["cfg.beta", "dll.beta"], "exp": "unstable"},
    "alpha": {"tag": "Alpha", "use": ["cfg.alpha", "dat.alpha"], "exp": "stable"}
  },
  "cfg": {
    "beta": {"url": "https://example.org/beta.cfg", "sum": "https://example.org/beta.cfg.md5"},
    "alpha": {"url": "https://example.org/alpha.cfg"},
    "lone": {"url": "https://example.org/lone.cfg", "sum": "00000000000000000000000000000000"}
  },
  "dat": {"alpha": {"url": "https://example.org/alpha.dat", "sum": "11111111111111111111111111111111"}},
  "dll": {"beta": {"url": "https://example.org/beta.dll", "sum": "https://example.org/beta.dll.md5"}}
}"#;

#[test]
fn toml_style_entries_with_sum_urls_doc_urls_and_unstable_packages() {
    let m = parse(TOML_STYLE).unwrap();
    assert_eq!(
        m.files["cfg"]["beta"].sum,
        Sum::Url("https://example.org/beta.cfg.md5".into())
    );
    assert_eq!(m.files["cfg"]["alpha"].sum, Sum::Missing);
    let l = m.listing(|_| None, "no sum");
    let names: Vec<&str> = l.contests.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        ["Alpha", "Beta", "lone"],
        "stable first, unused cfgs last"
    );
    assert!(l.contests[1].experimental);
    assert_eq!(l.contests[1].doc, "https://example.org/beta.md");
    assert_eq!(names_of(&l.contests[1].files), ["beta.cfg"]);
    assert_eq!(l.contests[0].files[0].note, "no sum");
    assert_eq!(l.contests[0].files[1].note, "");
}

fn md5_server(r: zlog_net::Request) -> zlog_net::Exchange {
    let body = match r.url.as_str() {
        "https://example.org/beta.cfg.md5" => format!("{}  beta.cfg\n", checksum(b"beta")),
        _ => TOML_STYLE.to_string(),
    };
    zlog_net::Exchange::ready(Ok(zlog_net::Reply {
        status: 200,
        body: body.into_bytes(),
        ..Default::default()
    }))
}

#[test]
fn a_sum_url_is_read_on_demand_and_used_for_checks() {
    let folder = scratch("sum-url");
    let mut l = MarketList::new(None, folder.clone(), md5_server);
    l.refresh();
    let beta = l.contests().unwrap().unwrap()[1].clone();
    assert_eq!(l.sum(&beta.files[0].url), Some(checksum(b"beta").as_str()));
    assert!(l.verify(&beta.files[0], b"beta", "en").is_ok());
    assert!(l.verify(&beta.files[0], b"other", "en").is_err());
    std::fs::write(folder.join("beta.cfg"), "old").unwrap();
    assert!(l.outdated(&folder, &beta, &beta.files[0], (2026, 1, 1)));
    let alpha = l.contests().unwrap().unwrap()[0].clone();
    std::fs::write(folder.join("alpha.cfg"), "anything").unwrap();
    assert!(
        l.verify(&alpha.files[0], b"x", "en").is_ok(),
        "no sum: installed"
    );
    assert!(
        !l.outdated(&folder, &alpha, &alpha.files[0], (2026, 1, 1)),
        "no sum: no check"
    );
}

#[test]
fn bad_entries_are_tolerated() {
    let l = parse("{}").unwrap().listing(|_| None, "");
    assert!(l.contests.is_empty());
    let m = parse(r#"{"cfg":{"x":{"sum":"zz","url":"https://x/x.cfg"},"y":{"url":1}},"pkg":3}"#);
    let m = m.unwrap();
    assert_eq!(m.files["cfg"].len(), 1);
    assert_eq!(m.files["cfg"]["x"].sum, Sum::Missing);
}

#[test]
fn bad_json_is_an_error() {
    assert!(parse("{").is_err());
    assert!(parse("[]").is_err());
    assert!(parse("").is_err());
}

fn listed(l: &mut MarketList) -> zlog_ui::ListedContest {
    l.contests()
        .unwrap()
        .unwrap()
        .iter()
        .find(|c| c.name == "Sample A")
        .unwrap()
        .clone()
}

#[test]
fn outdated_only_when_the_local_copy_differs() {
    let folder = scratch("outdated");
    let mut l = MarketList::new(None, folder.clone(), served);
    l.refresh();
    let hs = listed(&mut l);
    assert!(
        !l.outdated(&folder, &hs, &hs.files[0], (2026, 1, 1)),
        "missing"
    );
    std::fs::write(folder.join("SAMPLE-A.CFG"), "old").unwrap();
    assert!(l.outdated(&folder, &hs, &hs.files[0], (2026, 1, 1)));
    std::fs::remove_file(folder.join("SAMPLE-A.CFG")).unwrap();
    let sum = l.sum(&hs.files[0].url).unwrap().to_string();
    let cache = scratch("outdated-cache").join("market.json");
    std::fs::write(&cache, MARKET.replace(&sum, &checksum(b"same"))).unwrap();
    std::fs::write(folder.join("sample-a.cfg"), "same").unwrap();
    let mut m = MarketList::new(Some(cache), folder.clone(), offline);
    let hs = listed(&mut m);
    assert!(!m.outdated(&folder, &hs, &hs.files[0], (2026, 1, 1)));
}

#[test]
fn the_list_verifies_downloads_by_their_sum() {
    let mut l = MarketList::new(None, scratch("verify"), served);
    l.refresh();
    let hs = listed(&mut l);
    let e = l.verify(&hs.files[0], b"data", "en").unwrap_err();
    assert!(e.contains("sample-a.cfg"));
    let e = l.verify(&hs.files[0], b"data", "ja").unwrap_err();
    assert!(e.contains("保存しません"));
    let unknown = zlog_ui::ListedFile {
        name: "other.cfg".into(),
        ..Default::default()
    };
    assert!(l.verify(&unknown, b"data", "en").is_err());
}

#[test]
fn the_cache_is_written_atomically() {
    let folder = scratch("atomic");
    write_atomic(&folder.join("sub").join("a.json"), b"new").unwrap();
    assert_eq!(std::fs::read(folder.join("sub/a.json")).unwrap(), b"new");
    let blocked = folder.join("dir");
    std::fs::create_dir_all(blocked.join("inner")).unwrap();
    assert!(write_atomic(&blocked, b"x").is_err());
    assert_eq!(std::fs::read_dir(folder.join("sub")).unwrap().count(), 1);
}

#[test]
fn a_second_check_asks_with_the_etag_and_keeps_the_copy_on_304() {
    let cache = scratch("etag").join("market.json");
    let mut l = MarketList::new(Some(cache.clone()), scratch("etag-cfg"), served);
    l.refresh();
    assert_eq!(l.contests().unwrap().unwrap().len(), 6);
    let mut again = MarketList::new(Some(cache.clone()), scratch("etag-cfg2"), unchanged);
    again.refresh();
    assert_eq!(again.contests().unwrap().unwrap().len(), 6);
    assert_eq!(std::fs::read_to_string(&cache).unwrap(), MARKET);
    let mut fresh = MarketList::new(None, scratch("etag-cfg3"), unchanged);
    fresh.refresh();
    assert_eq!(
        fresh.contests().unwrap().err().as_deref(),
        Some("no validator sent"),
        "without a copy the request is not conditional"
    );
}

#[test]
fn the_cached_copy_is_kept_when_offline() {
    let cache = scratch("cache").join("market.json");
    let mut l = MarketList::new(Some(cache.clone()), scratch("cache-cfg"), served);
    assert!(l.contests().is_none(), "no network before refresh");
    l.refresh();
    assert_eq!(l.contests().unwrap().unwrap().len(), 6);
    assert_eq!(std::fs::read_to_string(&cache).unwrap(), MARKET);
    let mut again = MarketList::new(Some(cache), scratch("cache-cfg2"), offline);
    again.refresh();
    assert_eq!(again.contests().unwrap().unwrap().len(), 6);
    assert!(again.last_checked().is_some());
}

#[test]
fn without_a_copy_a_failure_is_shown() {
    let mut l = MarketList::new(None, scratch("fail"), offline);
    l.refresh();
    assert_eq!(l.contests().unwrap().err().as_deref(), Some("offline"));
}

fn md5_missing(r: zlog_net::Request) -> zlog_net::Exchange {
    let (status, body) = match r.url.ends_with(".md5") {
        true => (404, String::new()),
        false => (200, TOML_STYLE.to_string()),
    };
    zlog_net::Exchange::ready(Ok(zlog_net::Reply {
        status,
        body: body.into_bytes(),
        ..Default::default()
    }))
}

#[test]
fn an_unreadable_sum_url_refuses_the_download() {
    let mut l = MarketList::new(None, scratch("sum-404"), md5_missing);
    l.refresh();
    let beta = l.contests().unwrap().unwrap()[1].clone();
    let e = l.verify(&beta.files[0], b"beta", "ja").unwrap_err();
    assert!(e.contains("beta.cfg") && e.contains("チェックサム"), "{e}");
}

const SUM_URL: &str = r#"{"pkg":{"beta":{"use":["cfg.beta"]}},
 "cfg":{"beta":{"url":"https://example.org/beta.cfg","sum":"https://example.org/beta.cfg.md5"}}}"#;

fn reply(status: u16, body: String) -> zlog_net::Exchange {
    zlog_net::Exchange::ready(Ok(zlog_net::Reply {
        status,
        body: body.into_bytes(),
        ..Default::default()
    }))
}

static FLAKY: AtomicUsize = AtomicUsize::new(0);

fn flaky(r: zlog_net::Request) -> zlog_net::Exchange {
    match r.url.ends_with(".md5") {
        true if FLAKY.fetch_add(1, Ordering::SeqCst) == 0 => {
            zlog_net::Exchange::ready(Err("timeout".into()))
        }
        true => reply(200, checksum(b"beta")),
        false => reply(200, SUM_URL.into()),
    }
}

#[test]
fn a_failed_sum_fetch_is_retried_on_refresh() {
    let mut l = MarketList::new(None, scratch("md5-retry"), flaky);
    l.refresh();
    let beta = l.contests().unwrap().unwrap()[0].clone();
    assert!(l.verify(&beta.files[0], b"beta", "en").is_err());
    l.refresh();
    let _ = l.contests();
    assert!(l.verify(&beta.files[0], b"beta", "en").is_ok());
}

static VERSION: AtomicUsize = AtomicUsize::new(0);

fn changing(r: zlog_net::Request) -> zlog_net::Exchange {
    match r.url.ends_with(".md5") {
        true if VERSION.load(Ordering::SeqCst) == 0 => reply(200, checksum(b"v1")),
        true => reply(200, checksum(b"v2")),
        false => reply(200, SUM_URL.into()),
    }
}

#[test]
fn sums_are_read_again_after_a_refresh() {
    let mut l = MarketList::new(None, scratch("md5-again"), changing);
    l.refresh();
    let beta = l.contests().unwrap().unwrap()[0].clone();
    assert!(l.verify(&beta.files[0], b"v1", "en").is_ok());
    VERSION.store(1, Ordering::SeqCst);
    l.refresh();
    let _ = l.contests();
    assert!(l.verify(&beta.files[0], b"v2", "en").is_ok());
}

#[test]
fn files_with_the_same_name_keep_their_own_sums() {
    let j = r#"{"pkg":{"a":{"use":["cfg.a","dat.a"]},"b":{"use":["cfg.b","dat.b"]}},
     "cfg":{"a":{"url":"https://x/a.cfg"},"b":{"url":"https://x/b.cfg"}},
     "dat":{"a":{"url":"https://x/a/zlog.dat","sum":"11111111111111111111111111111111"},
            "b":{"url":"https://x/b/zlog.dat","sum":"22222222222222222222222222222222"}}}"#;
    let l = parse(j).unwrap().listing(|_| None, "");
    assert_eq!(l.sums["https://x/a/zlog.dat"], Sum::Hex("1".repeat(32)));
    assert_eq!(l.sums["https://x/b/zlog.dat"], Sum::Hex("2".repeat(32)));
}

#[test]
fn sum_files_in_common_formats() {
    let hex = "0123456789abcdef0123456789abcdef";
    for text in [
        format!("\u{feff}{hex}  a.cfg\n"),
        format!("MD5 (a.cfg) = {}\n", hex.to_uppercase()),
        format!("{hex} *a.cfg"),
    ] {
        assert_eq!(md5_hex(&text).as_deref(), Some(hex), "{text}");
    }
    assert_eq!(md5_hex("MD5 (a.cfg) = 123"), None);
}

const REFERENCED: &str = r#"{"pkg":{"p":{"use":["cfg.p"]}},
 "cfg":{"p":{"url":"https://x/p.cfg"}},
 "dat":{"d":{"url":"https://x/p.dat","sum":"11111111111111111111111111111111"}}}"#;

fn referenced(_: zlog_net::Request) -> zlog_net::Exchange {
    reply(200, REFERENCED.into())
}

#[test]
fn the_chosen_folder_decides_the_referenced_dat() {
    let mut l = MarketList::new(None, scratch("default-folder"), referenced);
    l.refresh();
    let p = l.contests().unwrap().unwrap()[0].clone();
    assert_eq!(names_of(&p.files), ["p.cfg"]);
    let chosen = scratch("chosen-folder");
    std::fs::write(chosen.join("p.cfg"), "DAT p.dat\n").unwrap();
    std::fs::write(chosen.join("p.dat"), "old").unwrap();
    let _ = l.outdated(&chosen, &p, &p.files[0], (2026, 1, 1));
    let p = l.contests().unwrap().unwrap()[0].clone();
    assert_eq!(names_of(&p.files), ["p.cfg", "p.dat"]);
    assert!(l.outdated(&chosen, &p, &p.files[1], (2026, 1, 1)));
}

static REQUESTS: AtomicUsize = AtomicUsize::new(0);

fn counted(_: zlog_net::Request) -> zlog_net::Exchange {
    REQUESTS.fetch_add(1, Ordering::SeqCst);
    reply(200, REFERENCED.into())
}

#[test]
fn one_request_at_a_time_and_not_too_often() {
    let mut l = MarketList::new(None, scratch("in-flight"), counted);
    assert!(l.stale((2026, 1, 1)));
    l.refresh();
    l.refresh();
    assert_eq!(REQUESTS.load(Ordering::SeqCst), 1);
    assert!(l.contests().unwrap().is_ok());
    assert!(!l.stale((2026, 1, 1)));
    l.refresh();
    assert_eq!(REQUESTS.load(Ordering::SeqCst), 2);
}

#[test]
fn a_multi_file_sum_picks_our_line_and_html_is_no_sum() {
    let e = md5_entries(&format!(
        "{}  a.dat\n{} *dir/b.dat\n",
        "1".repeat(32),
        "2".repeat(32)
    ))
    .unwrap();
    assert_eq!(
        md5_for(&e, "https://x/b.dat"),
        Some("2".repeat(32).as_str())
    );
    assert_eq!(md5_for(&e, "https://x/c.dat"), None);
    let e = md5_entries(&format!("MD5 (b.dat) = {}\n", "3".repeat(32))).unwrap();
    assert_eq!(
        md5_for(&e, "https://x/other.dat"),
        Some("3".repeat(32).as_str())
    );
    assert!(md5_entries(&format!("<html>{}</html>", "4".repeat(32))).is_none());
}

fn html(r: zlog_net::Request) -> zlog_net::Exchange {
    match r.url.ends_with(".md5") {
        true => zlog_net::Exchange::ready(Ok(zlog_net::Reply {
            status: 200,
            content_type: "Text/HTML; charset=utf-8".into(),
            body: checksum(b"beta").into_bytes(),
            ..Default::default()
        })),
        false => reply(200, SUM_URL.into()),
    }
}

#[test]
fn an_html_page_is_not_a_sum() {
    let mut l = MarketList::new(None, scratch("md5-html"), html);
    l.refresh();
    let beta = l.contests().unwrap().unwrap()[0].clone();
    assert_eq!(l.sum(&beta.files[0].url), None);
}

static SLOW: AtomicUsize = AtomicUsize::new(0);

fn slow(r: zlog_net::Request) -> zlog_net::Exchange {
    match r.url.ends_with(".md5") {
        true if SLOW.fetch_add(1, Ordering::SeqCst) == 0 => reply(200, checksum(b"beta")),
        true => zlog_net::Exchange::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(5));
            Err("slow".into())
        }),
        false => reply(200, SUM_URL.into()),
    }
}

#[test]
fn old_sums_stay_until_new_ones_arrive() {
    let mut l = MarketList::new(None, scratch("md5-keep"), slow);
    l.refresh();
    let beta = l.contests().unwrap().unwrap()[0].clone();
    l.refresh();
    let _ = l.contests();
    assert!(l.verify(&beta.files[0], b"beta", "en").is_ok());
}

#[test]
fn a_file_missing_from_the_listing_is_refused() {
    let mut l = MarketList::new(None, scratch("unlisted"), md5_server);
    l.refresh();
    let mut beta = l.contests().unwrap().unwrap()[1].files[0].clone();
    beta.url = "https://example.org/v2/beta.cfg".into();
    assert!(l.verify(&beta, b"beta", "en").is_err());
}
