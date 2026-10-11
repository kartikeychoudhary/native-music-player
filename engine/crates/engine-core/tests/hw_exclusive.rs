//! Run manually with a real output device: `cargo test -p engine-core --test hw_exclusive -- --ignored --nocapture`
//! `AURORA_HW_DEVICE=<device id>` picks the device (default: the system default). With
//! `AURORA_HW_SILENT=1` the stream carries silence, for checking timing on a device someone can hear.
use std::sync::atomic::Ordering::*;
use std::time::{Duration, Instant};

use engine_core::output::format::OutputFormat;
use engine_core::output::ring::ring_pair;
use engine_core::output::{system::SystemBackend, OutputBackend};

#[test]
#[ignore = "needs a real audio device"]
fn exclusive_stream_consumes_in_real_time() {
    let b = SystemBackend::new();
    let device = std::env::var("AURORA_HW_DEVICE").ok();
    let silent = std::env::var("AURORA_HW_SILENT").is_ok();
    let caps = b.caps(device.as_deref(), true).expect("exclusive caps");
    println!("exclusive caps: {caps:?}");
    assert!(caps.exclusive && !caps.rates.is_empty());
    let sample = caps.best_int_for(16).expect("an integer format");
    let rate = if caps.supports_rate(44_100) { 44_100 } else { caps.rates[0] };
    let fmt = OutputFormat { sample_rate: rate, channels: 2, sample };
    let secs = 4;
    let (mut prod, src, shared) = ring_pair(rate as usize * secs, fmt);
    let mut bytes = Vec::new();
    for n in 0..rate as usize * secs {
        let v = if silent { 0 } else { ((2.0 * std::f64::consts::PI * 440.0 * n as f64 / rate as f64).sin() * 0.1 * i32::MAX as f64) as i32 };
        for _ in 0..2 { engine_core::dsp::convert::write_int_passthrough(&[v], sample, &mut bytes); }
    }
    let mut c = prod.write_chunk(bytes.len().min(prod.slots())).unwrap();
    let (a, rest) = c.as_mut_slices();
    let (al, rl) = (a.len(), rest.len());
    a.copy_from_slice(&bytes[..al]);
    rest.copy_from_slice(&bytes[al..al + rl]);
    c.commit_all();
    let mut s = b.open(device.as_deref(), fmt, true, src).expect("open exclusive");
    s.start().unwrap();
    shared.playing.store(true, Release);
    // Measured after a settling second, so stream start-up doesn't count: the device must take
    // audio at its sample rate (a device fed too slowly plays slow, "robotic" audio).
    std::thread::sleep(Duration::from_secs(1));
    let (f0, t0) = (shared.consumed_frames.load(Acquire), Instant::now());
    std::thread::sleep(Duration::from_secs(2));
    let rate_seen = (shared.consumed_frames.load(Acquire) - f0) as f64 / t0.elapsed().as_secs_f64();
    println!("stream: {:?}; consumed {rate_seen:.0} frames/s at {rate} Hz", s.info());
    assert!((rate_seen / rate as f64 - 1.0).abs() < 0.03, "consumed {rate_seen:.0} frames/s at {rate} Hz");
}
