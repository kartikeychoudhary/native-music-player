mod common;
use common::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use engine_core::output::null::{NullBackend, NullHandle, NullMode};
use engine_core::player::{spawn_player, Command, PlayerConfig, PlayerEvent, PlayerHandle};
use protocol::*;

fn setup(mode: OutputMode) -> (PlayerHandle, NullHandle, Receiver<PlayerEvent>) {
    let (backend, h) = NullBackend::new(NullMode::Manual);
    let (tx, rx) = crossbeam_channel::unbounded();
    let p = spawn_player(Arc::new(backend), PlayerConfig::default(), tx);
    p.send(Command::SetMode(mode));
    (p, h, rx)
}

fn queue(p: &PlayerHandle, names: &[&str]) {
    p.send(Command::SetQueue { paths: names.iter().map(|n| fixture(n)).collect(), start: 0 });
    p.send(Command::Play(None));
}

/// Pulls audio until the player reports Ended. Returns every event seen.
fn run_to_end(h: &NullHandle, rx: &Receiver<PlayerEvent>) -> Vec<PlayerEvent> {
    let start = Instant::now();
    let mut events = vec![];
    loop {
        assert!(start.elapsed() < Duration::from_secs(60), "timed out; events: {events:?}");
        let n = h.pull(4096);
        events.extend(rx.try_iter());
        if events.iter().any(|e| matches!(e, PlayerEvent::Ended)) { return events; }
        if n == 0 { std::thread::sleep(Duration::from_millis(1)); }
    }
}

fn wait_for(p: &PlayerHandle, cond: impl Fn(&PlayerState) -> bool) {
    let start = Instant::now();
    while !cond(&p.state()) { assert!(start.elapsed() < Duration::from_secs(10), "state: {:?}", p.state()); std::thread::sleep(Duration::from_millis(2)); }
}

fn pull_frames(h: &NullHandle, frames: usize) {
    let mut got = 0;
    let start = Instant::now();
    while got < frames { got += h.pull((frames - got).min(4096)); assert!(start.elapsed() < Duration::from_secs(10)); std::thread::sleep(Duration::from_micros(200)); }
}

fn f32s(bytes: &[u8]) -> Vec<f32> { bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect() }

#[test]
fn bit_perfect_bytes_match_source_exactly() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    queue(&p, &["flac24_96k.flac"]);
    let events = run_to_end(&h, &rx);
    let expected: Vec<u8> = read_s32le("pcm24_96k.s32le").iter().flat_map(|s| (s >> 8).to_le_bytes()[..3].to_vec()).collect();
    assert_eq!(h.recorded(), expected);
    let sp = events.iter().find_map(|e| if let PlayerEvent::SignalPath(s) = e { Some(s.clone()) } else { None }).unwrap();
    assert!(sp.bit_perfect);
    assert_eq!(sp.bit_perfect_scope, Some(BitPerfectScope::Device));
    assert_eq!(sp.output.format, "s24");
}

#[test]
fn gapless_pair_is_sample_exact_in_processed_mode() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["gap_a.flac", "gap_b.flac"]);
    run_to_end(&h, &rx);
    assert_eq!(f32s(&h.recorded()), read_f32le("gap_src.f32le"));
    assert_eq!(h.opened().len(), 1);
}

#[test]
fn rate_change_in_bit_perfect_reopens_with_note() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    queue(&p, &["flac16_44k.flac", "flac24_96k.flac"]);
    let events = run_to_end(&h, &rx);
    let rates: Vec<u32> = h.opened().iter().map(|(f, _)| f.sample_rate).collect();
    assert_eq!(rates, vec![44_100, 96_000]);
    assert!(events.iter().any(|e| matches!(e, PlayerEvent::SignalPath(s) if s.notes.iter().any(|n| n == "Rate change: brief gap"))));
    let expected_len = 88_200 * 2 * 2 + 192_000 * 2 * 3;
    assert_eq!(h.recorded().len(), expected_len);
}

#[test]
fn processed_mode_resamples_second_track_without_reopening() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["flac16_44k.flac", "flac24_96k.flac"]);
    run_to_end(&h, &rx);
    assert_eq!(h.opened().len(), 1);
    let frames = h.recorded().len() / 8;
    assert!((frames as i64 - 176_400).abs() <= 4, "{frames}");
}

#[test]
fn seek_is_sample_accurate() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["flac16_44k.flac"]);
    pull_frames(&h, 4_410);
    p.send(Command::Seek(1_000));
    wait_for(&p, |s| s.position_ms == 1_000);
    h.clear_recorded();
    run_to_end(&h, &rx);
    assert_eq!(f32s(&h.recorded()), read_f32le("pcm16_44k.f32le")[44_100 * 2..]);
}

#[test]
fn seek_during_preroll_discards_next() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["gap_a.flac", "gap_b.flac"]);
    pull_frames(&h, 44_100);
    std::thread::sleep(Duration::from_millis(300)); // let the ring refill well into gap_b
    p.send(Command::Seek(500));
    wait_for(&p, |s| s.position_ms == 500 && s.queue_index == Some(0));
    h.clear_recorded();
    run_to_end(&h, &rx);
    assert_eq!(f32s(&h.recorded()), read_f32le("gap_src.f32le")[22_050 * 2..]);
}

