use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use zlog_core::Band;

use crate::user::{delphi_str_to_int, DatEntry};

pub fn in_list(item: &str, list: &str) -> bool {
    let (item, list) = (item.as_bytes(), list.as_bytes());
    let n = item.len();
    n <= list.len()
        && (0..=list.len() - n).any(|i| {
            (i == 0 || list[i - 1] == b',')
                && list[i..].starts_with(item)
                && (i + n == list.len() || list[i + n] == b',')
        })
}
fn number(s: &str) -> Option<i32> {
    delphi_str_to_int(s).filter(|v| *v != -1)
}

pub fn matches(s: &str, code: &str, numeric: bool) -> bool {
    if numeric {
        if let (Some(a), Some(b)) = (number(s), number(code)) {
            return a == b;
        }
    }
    if s.contains(',') {
        in_list(s, code)
    } else {
        code.split(',').any(|c| c == s)
    }
}

#[derive(Default, Clone)]
struct Keys {
    all: HashMap<String, usize>,
    text: HashMap<String, usize>,
    number: HashMap<i32, usize>,
}

impl Keys {
    fn add(&mut self, id: usize, code: &str, earlier: impl Fn(usize, usize) -> bool) {
        let put = |map: &mut HashMap<String, usize>, key: &str| match map.get_mut(key) {
            Some(old) if earlier(id, *old) => *old = id,
            Some(_) => {}
            None => {
                map.insert(key.to_string(), id);
            }
        };
        let n = number(code);
        for item in code.split(',') {
            put(&mut self.all, item);
            if n.is_none() {
                put(&mut self.text, item);
            }
        }
        if let Some(n) = n {
            match self.number.get_mut(&n) {
                Some(old) if earlier(id, *old) => *old = id,
                Some(_) => {}
                None => {
                    self.number.insert(n, id);
                }
            }
        }
    }

    fn find(&self, s: &str, numeric: bool, out: &mut [Option<usize>; 2]) {
        *out = match numeric.then(|| number(s)).flatten() {
            Some(n) => [self.number.get(&n).copied(), self.text.get(s).copied()],
            None => [self.all.get(s).copied(), None],
        };
    }

    fn clear(&mut self) {
        self.all.clear();
        self.text.clear();
        self.number.clear();
    }
}

fn str_more(a: &str, b: &str) -> bool {
    for (x, y) in a.chars().zip(b.chars()) {
        if x != y {
            return x > y;
        }
    }
    a.chars().count() > b.chars().count()
}

#[derive(Clone)]
pub struct Index {
    entries: Arc<Vec<DatEntry>>,
    keys: Keys,
}

impl Index {
    pub fn new(entries: Arc<Vec<DatEntry>>) -> Self {
        let mut keys = Keys::default();
        for (id, e) in entries.iter().enumerate() {
            keys.add(id, &e.code, |a, b| a < b);
        }
        Index { entries, keys }
    }
}

pub struct Table {
    base: Arc<Index>,
    numeric: bool,
    added: Vec<DatEntry>,
    added_keys: Keys,
    order: Vec<usize>,
    rank: Vec<usize>,
    worked: Vec<BTreeSet<Band>>,
}

impl Table {
    pub fn new(base: Arc<Index>, numeric: bool) -> Self {
        let n = base.entries.len();
        Table {
            base,
            numeric,
            added: Vec::new(),
            added_keys: Keys::default(),
            order: (0..n).collect(),
            rank: (0..n).collect(),
            worked: vec![BTreeSet::new(); n],
        }
    }

    pub fn reset(&mut self) {
        let n = self.base.entries.len();
        if !self.added.is_empty() {
            self.added.clear();
            self.added_keys.clear();
            self.order = (0..n).collect();
            self.rank = (0..n).collect();
            self.worked.truncate(n);
        }
        self.worked.iter_mut().for_each(BTreeSet::clear);
    }

    pub fn entry(&self, id: usize) -> &DatEntry {
        let n = self.base.entries.len();
        if id < n {
            &self.base.entries[id]
        } else {
            &self.added[id - n]
        }
    }

