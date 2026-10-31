use zlog_ui::MenuGroup;

pub fn group_key(group: MenuGroup) -> &'static str {
    match group {
        MenuGroup::Search => "menu.group.search",
        MenuGroup::Device => "menu.group.device",
        MenuGroup::Network => "menu.group.network",
        MenuGroup::Report => "menu.group.report",
        MenuGroup::Tools => "menu.group.tools",
        MenuGroup::Help => "menu.help",
        MenuGroup::Window => "menu.window",
        MenuGroup::Settings => "menu.settings",
        MenuGroup::File => "menu.file",
        MenuGroup::Edit => "menu.edit",
    }
}

pub fn position(group: MenuGroup) -> usize {
    MenuGroup::ALL
        .iter()
        .position(|g| *g == group)
        .unwrap_or(MenuGroup::ALL.len())
}

pub(crate) fn grouped<T>(
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
            ("call-candidates", MenuGroup::Search),
            ("this-station", MenuGroup::Search),
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

    #[test]
    fn ellipsis_marks_exactly_the_commands_needing_input() {
        crate::ellipsis::classify(
            &[
                "file/file.export/file.export.0",
                "file/file.import/file.import.0",
                "file/file.open",
                "file/file.save_as",
                "file/file.save_as_zlo",
                "file/file.start_time",
                "settings/settings.options",
                "update/window",
            ],
            &[
                "edit/edit.clear",
                "edit/edit.cq_sp",
                "edit/edit.next_band",
                "edit/edit.next_mode",
                "edit/edit.next_power",
                "file/file.exit",
                "file/file.export",
                "file/file.import",
                "file/file.save",
                "file/file.station",
                "settings/settings.reopen",
                "undo/redo",
                "undo/undo",
            ],
        );
        crate::ellipsis::lang_files(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../.."
        )));
    }

    #[test]
    fn dialog_items_open_and_tool_windows_toggle() {
        let dialogs = [
            "correct_nr",
            "jarl_member",
            "guide",
            "update",
            "zylo",
            "soumu",
            "summary",
            "ports",
            "ops",
        ];
        for mut p in crate::registry::panels() {
            let id = p.id().to_string();
            let dialog = p.window_kind() == zlog_ui::WindowKind::Dialog;
            assert_eq!(dialog, dialogs.contains(&id.as_str()), "{id}");
            if !p.window_kind().has_window() {
                continue;
            }
            for lang in ["en", "ja"] {
                let list = zlog_ui::panel_commands(p.as_ref(), false, lang);
                let w = list.iter().find(|c| c.is_window()).expect("window");
                if dialog {
                    assert!(!w.checkable && !w.toolbar, "{id}");
                    assert!(w.dialog && w.menu_text().ends_with('…'), "{id}");
                    assert!(!w.window_title().ends_with('…'), "{id}");
                } else if !p.menu().is_app_menu() {
                    assert!(w.checkable, "{id}");
                }
            }
            let mut open = false;
            for _ in 0..2 {
                use zlog_ui::Commands;
                zlog_ui::PanelCommands {
                    panel: p.as_mut(),
                    open: &mut open,
                }
                .run(zlog_ui::Command::WINDOW);
            }
            assert_eq!(open, dialog || p.menu().is_app_menu(), "{id}");
        }
    }

    #[test]
    fn toolbar_buttons_and_defaults() {
        let shown = [
            "score",
            "multipliers",
            "call-candidates",
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
        let mut keys = std::collections::HashMap::new();
        let mut candidates = 0;
        for p in crate::registry::panels() {
            let id = p.id();
            for lang in ["en", "ja"] {
                let b = zlog_ui::panel_commands(p.as_ref(), false, lang);
                if id == "update" || !p.window_kind().has_window() && p.commands(lang).is_empty() {
                    assert!(b.iter().all(|x| !x.toolbar), "{id}");
                    continue;
                }
                let on: Vec<&zlog_ui::Command> = b
                    .iter()
                    .filter(|x| x.toolbar || x.toolbar_key.is_some())
                    .collect();
                for (n, x) in on.iter().enumerate() {
                    assert!(!x.label.is_empty() && !x.description.is_empty(), "{id}");
                    let short = x.label.chars().count() <= 12 || id == "rig";
                    assert!(short, "{id}: {}", x.label);
                    assert!(x.icon.is_some(), "{id}: {}", x.id);
                    assert!(!x.is_window() || n == 0, "{id}: window first");
                    match &x.toolbar_key {
                        Some(k) => {
                            assert!(!(x.toolbar && x.toolbar_default), "{id}: {k} hidden");
                            let owner = keys
                                .entry(k.clone())
                                .or_insert((id.to_string(), x.id.clone()));
                            assert_eq!(*owner, (id.to_string(), x.id.clone()), "{k} twice");
                            candidates += 1;
                        }
                        None => {
                            assert!(!p.menu().is_app_menu(), "{id}");
                            assert_eq!(x.toolbar_default, shown.contains(&id), "{id}");
                        }
                    }
                }
                let shared: Vec<&str> = on
                    .iter()
                    .filter(|x| x.toolbar_key.is_none() && !x.is_window())
                    .map(|x| x.id.as_str())
                    .collect();
                let want = actions
                    .iter()
                    .find(|(p, _)| *p == id)
                    .map_or(&[][..], |(_, a)| *a);
                assert_eq!(shared, want, "{id}");
            }
        }
        assert!((1..=16).contains(&(candidates / 2)), "{candidates}");
    }
}
