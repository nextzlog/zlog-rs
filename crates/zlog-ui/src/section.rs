//! What a command does, as the part of its menu it is listed in.
//!
//! Every [`crate::Command`] names its [`MenuSection`] and a weight within it; the
//! application sorts a menu by the order of the sections here, then the weight,
//! then the label, and draws a separator between two sections that both have
//! items. So an extension adds its items without knowing the other extensions or
//! the order of the menu, and leaving one out never leaves a stray separator.

/// The parts of the menus, in the order they are listed; each menu uses those of
/// its group, and [`MenuSection::Other`] comes last in every menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum MenuSection {
    /// File: opening and saving the log.
    Log,
    /// File: importing and exporting other formats.
    Exchange,
    /// File: the contest period.
    Period,
    /// File: leaving the program.
    Quit,
    /// Edit: taking steps back and forth (undo, redo, skip).
    Undo,
    /// Edit: the QSO selected in the log (edit, delete, insert, band, mode).
    Qso,
    /// Edit: many QSOs at once (correcting the numbers received).
    Bulk,
    /// Edit: the keys of the entry row.
    Entry,
    /// Search: checks of the callsign being entered (partial, super check).
    Check,
    /// Search: searching the log.
    Find,
    /// Search: looking stations up elsewhere (JARL members, licences).
    LookUp,
    /// Report: the score and the multipliers.
    Score,
    /// Report: how the contest goes (rate, band scope, map).
    Progress,
    /// Report: the summary sheet to submit.
    Submit,
    /// Device: the rig, which rig is in use, and the second radio (SO2R and its
    /// queue).
    Rig,
    /// Device: CW (the keyer, the CW decoder).
    Cw,
    /// Device: phone (the voice memories).
    Phone,
    /// Device: digital modes (the RTTY terminal).
    Digital,
    /// Device: helping the operator (QSY rules).
    Assist,
    /// Device: serial ports.
    Ports,
    /// Network: stations of the same contest (Z-Link).
    Link,
    /// Network: spots and propagation news (cluster, Es).
    Spots,
    /// Network: posting QSOs elsewhere (Discord, Slack).
    Post,
    /// Settings: the station (its operators).
    Station,
    /// Settings: extensions loaded at run time (ZyLO plugins).
    Plugins,
    /// Whatever names no section; last in every menu.
    #[default]
    Other,
}

/// Where a command stands in its menu: its section, its weight, its label.
pub fn sort_key(section: MenuSection, weight: i32, label: &str) -> (MenuSection, i32, String) {
    (section, weight, label.to_lowercase())
}

/// `items` (section, weight, label, item) sorted and split into the sections that
/// have items, in order; a separator goes between two of them.
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
