#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum MenuSection {
    Log,
    Exchange,
    Period,
    Quit,
    Undo,
    Qso,
    Bulk,
    Entry,
    Check,
    Find,
    LookUp,
    Score,
    Progress,
    Submit,
    Rig,
    Cw,
    Phone,
    Digital,
    Assist,
    Ports,
    Link,
    Spots,
    Post,
    Station,
    Plugins,
    #[default]
    Other,
}

pub fn sort_key(section: MenuSection, weight: i32, label: &str) -> (MenuSection, i32, String) {
    (section, weight, label.to_lowercase())
}

pub fn split<T>(mut items: Vec<(MenuSection, i32, String, T)>) -> Vec<(MenuSection, Vec<T>)> {
    items.sort_by_cached_key(|(s, w, l, _)| sort_key(*s, *w, l));
    let mut out: Vec<(MenuSection, Vec<T>)> = Vec::new();
    for (s, _, _, t) in items {
        match out.last_mut() {
            Some((last, list)) if *last == s => list.push(t),
            _ => out.push((s, vec![t])),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorted_by_section_weight_and_label_then_split() {
        let items = vec![
            (MenuSection::Other, 0, "Zeta".into(), 1),
            (MenuSection::Phone, 0, "Voice".into(), 2),
            (MenuSection::Cw, 1, "Decoder".into(), 3),
            (MenuSection::Cw, 0, "Keyer".into(), 4),
            (MenuSection::Digital, 0, "RTTY".into(), 5),
        ];
        assert_eq!(
            split(items),
            [
                (MenuSection::Cw, vec![4, 3]),
                (MenuSection::Phone, vec![2]),
                (MenuSection::Digital, vec![5]),
                (MenuSection::Other, vec![1]),
            ]
        );
        assert!(split(Vec::<(MenuSection, i32, String, ())>::new()).is_empty());
    }
}
