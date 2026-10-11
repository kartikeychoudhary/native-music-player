//! Run manually with a real output device: `cargo test -p engine-core --test hw_exclusive -- --ignored --nocapture`
use std::sync::atomic::Ordering::*;
use std::time::Duration;

use engine_core::output::format::OutputFormat;
use engine_core::output::ring::ring_pair;
use engine_core::output::{system::SystemBackend, OutputBackend};

#[test]
#[ignore = "needs a real audio device"]
fn exclusive_caps_and_one_second_of_tone() {
    let b = SystemBackend::new();
    let caps = b.caps(None, true).expect("exclusive caps");
    println!("exclusive caps: {caps:?}");
    assert!(caps.exclusive && !caps.rates.is_empty());
    let sample = caps.best_int_for(16).expect("an integer format");
    let rate = if caps.supports_rate(48_000) { 48_000 } else { caps.rates[0] };
    let fmt = OutputFormat { sample_rate: rate, channels: 2, sample };
    let (mut prod, src, shared) = ring_pair(rate as usize, fmt);
    let mut bytes = Vec::new();
    for n in 0..rate as usize {
        let v = ((2.0 * std::f64::consts::PI * 440.0 * n as f64 / rate as f64).sin() * 0.1 * i32::MAX as f64) as i32;
        for _ in 0..2 { engine_core::dsp::convert::write_int_passthrough(&[v], sample, &mut bytes); }
    }
    let mut c = prod.write_chunk(bytes.len().min(prod.slots())).unwrap();
    let (a, rest) = c.as_mut_slices();
    let (al, rl) = (a.len(), rest.len());
    a.copy_from_slice(&bytes[..al]);
    rest.copy_from_slice(&bytes[al..al + rl]);
    c.commit_all();
    let mut s = b.open(None, fmt, true, src).expect("open exclusive");
    s.start().unwrap();
    shared.playing.store(true, Release);
    std::thread::sleep(Duration::from_millis(1200));
    assert!(shared.consumed_frames.load(Acquire) >= rate as u64 * 9 / 10);
    println!("stream: {:?}", s.info());
}
