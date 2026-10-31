fn init() {
    zlog_gui::testing::registry::install(zlog_std::register);
}

#[test]
fn core_declares_no_drivers() {
    init();
    assert!(zlog_gui::testing::registry::drivers().is_empty());
}

#[test]
fn toolbar_default_set() {
    init();
    use zlog_gui::testing::commands::{list, toolbar_rows, CoreCommands, CORE_GROUPS};
    let core: Vec<CoreCommands> = CORE_GROUPS
        .iter()
        .map(|&group| CoreCommands {
            group,
            state: Default::default(),
            chosen: None,
        })
        .collect();
    let panels = zlog_gui::testing::registry::panels();
    let all = list(&core, panels.iter().map(|p| (p.as_ref(), false)), "en");
    let mut shown: Vec<String> = toolbar_rows(&all, &[])
        .into_iter()
        .filter(|r| r.default)
        .map(|r| r.key)
        .collect();
    shown.sort();
    assert_eq!(shown, DEFAULT_SHOWN, "{shown:?}");
}
const DEFAULT_SHOWN: &[&str] = &["file.open", "file.save"];
