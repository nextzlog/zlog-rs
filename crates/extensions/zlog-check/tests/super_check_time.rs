use std::time::{Duration, Instant};
use zlog_check::matching::Database;

fn letter(n: usize) -> char {
    (b'A' + (n % 26) as u8) as char
}

fn database() -> Database {
    let records = (0..50_000usize).map(|i| {
        let call = match i % 4 {
            0 => format!(
                "JA{}{}{}{}",
                i % 10,
                letter(i / 10),
                letter(i / 260),
                letter(i / 6760)
            ),
            1 => format!("W{}{}{}", i % 10, letter(i / 7), letter(i / 182)),
            2 => format!(
                "{}{}{}{}{}{}/P",
                letter(i),
                letter(i / 26),
                i % 10,
                letter(i / 3),
                letter(i / 70),
                letter(i / 1900)
            ),
            _ => format!(
                "{}{}{}{}{}",
                letter(i / 5),
                letter(i / 130),
                i % 10,
                letter(i / 11),
                letter(i / 300)
            ),
        };
        (call, String::new())
    });
    Database::new(records)
}

fn reference(db: &Database, fragment: &str) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize, f64)> = (0..db.len())
        .filter_map(|i| {
            let call = &db.entry(i).callsign;
            let longer = call.len().max(fragment.len()) as f64;
            let d = strsim::levenshtein(call, fragment);
            (d as f64 / longer < 0.3).then(|| (call.clone(), d, d as f64 / longer))
        })
        .collect();
    out.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
    out.into_iter().map(|(c, d, _)| (c, d)).collect()
}

#[test]
fn n_plus_one_on_50k_calls_is_fast_and_unchanged() {
    let db = database();
    let fragments = [
        "JA1",
        "JA1A",
        "JA1AB",
        "JA1ABA",
        "W1AB",
        "AB1CDE/P",
        "XY9ZZZZZZ",
        "JA1ABAQQ",
    ];
    let mut worst = Duration::ZERO;
    for f in fragments {
        let mut best = Duration::MAX;
        let mut hits = Vec::new();
        for _ in 0..5 {
            let t = Instant::now();
            hits = db.n_plus_one(f, usize::MAX);
            best = best.min(t.elapsed());
        }
        worst = worst.max(best);
        let got: Vec<(String, usize)> = hits
            .iter()
            .map(|h| (db.entry(h.index).callsign.clone(), h.distance))
            .collect();
        assert_eq!(got, reference(&db, f), "{f}");
    }
    eprintln!("slowest N+1 on {} calls: {worst:?}", db.len());
    if !cfg!(debug_assertions) {
        assert!(worst < Duration::from_millis(3), "{worst:?}");
    }
}

#[test]
fn a_one_letter_partial_list_stops_at_the_limit() {
    let db = database();
    let mut best = Duration::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        assert_eq!(db.lines("W", 100).len(), 100);
        best = best.min(t.elapsed());
    }
    eprintln!(
        "partial list for one letter on {} calls: {best:?}",
        db.len()
    );
    if !cfg!(debug_assertions) {
        assert!(best < Duration::from_millis(1), "{best:?}");
    }
}
