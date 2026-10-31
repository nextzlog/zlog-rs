mod page;
mod resample;
mod sound;

pub use crate::page::SoundPage;
pub use crate::resample::{resample, Resampler};
pub use crate::sound::{devices, fade, use_no_devices, SoundDrv};
pub use zlog_ui::loopback::{LoopbackDrv, Source};

zlog_core::texts!();

use std::sync::Arc;

use rtty::{Modem, Options, Rtty};
use zlog_ui::loopback::TextSource;

pub const CW_TONE_HZ: f64 = 700.0;

pub fn cw_samples(text: &str, rate: u32) -> Vec<f32> {
    let timing = zlog_keyer::morse::Timing::default();
    let step = std::f64::consts::TAU * CW_TONE_HZ / f64::from(rate.max(1));
    let mut out = Vec::new();
    for s in zlog_keyer::morse::keying(text, &timing) {
        let n = (s.len.as_secs_f64() * f64::from(rate)) as usize;
        out.extend((0..n).map(|i| match s.key {
            true => 0.5 * (step * i as f64).sin() as f32,
            false => 0.0,
        }));
    }
    out.extend(std::iter::repeat_n(0.0, rate as usize));
    out
}

pub fn rtty_samples(text: &str, rate: u32) -> Vec<f32> {
    let o = Options::default();
    let mut out = Rtty::new(rate, o)
        .and_then(|mut m| m.transmit(text, o.rtty.mark, std::time::SystemTime::now()))
        .map(|t| t.audio)
        .unwrap_or_default();
    out.extend(std::iter::repeat_n(0.0, rate as usize));
    out
}

pub fn other_stations() -> Vec<TextSource> {
    vec![
        TextSource {
            id: "cw",
            label: |l| texts().get(l, "source.cw").into(),
            generate: Arc::new(cw_samples),
        },
        TextSource {
            id: "rtty",
            label: |l| texts().get(l, "source.rtty").into(),
            generate: Arc::new(rtty_samples),
        },
    ]
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.page(205, |ctx| {
        SoundPage::new(ctx.new_drivers().take(), ctx.ports.clone())
    });
    b.driver::<dyn zlog_ui::device::AudioDrv>(10, || Box::new(SoundDrv));
    b.driver::<dyn zlog_ui::device::AudioDrv>(20, || {
        Box::new(LoopbackDrv::with_texts(other_stations()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    use zlog_ui::device::{AudioDrv, PortHub};
    use zlog_ui::Section;

    fn heard(source: &str, text: &str, seconds: f64) -> Vec<f32> {
        let drv = LoopbackDrv::with_texts(other_stations());
        let mut s = Section::new();
        for (k, v) in [("rate", "8000"), ("source", source), ("text", text)] {
            s.insert(k.into(), v.into());
        }
        let mut io = drv.open(&s, &PortHub::new()).unwrap();
        let want = (seconds * 8000.0) as usize;
        let mut got = Vec::new();
        let start = std::time::Instant::now();
        while got.len() < want {
            assert!(start.elapsed().as_secs_f64() < seconds + 5.0);
            std::thread::sleep(std::time::Duration::from_millis(20));
            io.record(&mut got);
        }
        got
    }

    #[test]
    fn rtty_from_another_station_decodes_through_the_real_demodulator() {
        let len = rtty_samples("CQ TEST JA1ZLO", 8000).len() as f64 / 8000.0;
        let got = heard("rtty", "CQ TEST JA1ZLO", len);
        let mut rx = Rtty::new(8000, Options::default()).unwrap();
        let words: Vec<String> = rx
            .push(&got, std::time::SystemTime::UNIX_EPOCH)
            .into_iter()
            .map(|d| d.text)
            .collect();
        let text = words.join(" ");
        assert!(text.contains("CQ TEST JA1ZLO"), "{text:?}");
    }

    #[test]
    fn cw_from_another_station_is_keyed_text() {
        let samples = cw_samples("E", 8000);
        let on = samples.iter().filter(|s| s.abs() > 0.1).count();
        let dot = zlog_keyer::morse::Timing::default().dot().as_secs_f64() * 8000.0;
        assert!((on as f64) < dot && (on as f64) > dot * 0.5, "{on} {dot}");
        let got = heard("cw", "E", 0.2);
        assert!(got.iter().any(|s| s.abs() > 0.4));
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
