use std::cell::RefCell;
use std::rc::Rc;

use crate::{MenuGroup, Section};

pub const SECTION: &str = "window";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolbarStyle {
    Icons,
    #[default]
    IconsAndLabels,
    Labels,
}

impl ToolbarStyle {
    pub const ALL: [ToolbarStyle; 3] = [
        ToolbarStyle::Icons,
        ToolbarStyle::IconsAndLabels,
        ToolbarStyle::Labels,
    ];

    pub fn code(self) -> &'static str {
        match self {
            ToolbarStyle::Icons => "icons",
            ToolbarStyle::IconsAndLabels => "both",
            ToolbarStyle::Labels => "labels",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarRow {
    pub key: String,
    pub label: String,
    pub default: bool,
    pub group: MenuGroup,
    pub group_title: String,
}

pub type Rows = Rc<RefCell<Vec<ToolbarRow>>>;

pub fn position(group: MenuGroup) -> usize {
    let menu = |g| {
        MenuGroup::ALL
            .iter()
            .position(|x| *x == g)
            .unwrap_or(MenuGroup::ALL.len())
    };
    match group {
        MenuGroup::File => 0,
        MenuGroup::Edit => 1,
        MenuGroup::Settings => 2 + MenuGroup::ALL.len(),
        MenuGroup::Window => 3 + MenuGroup::ALL.len(),
        MenuGroup::Help => 4 + MenuGroup::ALL.len(),
        g => 2 + menu(g),
    }
}

pub fn in_order(rows: &mut [ToolbarRow], order: &[String]) {
    rows.sort_by_key(|r| {
        let chosen = order.iter().position(|k| *k == r.key);
        (position(r.group), chosen.unwrap_or(usize::MAX))
    });
}

pub fn style(s: &Section) -> ToolbarStyle {
    let v = s.get("toolbar_style");
    ToolbarStyle::ALL
        .into_iter()
        .find(|st| v.map(String::as_str) == Some(st.code()))
        .unwrap_or_default()
}

pub fn set_style(s: &mut Section, style: ToolbarStyle) {
    s.insert("toolbar_style".into(), style.code().into());
}

pub fn shown(s: &Section, id: &str, default: bool) -> bool {
    if list(s, "hidden_toolbars").iter().any(|h| h == id) {
        false
    } else if list(s, "shown_toolbars").iter().any(|h| h == id) {
        true
    } else {
        default
    }
}

pub fn set_shown(s: &mut Section, id: &str, shown: bool) {
    for (key, add) in [("hidden_toolbars", !shown), ("shown_toolbars", shown)] {
        let mut l = list(s, key);
        l.retain(|h| h != id);
        if add {
            l.push(id.to_string());
        }
        l.sort();
        s.insert(key.into(), l.join(","));
    }
}

pub fn order(s: &Section) -> Vec<String> {
    list(s, "toolbar_order")
}

pub fn set_order(s: &mut Section, keys: &[String]) {
    s.insert("toolbar_order".into(), keys.join(","));
}

pub fn reset(s: &mut Section) {
    for key in ["hidden_toolbars", "shown_toolbars", "toolbar_order"] {
        s.remove(key);
    }
}

fn list(s: &Section, key: &str) -> Vec<String> {
    s.get(key)
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}
