use zlog_core::{Band, Contest, ContestMode, Log, Mode, Power, Qso, ScoreSheet, Scoring, Station};

use super::*;

struct Test;

impl Contest for Test {
    fn name(&self) -> &str {
        "test"
    }
    fn band_range(&self) -> (Band, Band) {
        (Band::B1_9, Band::B50)
    }
    fn contest_mode(&self) -> ContestMode {
        ContestMode::Cw
    }
    fn sent_number(&self, _band: Band, _power: Power, _station: &Station) -> String {
        String::new()
    }
    fn valid_number(&self, _q: &Qso) -> bool {
        true
    }
    fn score(&mut self, _log: &Log) -> Scoring {
        Scoring {
            qsos: Vec::new(),
            sheet: ScoreSheet::default(),
        }
    }
}

fn with_view<R>(log: &Log, f: impl FnOnce(&View<'_>) -> R) -> R {
    let sheet = ScoreSheet::default();
    let station = Station::default();
    let view = View {
        station: &station,
        number: "",
        memo: "",
        rst_sent: "",
        power: Power::P010,
        log,
        contest: &Test,
        sheet: &sheet,
        callsign: "",
        band: Band::B7,
        mode: Mode::Cw,
        language: "en",
        spots: &[],
        hz: None,
        rig: 1,
        cq: false,
        operator: "",
        period_hours: 0,
        pc_name: "",
        log_path: "",
        focus: None,
        editing: None,
        selected: None,
        period: &(),
        tab: Default::default(),
        changes: 0,
    };
    f(&view)
}

fn providers() -> Vec<RoleCommand> {
    let all: Vec<Command> = ["score", "multi", "check", "history", "elog", "jarl_upload"]
        .into_iter()
        .map(|role| Command::new(role, role, "").role(role))
        .collect();
    zlog_ui::role_commands(&all)
}

#[test]
fn texts_complete() {
    assert!(texts().missing().is_empty());
    for lang in ["en", "ja"] {
        for (key, _) in STEPS {
            let k = format!("{key}.text");
            let text = texts().get(lang, &k);
            assert!(!text.is_empty() && !text.contains(".text"), "{lang} {key}");
            assert!(
                !text.chars().any(|c| ('\u{ff10}'..='\u{ff5a}').contains(&c)),
                "{lang} {key}"
            );
        }
    }
}

#[test]
fn with_only_the_guide_every_step_is_greyed() {
    let mut g = Guide::default();
    g.role_commands(&[]);
    for (_, roles) in STEPS {
        for role in roles {
            assert_eq!(g.link(role, "en"), Link::Missing, "{role}");
            assert!(!g.run(role));
        }
    }
    assert!(g.take_actions().is_empty());
}

#[test]
fn providers_resolve_the_steps_and_a_button_runs_the_command() {
    let mut g = Guide::default();
    g.role_commands(&providers());
    for role in ["score", "multi", "check", "history", "elog", "jarl_upload"] {
        assert!(matches!(g.link(role, "en"), Link::Ready(_)), "{role}");
    }
    assert_eq!(g.link("open", "en"), Link::Missing);
    assert!(g.run("score"));
    assert_eq!(g.take_actions(), [EntryAction::Run("score".into())]);
}

#[test]
fn a_disabled_command_is_greyed_with_its_reason() {
    let c = Command::new("x", "Export", "")
        .role("elog")
        .disabled_because("No QSOs");
    let mut g = Guide::default();
    g.role_commands(&zlog_ui::role_commands(&[c]));
    assert_eq!(
        g.link("elog", "en"),
        Link::Disabled("Export".into(), "No QSOs".into())
    );
    assert!(!g.run("elog"));
}

#[test]
fn progress_is_kept_for_each_log_across_a_restart() {
    let first = Log::default();
    let mut second = Log::default();
    second.set_start_time(zlog_core::DelphiTime(45000.5));
    let mut g = Guide::default();
    with_view(&first, |v| g.tick(v));
    g.next();
    g.set_done(3, true);
    assert_eq!(g.step(), 1);
    with_view(&second, |v| g.tick(v));
    assert!(!g.is_done(0), "another log starts afresh");
    g.set_done(5, true);
    let mut section = Section::new();
    g.save(&mut section);
    let mut again = Guide::default();
    again.load(&section);
    with_view(&first, |v| again.tick(v));
    assert!(again.is_done(0) && again.is_done(3) && !again.is_done(5));
    with_view(&second, |v| again.tick(v));
    assert!(again.is_done(5) && !again.is_done(0));
    again.set_done(5, false);
    again.save(&mut section);
    assert_eq!(section.len(), 1);
}