#[test]
fn seek_past_end_advances() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["short_a.flac", "short_b.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    p.send(Command::Seek(5_000));
    wait_for(&p, |s| s.queue_index == Some(1));
    h.clear_recorded();
    run_to_end(&h, &rx);
    assert_eq!(h.recorded().len() / 8, 22_050);
}

#[test]
fn short_tracks_play_back_to_back() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["short_a.flac", "short_b.flac"]);
    run_to_end(&h, &rx);
    assert_eq!(h.recorded().len() / 8, 2 * 22_050);
}

#[test]
fn empty_file_is_skipped() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["empty.mp3", "short_a.flac"]);
    let events = run_to_end(&h, &rx);
    assert!(events.iter().any(|e| matches!(e, PlayerEvent::Error(err) if err.name == "UNSUPPORTED_FORMAT" && err.path.as_deref().is_some_and(|p| p.ends_with("empty.mp3")))));
    assert_eq!(h.recorded().len() / 8, 22_050);
}

#[test]
fn pause_holds_position_and_resume_continues() {
    let (p, h, _rx) = setup(OutputMode::Processed);
    queue(&p, &["flac16_44k.flac"]);
    pull_frames(&h, 4_410);
    p.send(Command::Pause);
    wait_for(&p, |s| s.status == PlaybackStatus::Paused);
    let before = h.recorded().len();
    for _ in 0..10 { h.pull(4096); }
    assert_eq!(h.recorded().len(), before);
    assert!((p.state().position_ms as i64 - 100).abs() <= 2);
    p.send(Command::Play(None));
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    pull_frames(&h, 100);
}

#[test]
fn volume_scales_processed_output_only() {
    let (p, h, _rx) = setup(OutputMode::Processed);
    p.send(Command::SetVolume(0.5));
    queue(&p, &["flac16_44k.flac"]);
    pull_frames(&h, 44_100);
    let got = f32s(&h.recorded());
    let reference = read_f32le("pcm16_44k.f32le");
    for i in (0..got.len()).step_by(997) { assert!((got[i] - reference[i] * 0.25).abs() < 1e-6); }
}

#[test]
fn device_loss_pauses_and_play_resumes_on_default() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["flac16_44k.flac"]);
    pull_frames(&h, 4_410);
    h.simulate_device_lost();
    wait_for(&p, |s| s.status == PlaybackStatus::Paused);
    let events: Vec<_> = rx.try_iter().collect();
    assert!(events.iter().any(|e| matches!(e, PlayerEvent::Error(err) if err.name == "DEVICE_UNAVAILABLE")));
    assert!(events.iter().any(|e| matches!(e, PlayerEvent::DevicesChanged(_))));
    p.send(Command::Play(None));
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    assert_eq!(h.opened().len(), 2);
    assert!((p.state().position_ms as i64 - 100).abs() <= 2);
}

#[test]
fn exclusive_denied_falls_back_to_shared_with_note() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    h.set_fail_exclusive(true);
    queue(&p, &["short_a.flac"]);
    let events = run_to_end(&h, &rx);
    assert_eq!(h.opened(), vec![(h.opened()[0].0, false)]);
    assert!(events.iter().any(|e| matches!(e, PlayerEvent::SignalPath(s) if !s.bit_perfect && s.notes.iter().any(|n| n.contains("device busy")))));
}

#[test]
fn bluetooth_bit_perfect_reports_os_scope() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    h.set_transport(Transport::Bluetooth);
    queue(&p, &["short_a.flac"]);
    let events = run_to_end(&h, &rx);
    let sp = events.iter().find_map(|e| if let PlayerEvent::SignalPath(s) = e { Some(s.clone()) } else { None }).unwrap();
    assert_eq!(sp.bit_perfect_scope, Some(BitPerfectScope::Os));
    assert_eq!(sp.bluetooth.unwrap().codec, None);
}

#[test]
fn streams_start_without_counting_an_underrun() {
    let (p, h, _rx) = setup(OutputMode::BitPerfect);
    queue(&p, &["flac16_44k.flac", "flac24_96k.flac"]);
    let start = Instant::now();
    // Pulls before a stream's first write must not count as dropouts.
    while h.pull(1) == 0 { assert!(start.elapsed() < Duration::from_secs(10)); }
    assert_eq!(h.underruns(), 0, "first stream");
    while h.opened().len() < 2 {
        assert!(start.elapsed() < Duration::from_secs(10));
        if h.pull(4096) == 0 { std::thread::sleep(Duration::from_micros(200)); }
    }
    while h.pull(1) == 0 { assert!(start.elapsed() < Duration::from_secs(10)); }
    assert_eq!(h.underruns(), 0, "reopened stream");
}

#[test]
fn exclusive_denial_is_sticky_and_keeps_gapless() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    h.set_fail_exclusive(true);
    queue(&p, &["gap_a.flac", "gap_b.flac"]);
    let events = run_to_end(&h, &rx);
    assert_eq!(h.opened().len(), 1);
    assert!(!h.opened()[0].1);
    assert_eq!(f32s(&h.recorded()), read_f32le("gap_src.f32le"));
    let paths: Vec<_> = events.iter().filter_map(|e| if let PlayerEvent::SignalPath(s) = e { Some(s) } else { None }).collect();
    assert!(paths.len() >= 2, "{paths:?}");
    assert!(paths.iter().all(|s| s.notes.iter().any(|n| n.contains("Exclusive access denied (device busy)"))), "{paths:?}");
}
