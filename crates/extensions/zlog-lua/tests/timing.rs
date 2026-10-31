mod support;

#[test]
fn scripts_score_1000_qsos_fast_in_process() {
    for id in support::IDS {
        let t = support::bench::measure(id);
        println!(
            "in-process {id}: start {:.3} ms, append {:.4} ms, full replay {:.3} ms",
            t.start, t.append, t.replay
        );
        if !cfg!(debug_assertions) {
            assert!(t.append <= 1.0, "{id} append {} ms", t.append);
            assert!(t.replay <= 50.0, "{id} replay {} ms", t.replay);
        }
    }
}
