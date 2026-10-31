mod support;

use support::{builtin, gen, ported, IDS};
use zlog_core::{ContestMode, Log};

fn codes(id: &str) -> Vec<String> {
    gen::codes(&std::fs::read_to_string(support::fixture(&format!("{id}.dat"))).unwrap())
}

#[test]
fn every_edit_gives_a_full_recount() {
    for id in IDS {
        let (new, old) = (ported(id), builtin(id));
        let codes = codes(id);
        let spare = gen::log(900, 200, &codes).qsos;
        let mut log = gen::log(901, 300, &codes);
        let mut c = new.build_rules(ContestMode::Mix);
        let mut rng = gen::Rng::new(902);
        let mut history: Vec<Log> = Vec::new();
        let mut removed = Vec::new();
        for step in 0..80 {
            let n = log.qsos.len();
            let before = log.clone();
            match rng.below(7) {
                0 if n > 1 => log.qsos.swap(rng.below(n), rng.below(n)),
                1 if n > 0 => removed.push(log.qsos.remove(rng.below(n))),
                2 if !removed.is_empty() => {
                    let q = removed.remove(rng.below(removed.len()));
                    let at = rng.below(log.qsos.len() + 1);
                    log.qsos.insert(at, q);
                }
                3 => {
                    let at = rng.below(n + 1);
                    log.qsos.insert(at, rng.pick(&spare).clone());
                }
                4 if !history.is_empty() => {
                    log = history.pop().unwrap();
                    continue;
                }
                5 if n > 0 => {
                    let i = rng.below(n);
                    log.qsos[i].nr_rcvd = rng.pick(&spare).nr_rcvd.clone();
                }
                _ if n > 0 => {
                    let i = rng.below(n);
                    log.qsos[i].invalid = !log.qsos[i].invalid;
                }
                _ => log.qsos.push(rng.pick(&spare).clone()),
            }
            history.push(before);
            let mut l = log.clone();
            support::known(&mut l, &mut c);
            let mut fresh = old.build_rules(ContestMode::Mix);
            assert_eq!(
                gen::describe(&log, &mut c),
                gen::describe(&log, &mut fresh),
                "{id} after step {step}"
            );
        }
    }
}

#[test]
fn the_same_log_always_gives_the_same_result() {
    for id in IDS {
        let def = ported(id);
        let log = gen::log(950, 700, &codes(id));
        let mut a = def.build_rules(ContestMode::Mix);
        let mut b = def.build_rules(ContestMode::Mix);
        let first = gen::describe(&log, &mut a);
        let mut l = log.clone();
        support::known(&mut l, &mut b);
        let mut shuffled = log.clone();
        shuffled.qsos.reverse();
        support::known(&mut shuffled, &mut b);
        assert_eq!(first, gen::describe(&log, &mut b), "{id}");
        assert_eq!(first, gen::describe(&log, &mut a), "{id}");
    }
}
