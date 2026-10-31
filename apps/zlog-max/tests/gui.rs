zlog_testing::gui_tests!(
    zlog_max::register,
    "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"
);

#[test]
fn options_fields_follow_the_dialog_width() {
    zlog_testing::install(zlog_max::register);
    zlog_testing::options_fields_follow_the_dialog_width(&[
        "Station",
        "Rig 1",
        "Back up",
        "Auto-post to social media",
        "ZyLO",
    ]);
}

const TABS: &[&str] = &[
    "Station",
    "Preferences",
    "Entry row",
    "Display",
    "Shortcuts",
    "Toolbar",
    "Rig 1",
    "Rig 2",
    "CW4ISR",
    "RTTY",
    "FT8 / FT4",
    "CW keyer",
    "Voice memory",
    "SO2R",
    "Call history",
    "Back up",
    "CTY.DAT",
    "Auto-post to social media",
    "ZyLO",
    "PacketCluster",
    "Z-Link",
    "Band scope",
];

#[test]
#[ignore = "slow without optimisation; run with cargo test --release -p zlog-max -- --ignored"]
fn options_fields_line_up() {
    zlog_testing::install(zlog_max::register);
    zlog_testing::options_fields_line_up(TABS);
}
