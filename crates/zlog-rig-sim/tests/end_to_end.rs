//! The CAT drivers against the simulated radio, through real bytes.

use std::time::{Duration, Instant};

use zlog_rig::{Rig, RigError, RigMode};
use zlog_rig_cat::{Dialect, Icom, TextCat};
use zlog_rig_sim::{SimHandle, SimProtocol, SimRadio};

/// Run the rig for `ms` milliseconds of simulated time in 10 ms steps.
fn run(rig: &mut Rig, start: &mut Instant, ms: u64) {
    for _ in 0..ms / 10 {
        *start += Duration::from_millis(10);
        rig.tick(*start);
    }
}

fn icom(echo: bool, chunk: usize) -> (SimHandle, Rig) {
    let mut radio = SimRadio::new(SimProtocol::Icom(0x94));
    radio.echo = echo;
    radio.chunk = chunk;
    let sim = SimHandle::new(radio);
    let rig = Rig::new(Box::new(Icom::new(0x94)), Box::new(sim.transport()), 19200);
    (sim, rig)
}

#[test]
fn icom_follows_the_dial_with_echo_and_split_replies() {
    let (sim, mut rig) = icom(true, 3);
    let mut t = Instant::now();
    run(&mut rig, &mut t, 500);
    assert_eq!(rig.state.freq_a, Some(7_012_000));
    assert_eq!(rig.state.mode, Some(RigMode::Cw));

    sim.tune(14_025_000);
    sim.press_mode(RigMode::Usb);
    run(&mut rig, &mut t, 500);
    assert_eq!(rig.state.freq_a, Some(14_025_000));
    assert_eq!(rig.state.mode, Some(RigMode::Usb));
    assert_eq!(rig.error, None);
}

#[test]
fn icom_commands_reach_the_radio() {
    let (sim, mut rig) = icom(false, 0);
    let mut t = Instant::now();
    rig.set_freq(21_030_000);
    rig.set_mode(RigMode::Cw);
    run(&mut rig, &mut t, 500);
    assert_eq!(sim.with(|r| (r.freq_a, r.mode)), (21_030_000, RigMode::Cw));
}

#[test]
fn a_radio_switched_off_is_reported_and_recovers() {
    let (sim, mut rig) = icom(false, 0);
    let mut t = Instant::now();
    run(&mut rig, &mut t, 300);
    sim.with(|r| r.power_on = false);
    run(&mut rig, &mut t, 1500);
    assert_eq!(rig.error, Some(RigError::NoReply));
    sim.with(|r| r.power_on = true);
    run(&mut rig, &mut t, 1500);
    assert_eq!(rig.error, None);
}

#[test]
fn text_dialects_against_the_radio() {
    for (dialect, digits, yaesu) in [
        (Dialect::Kenwood, 11, false),
        (Dialect::Yaesu9, 9, true),
        (Dialect::Yaesu8, 8, true),
    ] {
        let mut radio = SimRadio::new(SimProtocol::Text { digits, yaesu });
        radio.chunk = 4;
        let sim = SimHandle::new(radio);
        let mut rig = Rig::new(
            Box::new(TextCat::new(dialect)),
            Box::new(sim.transport()),
            38400,
        );
        let mut t = Instant::now();
        run(&mut rig, &mut t, 400);
        assert_eq!(rig.state.freq_a, Some(7_012_000), "{dialect:?}");
        assert_eq!(rig.state.mode, Some(RigMode::Cw), "{dialect:?}");

        rig.set_freq(3_535_000);
        rig.set_mode(RigMode::Lsb);
        run(&mut rig, &mut t, 400);
        assert_eq!(
            sim.with(|r| (r.freq_a, r.mode)),
            (3_535_000, RigMode::Lsb),
            "{dialect:?}"
        );
        assert_eq!(rig.state.freq_a, Some(3_535_000), "{dialect:?}");
    }
}