    pub fn find(&self, s: &str) -> Option<usize> {
        if s.contains(',') {
            return self
                .order
                .iter()
                .copied()
                .find(|id| matches(s, &self.entry(*id).code, self.numeric));
        }
        let mut base = [None; 2];
        let mut added = [None; 2];
        self.base.keys.find(s, self.numeric, &mut base);
        self.added_keys.find(s, self.numeric, &mut added);
        base.into_iter()
            .chain(added)
            .flatten()
            .min_by_key(|id| self.rank[*id])
    }

    pub fn worked(&self, id: usize) -> &BTreeSet<Band> {
        &self.worked[id]
    }

    pub fn work(&mut self, id: usize, band: Band) -> bool {
        self.worked[id].insert(band)
    }

    pub fn insert(&mut self, code: &str, band: Band) {
        let at = self
            .order
            .iter()
            .position(|id| str_more(&self.entry(*id).code, code))
            .unwrap_or(self.order.len());
        let id = self.base.entries.len() + self.added.len();
        self.added.push(DatEntry {
            code: code.to_string(),
            name: String::new(),
        });
        self.order.insert(at, id);
        self.rank.push(at);
        for (p, id) in self.order.iter().enumerate().skip(at) {
            self.rank[*id] = p;
        }
        let rank = &self.rank;
        self.added_keys.add(id, code, |a, b| rank[a] < rank[b]);
        self.worked.push(BTreeSet::from([band]));
    }

    pub fn ordered(&self) -> impl Iterator<Item = (&DatEntry, &BTreeSet<Band>)> {
        self.order
            .iter()
            .map(|id| (self.entry(*id), &self.worked[*id]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(entries: &[DatEntry], s: &str, numeric: bool) -> Option<usize> {
        entries.iter().position(|e| matches(s, &e.code, numeric))
    }

    #[test]
    fn in_list_matches_comma_bounded_items() {
        let old = |i: &str, l: &str| format!(",{l},").contains(&format!(",{i},"));
        let words = [
            "", "a", "b", "ab", "a,b", "b,a", ",", "a,", ",a", "a,b,c", "c",
        ];
        for i in words {
            for l in words {
                assert_eq!(in_list(i, l), old(i, l), "{i:?} in {l:?}");
            }
        }
    }

    #[test]
    fn find_agrees_with_a_linear_scan_through_inserts() {
        let pool = [
            "01", "1", "001", "$1", "0x1", "-1", "02", "ABC", "01,02", "02,ABC", "ABC,01", "", "X",
            "10", "0A", "+2", "2", "-01",
        ];
        for numeric in [false, true] {
            for seed in 0..40u64 {
                let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
                let mut next = |n: usize| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    (x % n as u64) as usize
                };
                let base: Vec<DatEntry> = (0..next(8))
                    .map(|_| DatEntry {
                        code: pool[next(pool.len())].into(),
                        name: String::new(),
                    })
                    .collect();
                let mut shown = base.clone();
                let mut t = Table::new(Arc::new(Index::new(Arc::new(base))), numeric);
                for _ in 0..30 {
                    let s = pool[next(pool.len())];
                    let found = t.find(s).map(|id| t.entry(id).code.clone());
                    let want = naive(&shown, s, numeric).map(|i| shown[i].code.clone());
                    assert_eq!(found, want, "{s:?} in {shown:?}");
                    if want.is_none() {
                        t.insert(s, Band::B7);
                        let at = shown
                            .iter()
                            .position(|e| str_more(&e.code, s))
                            .unwrap_or(shown.len());
                        shown.insert(
                            at,
                            DatEntry {
                                code: s.into(),
                                name: String::new(),
                            },
                        );
                    }
                    let order: Vec<String> = t.ordered().map(|(e, _)| e.code.clone()).collect();
                    let want: Vec<String> = shown.iter().map(|e| e.code.clone()).collect();
                    assert_eq!(order, want);
                }
                t.reset();
                assert_eq!(t.ordered().count(), t.base.entries.len());
            }
        }
    }
}
