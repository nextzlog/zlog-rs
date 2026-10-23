//! The CAT drivers against the simulated radio, through real bytes.

use std::time::{Duration, Instant};

use zlog_rig::{Rig, RigError, RigMode, Vfo};
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
        run(&mut rig, &mut t, 800);
        assert_eq!(
            sim.with(|r| (r.freq_a, r.mode)),
            (3_535_000, RigMode::Lsb),
            "{dialect:?}"
        );
        assert_eq!(rig.state.freq_a, Some(3_535_000), "{dialect:?}");
    }
}

fn text(dialect: Dialect) -> (SimHandle, Rig) {
    let (digits, yaesu) = match dialect {
        Dialect::Kenwood => (11, false),
        Dialect::Yaesu9 | Dialect::Ftdx101 => (9, true),
        Dialect::Yaesu8 => (8, true),
    };
    let sim = SimHandle::new(SimRadio::new(SimProtocol::Text { digits, yaesu }));
    let rig = Rig::new(
        Box::new(TextCat::new(dialect)),
        Box::new(sim.transport()),
        38400,
    );
    (sim, rig)
}

/// The operator switches the rig to VFO B: the rig follows VFO B's frequency and
/// commands go to VFO B (`FB`, Yaesu `MD1`), leaving VFO A alone.
#[test]
fn text_rigs_follow_the_vfo_in_use() {
    for dialect in [
        Dialect::Kenwood,
        Dialect::Yaesu9,
        Dialect::Yaesu8,
        Dialect::Ftdx101,
    ] {
        let (sim, mut rig) = text(dialect);
        let mut t = Instant::now();
        run(&mut rig, &mut t, 400);
        assert_eq!(rig.state.vfo, Vfo::A, "{dialect:?}");
        assert_eq!(rig.state.freq(), Some(7_012_000), "{dialect:?}");

        sim.select_vfo(Vfo::B);
        run(&mut rig, &mut t, 400);
        assert_eq!(rig.state.vfo, Vfo::B, "{dialect:?}");
        assert_eq!(rig.state.freq(), Some(7_100_000), "{dialect:?}");

        rig.set_freq(14_025_000);
        rig.set_mode(RigMode::Usb);
        run(&mut rig, &mut t, 400);
        assert_eq!(
            sim.with(|r| (r.freq_a, r.freq_b, r.mode)),
            (7_012_000, 14_025_000, RigMode::Usb),
            "{dialect:?}"
        );
        assert_eq!(rig.state.freq(), Some(14_025_000), "{dialect:?}");
        assert_eq!(rig.state.mode, Some(RigMode::Usb), "{dialect:?}");

        sim.select_vfo(Vfo::A);
        run(&mut rig, &mut t, 400);
        assert_eq!(rig.state.vfo, Vfo::A, "{dialect:?}");
        assert_eq!(rig.state.freq(), Some(7_012_000), "{dialect:?}");
    }
}

#[test]
fn kenwood_is_polled_by_if() {
    let (sim, mut rig) = text(Dialect::Kenwood);
    let mut t = Instant::now();
    run(&mut rig, &mut t, 400);
    let sent = sim.with(|r| r.received.clone());
    assert_eq!(sent[0], b"IF;");
    assert!(sent.iter().all(|c| c != b"MD;"));
}

#[test]
fn data_mode_sends_nothing() {
    let (sim, mut rig) = text(Dialect::Yaesu9);
    let (isim, mut irig) = icom(false, 0);
    let mut t = Instant::now();
    rig.set_mode(RigMode::Data);
    irig.set_mode(RigMode::Data);
    run(&mut rig, &mut t, 300);
    run(&mut irig, &mut t, 300);
    assert!(sim.with(|r| r.received.iter().all(|c| !c.starts_with(b"MD"))));
    assert!(isim.with(|r| r.received.iter().all(|c| c[4] != 0x06)));
    assert_eq!(sim.with(|r| r.mode), RigMode::Cw);
}

#[test]
fn transverter_offset_against_the_radio() {
    let (sim, mut rig) = text(Dialect::Kenwood);
    rig.freq_offset = 1_150_000_000;
    let mut t = Instant::now();
    rig.set_freq(1_294_050_000);
    run(&mut rig, &mut t, 400);
    assert_eq!(sim.with(|r| r.freq_a), 144_050_000);
    assert_eq!(rig.state.freq(), Some(1_294_050_000));
}

/// Icom DV both ways, and a mode outside zLog's table read as Other.
#[test]
fn icom_dv_against_the_radio() {
    let (sim, mut rig) = icom(true, 2);
    let mut t = Instant::now();
    rig.set_mode(RigMode::Dv);
    run(&mut rig, &mut t, 1500);
    assert_eq!(sim.with(|r| r.mode), RigMode::Dv);
    assert_eq!(rig.state.mode, Some(RigMode::Dv));
    sim.press_mode(RigMode::Other);
    run(&mut rig, &mut t, 1500);
    assert_eq!(rig.state.mode, Some(RigMode::Other));
    // Other is never sent.
    sim.with(|r| r.received.clear());
    rig.set_mode(RigMode::Other);
    run(&mut rig, &mut t, 1500);
    assert!(sim.with(|r| r.received.iter().all(|c| c[4] != 0x06)));
}
