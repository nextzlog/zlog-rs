use zlog_ui::Builder;

pub fn register(b: &mut Builder) {
    b.extension(zlog_ui::builder::order::RULES + 3, || zlog_rule::Base);
    zlog_undo::register(b);
    zlog_update::register(b);
    zlog_entry::register(b);
    zlog_open::register(b);
    zlog_menu::register(b);
    #[cfg(target_os = "macos")]
    zlog_mac::register(b);
    zlog_toolbar::register(b);
    zlog_dock::register(b);
    zlog_status::register(b);
    zlog_table::register(b);
    zlog_style::register(b);
    zlog_bind::register(b);
}

#[cfg(test)]
mod tests {
    #[test]
    fn core_declares_its_parts() {
        let mut b = zlog_ui::Builder::new();
        super::register(&mut b);
        assert!(!b.registry().contests().is_empty());
        assert!(!b.build_entry_styles().is_empty());
        assert!(b.build_menu_bar().is_some());
        assert!(b.build_tool_bar().is_some());
        assert!(b.build_status_bar().is_some());
        assert!(b.build_log_table().is_some());
        assert!(b.build_window_host().is_some());
        assert!(b.look().is_some());
        assert!(
            b.start_time().is_none(),
            "the start-time dialog is an extension's"
        );
    }
}
