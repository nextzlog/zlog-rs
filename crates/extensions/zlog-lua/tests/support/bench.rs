use std::sync::Arc;
use std::time::Instant;

use zlog_cfg::script::{ScriptQso, Sums};

use super::gen;

pub struct Timing {
    pub start: f64,
    pub append: f64,
    pub replay: f64,
}

pub fn inputs(id: &str, len: usize) -> Vec<Arc<ScriptQso>> {
    let dat = std::fs::read_to_string(super::fixture(&format!("{id}.dat"))).unwrap();
    gen::log(5, len, &gen::codes(&dat))
        .qsos
        .iter()
        .enumerate()
        .map(|(i, q)| Arc::new(ScriptQso::of(q, i % 17 == 0, i % 23 != 0)))
        .collect()
}

pub fn program(id: &str) -> std::sync::Arc<dyn zlog_cfg::script::ScriptProgram> {
    super::setup();
    let text = std::fs::read_to_string(super::fixture(&format!("{id}.cfg"))).unwrap();
    let section = zlog_cfg::script::split(&text).1.unwrap();
    let engine = zlog_cfg::script::engine(&section.language).unwrap();
    zlog_cfg::script::host::program(engine, id, section.source, 1)
}

pub fn measure(id: &str) -> Timing {
    let all = inputs(id, 1100);
    let sums = Sums {
        qsos: 1,
        points: 2,
        multis: 3,
    };
    let upto = |n: usize| Arc::new(all[..n].to_vec());
    let t = Instant::now();
    let s = program(id).start();
    s.judge(&upto(0)).unwrap();
    let start = t.elapsed().as_secs_f64() * 1e3;
    let first = upto(1000);
    s.judge(&first).unwrap();
    s.total(&first, sums).unwrap();
    let t = Instant::now();
    for n in 1001..=1100 {
        let now = upto(n);
        s.judge(&now).unwrap();
        s.total(&now, sums).unwrap();
    }
    let append = t.elapsed().as_secs_f64() * 1e3 / 100.0;
    let mut edited = all[..1000].to_vec();
    let t = Instant::now();
    for i in 0..20 {
        let mut q = (*edited[0]).clone();
        q.rcvd = format!("{i}");
        edited[0] = Arc::new(q);
        let now = Arc::new(edited.clone());
        s.judge(&now).unwrap();
        s.total(&now, sums).unwrap();
    }
    let replay = t.elapsed().as_secs_f64() * 1e3 / 20.0;
    Timing {
        start,
        append,
        replay,
    }
}
