use zlog_core::{Log, LogFormat};
use zlog_qxsl::QxslFormat;

fn regress(path: &str) -> String {
    format!(
        "{}/../../../apps/zlog-max/tests/regress/{path}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn import(kind: &str, path: &str) -> Vec<zlog_core::Qso> {
    let format = rxsl::formats().into_iter().find(|f| f.kind() == kind);
    let bytes = std::fs::read(regress(path)).unwrap();
    QxslFormat::new(format.unwrap()).import(&bytes).unwrap()
}

#[test]
fn zlog_text_exports_of_allja1_import_back() {
    let zlo = Log::load(regress("logs/allja1.zlo")).unwrap().0.qsos;
    for (kind, path) in [
        ("zall", "convert-allja1-to-all/expected"),
        ("zdos", "convert-allja1-to-zlog-dos/expected"),
    ] {
        let back = import(kind, path);
        assert_eq!(back.len(), zlo.len(), "{kind}");
        for (i, (a, b)) in zlo.iter().zip(&back).enumerate() {
            let t = |q: &zlog_core::Qso| {
                let (_, mo, d, h, mi, _) = q.time.to_ymd_hms();
                (mo, d, h, mi)
            };
            let key = |q: &zlog_core::Qso| {
                (
                    q.callsign.clone(),
                    q.band,
                    q.mode,
                    q.rst_sent,
                    q.rst_rcvd,
                    q.nr_rcvd.clone(),
                    q.points,
                )
            };
            let flags = |q: &zlog_core::Qso| {
                (
                    q.dupe,
                    q.forced,
                    q.qsl_state,
                    q.memo.clone(),
                    q.operator.clone(),
                    q.freq.clone(),
                    q.new_multi1,
                    q.new_multi1.then(|| q.multi1.clone()),
                )
            };
            assert_eq!((t(a), key(a)), (t(b), key(b)), "{kind} record {i}");
            assert_eq!(flags(a), flags(b), "{kind} record {i}");
            assert_eq!(a.nr_sent, b.nr_sent, "{kind} record {i}");
            if kind == "zall" {
                assert_eq!(a.time.to_ymd_hms().0, b.time.to_ymd_hms().0);
            }
        }
    }
}
