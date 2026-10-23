//! How the extensions' windows, toolbar buttons and QSO menu items are grouped.
//!
//! Each panel names its [`MenuGroup`]; the main window shows one menu per group
//! that has a panel (Search, Report, Device, Network, Tools), between Edit and
//! Settings. Within a menu the items are sorted by their [`zlog_ui::MenuSection`]. Separate menus rather than one Tools menu with submenus: during a contest a
//! window is one click away, and the groups stay short.

use zlog_ui::MenuGroup;

/// The language key of a group's menu title.
pub fn group_key(group: MenuGroup) -> &'static str {
    match group {
        MenuGroup::Search => "menu.group.search",
        MenuGroup::Device => "menu.group.device",
        MenuGroup::Network => "menu.group.network",
        MenuGroup::Report => "menu.group.report",
        MenuGroup::Tools => "menu.group.tools",
        MenuGroup::Help => "menu.help",
        MenuGroup::Settings => "menu.settings",
        MenuGroup::File => "menu.file",
        MenuGroup::Edit => "menu.edit",
    }
}

/// Where `group` stands in the menu bar ([`MenuGroup::ALL`]; Help last).
pub fn position(group: MenuGroup) -> usize {
    MenuGroup::ALL
        .iter()
        .position(|g| *g == group)
        .unwrap_or(MenuGroup::ALL.len())
}

/// `items` (group, panel index, item) as one list per group, groups in the order
/// of [`MenuGroup::ALL`], items in the order given (the registry's); groups
/// without items are left out.
pub fn grouped<T>(
    items: impl IntoIterator<Item = (MenuGroup, usize, T)>,
) -> Vec<(MenuGroup, Vec<(usize, T)>)> {
    let mut groups: Vec<(MenuGroup, Vec<(usize, T)>)> =
        MenuGroup::ALL.iter().map(|g| (*g, Vec::new())).collect();
    for (g, i, t) in items {
        if let Some((_, list)) = groups.iter_mut().find(|(h, _)| *h == g) {
            list.push((i, t));
        }
    }
    groups.retain(|(_, list)| !list.is_empty());
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_grouped_in_menu_order_keeping_registry_order() {
        let items = [
            (MenuGroup::Report, 0, "score"),
            (MenuGroup::Search, 1, "partial"),
            (MenuGroup::Report, 2, "multi"),
            (MenuGroup::Tools, 3, "ports"),
            (MenuGroup::Search, 4, "super"),
            (MenuGroup::Device, 5, "keyer"),
            (MenuGroup::Network, 6, "cluster"),
            (MenuGroup::Device, 7, "ports"),
        ];
        let g = grouped(items);
        let names: Vec<(MenuGroup, Vec<&str>)> = g
            .iter()
            .map(|(g, l)| (*g, l.iter().map(|(_, n)| *n).collect()))
            .collect();
        assert_eq!(
            names,
            [
                (MenuGroup::Search, vec!["partial", "super"]),
                (MenuGroup::Report, vec!["score", "multi"]),
                (MenuGroup::Device, vec!["keyer", "ports"]),
                (MenuGroup::Network, vec!["cluster"]),
                (MenuGroup::Tools, vec!["ports"]),
            ]
        );
        assert!(grouped(Vec::<(MenuGroup, usize, ())>::new()).is_empty());
    }

    /// Every linked panel is in the group chosen for it.
    #[test]
    fn panels_are_in_their_menus() {
        let groups: std::collections::HashMap<String, MenuGroup> = crate::registry::panels()
            .iter()
            .map(|p| (p.id().to_string(), p.menu()))
            .collect();
        let expect = [
            ("score", MenuGroup::Report),
            ("multipliers", MenuGroup::Report),
            ("summary", MenuGroup::Report),
            ("map", MenuGroup::Report),
            ("partial-check", MenuGroup::Search),
            ("super-check", MenuGroup::Search),
            ("check-call", MenuGroup::Search),
            ("check-multi", MenuGroup::Search),
            ("keyer", MenuGroup::Device),
            ("cw-decoder", MenuGroup::Device),
            ("voice", MenuGroup::Device),
            ("rtty", MenuGroup::Device),
            ("qsy", MenuGroup::Device),
            ("ops", MenuGroup::Settings),
            ("scope", MenuGroup::Report),
            ("rate", MenuGroup::Report),
            ("cluster", MenuGroup::Network),
            ("link", MenuGroup::Network),
            ("soumu", MenuGroup::Search),
            ("jarl_member", MenuGroup::Search),
            ("es", MenuGroup::Network),
            ("update", MenuGroup::Help),
            ("zylo", MenuGroup::Settings),
            ("ports", MenuGroup::Device),
        ];
        for (id, group) in expect {
            if let Some(g) = groups.get(id) {
                assert_eq!(*g, group, "{id}");
            }
        }
    }

    /// Every panel but the updater has a window button with a tooltip, in both
    /// languages; the most used ones are shown by default, the rest hidden.
    #[test]
    fn toolbar_buttons_and_defaults() {
        let shown = [
            "score",
            "multipliers",
            "partial-check",
            "super-check",
            "scope",
            "cw-decoder",
            "map",
            "keyer",
            "cluster",
            "link",
        ];
        let actions: &[(&str, &[&str])] = &[
            ("keyer", &["cq", "stop"]),
            ("voice", &["cq", "stop"]),
            ("cluster", &["connect"]),
            ("link", &["connect"]),
        ];
        for p in crate::registry::panels() {
            let id = p.id();
            for lang in ["en", "ja"] {
                let b = p.commands(lang);
                // The updater and the country (status bar only) have no buttons.
                if id == "update" || p.menu().is_app_menu() || !p.has_window() {
                    assert!(b.iter().all(|x| !x.toolbar), "{id}");
                    continue;
                }
                if id == "update" {
                    assert!(b.is_empty());
                    continue;
                }
                assert!(b[0].is_window(), "{id}");
                for x in &b {
                    assert!(!x.label.is_empty() && !x.tooltip.is_empty(), "{id}");
                    assert!(x.label.chars().count() <= 12, "{id}: {}", x.label);
                }
                let ids: Vec<&str> = b[1..]
                    .iter()
                    .map(|x| {
                        assert!(!x.is_window(), "{id}: two window buttons");
                        x.id.as_str()
                    })
                    .collect();
                let want = actions
                    .iter()
                    .find(|(p, _)| *p == id)
                    .map_or(&[][..], |(_, a)| *a);
                assert_eq!(ids, want, "{id}");
            }
            if id != "update" && !p.menu().is_app_menu() && p.has_window() {
                assert_eq!(p.toolbar_default_shown(), shown.contains(&id), "{id}");
            }
        }
    }
}
