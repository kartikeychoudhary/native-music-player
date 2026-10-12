mod common;
use common::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use engine_core::output::null::{NullBackend, NullHandle, NullMode};
use engine_core::output::DeviceCaps;
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
    p.send(Command::SetQueue { items: names.iter().map(|n| engine_core::player::NewEntry::from_path(fixture(n))).collect(), start: 0 });
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

/// Waits until the player has written at least `frames` frames ahead of the output (nothing is
/// pulled meanwhile). Replaces sleeping for "long enough": the feeder, not the clock, decides.
fn wait_buffered(h: &NullHandle, frames: usize) {
    let start = Instant::now();
    while h.buffered_frames() < frames { assert!(start.elapsed() < Duration::from_secs(10), "only {} frames buffered, want {frames}", h.buffered_frames()); std::thread::sleep(Duration::from_millis(1)); }
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

fn s16_bytes(f: &[f32]) -> Vec<u8> { f.iter().flat_map(|v| ((v * 32_768.0).round() as i16).to_le_bytes()).collect() }

fn signal_paths(events: &[PlayerEvent]) -> Vec<&SignalPath> {
    events.iter().filter_map(|e| if let PlayerEvent::SignalPath(s) = e { Some(s) } else { None }).collect()
}

#[test]
fn exclusive_stream_keeps_its_caps_while_it_holds_the_device() {
    // A real exclusive backend cannot re-probe a device our own stream holds; the player must reuse the caps it opened with.
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    h.set_fail_caps_while_open(true);
    queue(&p, &["gap_a.flac", "gap_b.flac"]);
    let events = run_to_end(&h, &rx);
    assert_eq!(h.opened().len(), 1);
    assert!(h.opened()[0].1, "stream must be exclusive");
    assert_eq!(h.recorded(), s16_bytes(&read_f32le("gap_src.f32le")));
    let paths = signal_paths(&events);
    assert!(!paths.is_empty());
    assert!(paths.iter().all(|s| s.bit_perfect && s.notes.iter().all(|n| !n.contains("Exclusive mode unavailable"))), "{paths:?}");
}

#[test]
fn seek_in_exclusive_mode_does_not_reopen_when_caps_are_busy() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    h.set_fail_caps_while_open(true);
    queue(&p, &["gap_a.flac"]);
    pull_frames(&h, 4_410);
    p.send(Command::Seek(500));
    wait_for(&p, |s| s.position_ms == 500);
    h.clear_recorded();
    run_to_end(&h, &rx);
    assert_eq!(h.opened().len(), 1);
    assert!(h.opened()[0].1);
    assert_eq!(h.recorded(), s16_bytes(&read_f32le("gap_src.f32le")[..110_583 * 2])[22_050 * 4..]);
}

#[test]
fn caps_time_exclusive_denial_reports_its_reason() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    h.set_deny_exclusive_caps(true);
    queue(&p, &["short_a.flac"]);
    let events = run_to_end(&h, &rx);
    assert!(!h.opened()[0].1);
    let paths = signal_paths(&events);
    assert!(paths.iter().any(|s| !s.bit_perfect && s.notes.iter().any(|n| n.contains("device busy"))), "{paths:?}");
}

#[test]
fn bit_perfect_fallback_ignores_software_volume() {
    // Bit-perfect mode hides the volume slider, so a fallback to a processed stream must stay at unity gain.
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    p.send(Command::SetVolume(0.5));
    h.set_fail_exclusive(true);
    queue(&p, &["flac16_44k.flac"]);
    let events = run_to_end(&h, &rx);
    assert!(!h.opened()[0].1, "fallback stream must be shared");
    assert_eq!(f32s(&h.recorded()), read_f32le("pcm16_44k.f32le"));
    let paths = signal_paths(&events);
    assert!(!paths.is_empty());
    for sp in paths {
        let vol = sp.stages.iter().find(|s| s.kind == StageKind::Volume).unwrap();
        assert!(!vol.active, "{vol:?}");
        assert_eq!(vol.detail, "100%");
    }
}

#[test]
fn any_exclusive_open_failure_falls_back_to_shared_with_note() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    h.set_fail_exclusive_format(true);
    queue(&p, &["short_a.flac"]);
    let events = run_to_end(&h, &rx);
    assert_eq!(h.opened().len(), 1);
    assert!(!h.opened()[0].1, "fallback stream must be shared");
    assert_eq!(h.recorded().len() / 8, 22_050);
    let paths = signal_paths(&events);
    assert!(!paths.is_empty());
    assert!(paths.iter().all(|s| !s.bit_perfect && s.notes.iter().any(|n| n == "Exclusive open failed (unsupported output format: driver rejected the format) — using shared mode")), "{paths:?}");
}

#[test]
fn user_next_re_probes_after_an_exclusive_denial() {
    let (p, h, _rx) = setup(OutputMode::BitPerfect);
    h.set_fail_exclusive(true);
    queue(&p, &["short_a.flac", "short_b.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    assert!(!h.opened()[0].1);
    h.set_fail_exclusive(false);
    p.send(Command::Next);
    wait_for(&p, |s| s.queue_index == Some(1) && s.status == PlaybackStatus::Playing);
    assert_eq!(h.opened().len(), 2);
    assert!(h.opened()[1].1, "a user-initiated start must re-probe exclusive access");
}

#[test]
fn play_from_stopped_re_probes_after_a_caps_time_denial() {
    let (p, h, _rx) = setup(OutputMode::BitPerfect);
    h.set_deny_exclusive_caps(true);
    queue(&p, &["short_a.flac", "short_b.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    assert!(!h.opened()[0].1);
    h.set_deny_exclusive_caps(false);
    p.send(Command::Stop);
    wait_for(&p, |s| s.status == PlaybackStatus::Stopped);
    p.send(Command::Play(None));
    wait_for(&p, |s| s.status == PlaybackStatus::Playing && h.opened().len() == 2);
    assert!(h.opened()[1].1, "Play from Stopped must re-probe exclusive access");
}

#[test]
fn bluetooth_details_are_queried_once_per_stream() {
    let (p, h, rx) = setup(OutputMode::Processed);
    h.set_transport(Transport::Bluetooth);
    queue(&p, &["gap_a.flac", "gap_b.flac"]);
    let events = run_to_end(&h, &rx);
    assert_eq!(h.opened().len(), 1);
    assert!(signal_paths(&events).len() >= 2);
    assert_eq!(h.bluetooth_queries(), 1);
}

#[test]
fn exclusive_caps_failure_reason_is_in_the_note() {
    // e.g. Linux: PipeWire can't tell which ALSA device the sink uses, so exclusive mode is unavailable.
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    h.set_caps(DeviceCaps { exclusive: false, ..DeviceCaps::fallback() });
    queue(&p, &["short_a.flac"]);
    let events = run_to_end(&h, &rx);
    assert!(!h.opened()[0].1);
    let paths = signal_paths(&events);
    assert!(paths.iter().all(|s| s.notes == vec!["Exclusive mode unavailable (exclusive mode not available) — using processed mode".to_string()]), "{paths:?}");
}

/// Pulls small blocks at about real time (256 frames per 5 ms), like a device callback, until the returned flag is set.
fn live_puller(h: &NullHandle) -> (Arc<std::sync::atomic::AtomicBool>, std::thread::JoinHandle<()>) {
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (h, s) = (h.clone(), stop.clone());
    let join = std::thread::spawn(move || while !s.load(std::sync::atomic::Ordering::Relaxed) { h.pull(256); std::thread::sleep(Duration::from_millis(5)); });
    (stop, join)
}

/// Largest jump between consecutive samples of one channel (interleaved stereo): a cut
/// mid-waveform is a jump far beyond what the music itself does, heard as a click.
fn max_jump(samples: &[f32]) -> f32 {
    samples.windows(3).map(|w| (w[2] - w[0]).abs()).fold(0.0, f32::max)
}

#[test]
fn interrupting_playback_fades_out_instead_of_cutting() {
    for interrupt in [Command::Pause, Command::Stop, Command::Next, Command::Seek(1500)] {
        let (p, h, _rx) = setup(OutputMode::Processed);
        queue(&p, &["flac16_44k.flac", "flac16_44k.flac"]);
        wait_for(&p, |s| s.status == PlaybackStatus::Playing);
        let (stop, join) = live_puller(&h);
        wait_for(&p, |s| s.position_ms >= 300);
        let label = format!("{interrupt:?}");
        let before = h.recorded().len();
        let after = match interrupt { Command::Next => 1, _ => 0 };
        p.send(interrupt);
        wait_for(&p, |s| s.queue_index == Some(after));
        std::thread::sleep(Duration::from_millis(150));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        join.join().unwrap();
        let rec = f32s(&h.recorded());
        let natural = max_jump(&rec[before / 4 - 8820..before / 4]);
        let worst = max_jump(&rec[before / 4 - 2..]);
        assert!(worst < natural * 2.0, "{label}: a {worst:.4} jump at the interruption; the music itself moves at most {natural:.4}");
    }
}

fn eq(enabled: bool, bands: [f32; 10], preamp: f32, preset: Option<&str>) -> EqSettings {
    EqSettings { enabled, bands: bands.to_vec(), preamp, preset: preset.map(Into::into) }
}

fn peak(bytes: &[u8]) -> f32 { f32s(bytes).iter().fold(0f32, |m, v| m.max(v.abs())) }

#[test]
fn eq_changes_are_heard_immediately_not_after_the_buffer() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["flac16_44k.flac"]);
    pull_frames(&h, 22_050);
    std::thread::sleep(Duration::from_millis(200)); // the ring now holds ~2 s decoded without EQ
    let before = peak(&h.recorded());
    p.send(Command::SetEq(eq(true, [0.0; 10], -12.0, None)));
    let start = Instant::now();
    while !rx.try_iter().any(|e| matches!(e, PlayerEvent::SignalPath(s) if s.stages.iter().any(|st| st.kind == StageKind::Eq && st.active))) {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(2));
    }
    h.clear_recorded();
    pull_frames(&h, 4_410);
    let ratio = peak(&h.recorded()) / before;
    assert!((ratio - 0.251).abs() < 0.03, "-12 dB within 100 ms of buffered audio, got x{ratio:.3}");
}

#[test]
fn bit_perfect_ignores_eq() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    p.send(Command::SetEq(eq(true, [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], -6.0, Some("Bass"))));
    queue(&p, &["flac24_96k.flac"]);
    let events = run_to_end(&h, &rx);
    let expected: Vec<u8> = read_s32le("pcm24_96k.s32le").iter().flat_map(|s| (s >> 8).to_le_bytes()[..3].to_vec()).collect();
    assert_eq!(h.recorded(), expected);
    let sp = events.iter().find_map(|e| if let PlayerEvent::SignalPath(s) = e { Some(s.clone()) } else { None }).unwrap();
    let st = sp.stages.iter().find(|s| s.kind == StageKind::Eq).unwrap();
    assert_eq!((st.active, st.detail.as_str()), (false, "bypassed (bit-perfect)"));
}

#[test]
fn eq_stage_names_the_preset_and_follows_changes() {
    let (p, h, rx) = setup(OutputMode::Processed);
    p.send(Command::SetEq(eq(true, [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 0.0, Some("Bass"))));
    queue(&p, &["flac16_44k.flac"]);
    let eq_stage = |rx: &Receiver<PlayerEvent>| {
        let start = Instant::now();
        loop {
            assert!(start.elapsed() < Duration::from_secs(5));
            pull_frames(&h, 256);
            if let Some(st) = rx.try_iter().filter_map(|e| if let PlayerEvent::SignalPath(s) = e { s.stages.into_iter().find(|s| s.kind == StageKind::Eq) } else { None }).last() { return st; }
        }
    };
    let st = eq_stage(&rx);
    assert_eq!((st.active, st.detail.as_str()), (true, "Bass"));
    p.send(Command::SetEq(eq(false, [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 0.0, Some("Bass"))));
    let st = eq_stage(&rx);
    assert_eq!((st.active, st.detail.as_str()), (false, "off"));
    p.send(Command::SetEq(eq(true, [0.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0], 0.0, None)));
    assert_eq!(eq_stage(&rx).detail, "Custom");
}

#[test]
fn eq_stage_change_reaches_a_track_that_is_queued_but_not_yet_audible() {
    let (p, h, rx) = setup(OutputMode::Processed);
    queue(&p, &["gap_a.flac", "gap_b.flac"]);
    pull_frames(&h, 44_100);
    // More than (gap_a's end - what was pulled) buffered means the feeder wrote past gap_a's last frame, so it
    // had already started gap_b (pushing its marker) while gap_b is still 1.4 s from audible.
    wait_buffered(&h, GAP_A - 44_100 + 1);
    let is_eq_active = |e: &PlayerEvent| matches!(e, PlayerEvent::SignalPath(s) if s.stages.iter().any(|st| st.kind == StageKind::Eq && st.active));
    p.send(Command::SetEq(eq(true, [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], 0.0, Some("Bass"))));
    // Pull nothing until the player has applied the change (it re-sends the audible track's path):
    // pulling on would make gap_b audible before the command is handled.
    let mut events = vec![];
    let start = Instant::now();
    while !events.iter().any(is_eq_active) {
        assert!(start.elapsed() < Duration::from_secs(10), "the change was never applied");
        events.extend(rx.try_iter());
        std::thread::sleep(Duration::from_millis(1));
    }
    events.extend(run_to_end(&h, &rx));
    let paths: Vec<SignalPath> = events.into_iter().filter_map(|e| if let PlayerEvent::SignalPath(s) = e { Some(s) } else { None }).collect();
    let refreshed = paths.iter().position(|sp| sp.stages.iter().any(|st| st.kind == StageKind::Eq && st.active)).unwrap();
    assert!(refreshed + 1 < paths.len(), "gap_a's refresh, then gap_b becoming audible: {} signal paths", paths.len());
    for sp in &paths[refreshed..] {
        let st = sp.stages.iter().find(|s| s.kind == StageKind::Eq).unwrap();
        assert_eq!((st.active, st.detail.as_str()), (true, "Bass"));
    }
}

fn rg_entry(name: &str, track_gain: f32) -> engine_core::player::NewEntry {
    engine_core::player::NewEntry {
        replay_gain: ReplayGainInfo { track_gain: Some(track_gain), track_peak: Some(0.125), ..Default::default() },
        ..engine_core::player::NewEntry::from_path(fixture(name))
    }
}

#[test]
fn replay_gain_scales_processed_output_and_follows_the_mode() {
    let (p, h, rx) = setup(OutputMode::Processed);
    p.send(Command::SetReplayGain(ReplayGainMode::Track));
    p.send(Command::SetQueue { items: vec![rg_entry("flac16_44k.flac", -6.0206)], start: 0 });
    p.send(Command::Play(None));
    let events = run_to_end(&h, &rx);
    let half = read_f32le("pcm16_44k.f32le").iter().map(|v| v * 0.5).collect::<Vec<f32>>();
    let got = f32s(&h.recorded());
    assert_eq!(got.len(), half.len());
    assert!(got.iter().zip(&half).all(|(a, b)| (a - b).abs() < 1e-5), "-6.02 dB = half amplitude");
    let st = events.iter().filter_map(|e| if let PlayerEvent::SignalPath(s) = e { s.stages.iter().find(|s| s.kind == StageKind::ReplayGain).cloned() } else { None }).next_back().unwrap();
    assert_eq!((st.active, st.detail.as_str()), (true, "-6.0 dB (track)"));
}

#[test]
fn switching_replay_gain_mode_mid_track_is_heard_at_once() {
    let (p, h, rx) = setup(OutputMode::Processed);
    p.send(Command::SetQueue { items: vec![rg_entry("flac16_44k.flac", -6.0206)], start: 0 });
    p.send(Command::Play(None));
    pull_frames(&h, 22_050);
    std::thread::sleep(Duration::from_millis(200));
    let before = peak(&h.recorded());
    p.send(Command::SetReplayGain(ReplayGainMode::Track));
    let start = Instant::now();
    while !rx.try_iter().any(|e| matches!(e, PlayerEvent::SignalPath(s) if s.stages.iter().any(|st| st.kind == StageKind::ReplayGain && st.active))) {
        assert!(start.elapsed() < Duration::from_secs(5));
        pull_frames(&h, 256);
    }
    h.clear_recorded();
    pull_frames(&h, 8_820);
    let after = f32s(&h.recorded());
    let tail_peak = after[after.len() / 2..].iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!((tail_peak / before - 0.5).abs() < 0.03, "x{:.3}", tail_peak / before);
}

#[test]
fn bit_perfect_ignores_replay_gain() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    p.send(Command::SetReplayGain(ReplayGainMode::Track));
    p.send(Command::SetQueue { items: vec![rg_entry("flac24_96k.flac", -6.0)], start: 0 });
    p.send(Command::Play(None));
    run_to_end(&h, &rx);
    let expected: Vec<u8> = read_s32le("pcm24_96k.s32le").iter().flat_map(|s| (s >> 8).to_le_bytes()[..3].to_vec()).collect();
    assert_eq!(h.recorded(), expected);
}

/// Pulls (so a restart's fade-out can finish) until the player publishes a signal path matching `pred`.
fn pull_until_signal_path(h: &NullHandle, rx: &Receiver<PlayerEvent>, pred: impl Fn(&SignalPath) -> bool) -> SignalPath {
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < Duration::from_secs(10), "no matching signal path");
        h.pull(256);
        if let Some(sp) = rx.try_iter().filter_map(|e| if let PlayerEvent::SignalPath(s) = e { Some(s) } else { None }).find(&pred) { return sp; }
        std::thread::sleep(Duration::from_micros(200));
    }
}

#[test]
fn leaving_bit_perfect_brings_eq_replay_gain_and_crossfade_back() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    p.send(Command::SetEq(eq(true, [0.0; 10], -12.0, None)));
    p.send(Command::SetReplayGain(ReplayGainMode::Track));
    p.send(Command::SetCrossfade(1.0));
    p.send(Command::SetQueue { items: vec![rg_entry("flac16_44k.flac", -6.0206)], start: 0 });
    p.send(Command::Play(None));
    // Bit-perfect: the source's samples, untouched, with every DSP stage bypassed.
    let sp = pull_until_signal_path(&h, &rx, |_| true);
    assert!(sp.bit_perfect);
    for kind in [StageKind::ReplayGain, StageKind::Eq, StageKind::Crossfade] {
        let st = sp.stages.iter().find(|s| s.kind == kind).unwrap();
        assert_eq!((st.active, st.detail.as_str()), (false, "bypassed (bit-perfect)"), "{kind:?}");
    }
    pull_frames(&h, 4_410);
    let src = read_f32le("pcm16_44k.f32le");
    let got = h.recorded();
    assert!(got.len() >= 4_410 * 4);
    let expect: Vec<u8> = src[..got.len() / 2].iter().flat_map(|v| ((v * 32_768.0).round() as i16).to_le_bytes()).collect();
    assert!(got == expect, "bit-perfect bytes are the source's");

    // Back to processed: the stream restarts and all three stages are live again.
    p.send(Command::SetMode(OutputMode::Processed));
    let sp = pull_until_signal_path(&h, &rx, |s| !s.bit_perfect);
    for (kind, detail) in [(StageKind::ReplayGain, "-6.0 dB (track)"), (StageKind::Eq, "Custom"), (StageKind::Crossfade, "1 s")] {
        let st = sp.stages.iter().find(|s| s.kind == kind).unwrap();
        assert_eq!((st.active, st.detail.as_str()), (true, detail), "{kind:?}");
    }
    assert_eq!(h.opened().last().unwrap().0.sample, engine_core::output::format::DeviceSample::F32, "a processed stream");
    h.clear_recorded();
    pull_frames(&h, 8_820);
    let want = peak(&src.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()) * 0.251_189 * 0.5;
    let got = peak(&h.recorded());
    assert!((got / want - 1.0).abs() < 0.03, "-12 dB preamp and -6 dB ReplayGain: peak {got:.5}, want {want:.5}");
}

const GAP_A: usize = 110_583;
const GAP_B: usize = 6 * 44_100 - GAP_A;
const SHORT: usize = 22_050;

fn album_entry(name: &str, album: Option<i64>, track: Option<u32>) -> engine_core::player::NewEntry {
    engine_core::player::NewEntry { album_id: album, disc_no: Some(1), track_no: track, ..engine_core::player::NewEntry::from_path(fixture(name)) }
}

fn play_entries(p: &PlayerHandle, items: Vec<engine_core::player::NewEntry>) {
    p.send(Command::SetQueue { items, start: 0 });
    p.send(Command::Play(None));
}

/// A player whose ring holds only 0.5 s, so a 1 s crossfade's incoming track becomes audible while
/// the mix is still being decoded (the outgoing track still feeds, behind the audible one).
fn setup_small_ring() -> (PlayerHandle, NullHandle, Receiver<PlayerEvent>) {
    let (backend, h) = NullBackend::new(NullMode::Manual);
    let (tx, rx) = crossbeam_channel::unbounded();
    let p = spawn_player(Arc::new(backend), PlayerConfig { buffer_secs: 0.5, ..PlayerConfig::default() }, tx);
    p.send(Command::SetMode(OutputMode::Processed));
    (p, h, rx)
}

/// A fixture's decoded samples (interleaved stereo), as the processed chain sees them.
fn decoded(name: &str) -> Vec<f64> {
    use engine_core::decode::TrackDecoder;
    let mut d: Box<dyn TrackDecoder> = engine_core::decode::open(&fixture(name)).unwrap();
    let mut out = vec![];
    while let Some(b) = d.next_block().unwrap() { b.to_f64(&mut out); }
    out
}

/// Asserts that `got` (stereo f32) is `a`, then `a`'s last `total` frames equal-power-mixed with
/// `b`'s first `total` frames, then the rest of `b`: the shape of one crossfade.
fn assert_crossfade(got: &[f32], a: &[f64], b: &[f64], total: usize) {
    let (fa, fb) = (a.len() / 2, b.len() / 2);
    assert_eq!(got.len() / 2, fa + fb - total, "output = len(a) + len(b) - overlap");
    let un = (fa - total) * 2;
    assert!(got[..un].iter().zip(&a[..un]).all(|(g, s)| *g == *s as f32), "outgoing track unmixed before the overlap");
    for k in 0..total {
        let (go, gi) = engine_core::dsp::crossfade::equal_power(k as f64 / total as f64);
        for c in 0..2 {
            let want = a[un + k * 2 + c] * go + b[k * 2 + c] * gi;
            let g = got[un + k * 2 + c] as f64;
            assert!((g - want).abs() < 1e-6, "overlap frame {k} ch {c}: got {g}, want {want}");
        }
    }
    assert!(got[fa * 2..].iter().zip(&b[total * 2..]).all(|(g, s)| *g == *s as f32), "incoming track unmixed after the overlap");
}

/// For gap_a → gap_b with a 1 s crossfade: pulls exactly to 50 ms into the overlap, where gap_b
/// (queue entry 1) is audible, and waits until the player reports it. Pulling an exact count (not
/// polling the state while pulling) keeps the playhead from overshooting when the machine is busy,
/// and makes the position unambiguous (a later wait for another position can't match it by chance).
fn pull_into_overlap(p: &PlayerHandle, h: &NullHandle) {
    pull_frames(h, GAP_A - 44_100 + 2_205);
    wait_for(p, |s| s.queue_index == Some(1) && s.position_ms == 50);
}

/// Waits until a queue edit is applied (its queue.changed has `len` items), so pulls can't race its resync.
fn wait_queue_len(rx: &Receiver<PlayerEvent>, len: usize) {
    let start = Instant::now();
    while !rx.try_iter().any(|e| matches!(e, PlayerEvent::Queue(q) if q.items.len() == len)) {
        assert!(start.elapsed() < Duration::from_secs(10), "edit not applied");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn crossfade_overlaps_consecutive_tracks() {
    let (p, h, rx) = setup(OutputMode::Processed);
    p.send(Command::SetCrossfade(1.0));
    play_entries(&p, vec![album_entry("gap_a.flac", None, None), album_entry("gap_b.flac", None, None)]);
    let events = run_to_end(&h, &rx);
    assert_eq!(h.recorded().len() / 8, GAP_A + GAP_B - 44_100, "one second of overlap");
    assert_crossfade(&f32s(&h.recorded()), &decoded("gap_a.flac"), &decoded("gap_b.flac"), 44_100);
    assert_eq!(h.opened().len(), 1, "same stream throughout");
    let st = events.iter().filter_map(|e| if let PlayerEvent::SignalPath(s) = e { s.stages.iter().find(|s| s.kind == StageKind::Crossfade).cloned() } else { None }).next_back().unwrap();
    assert_eq!((st.active, st.detail.as_str()), (true, "1 s"));
}

#[test]
fn smart_skip_keeps_an_album_played_in_order_gapless() {
    let (p, h, rx) = setup(OutputMode::Processed);
    p.send(Command::SetCrossfade(1.0));
    play_entries(&p, vec![album_entry("gap_a.flac", Some(7), Some(1)), album_entry("gap_b.flac", Some(7), Some(2))]);
    run_to_end(&h, &rx);
    assert_eq!(f32s(&h.recorded()), read_f32le("gap_src.f32le"), "sample-exact gapless join");
}

#[test]
fn crossfade_shrinks_for_short_tracks() {
    let (p, h, rx) = setup(OutputMode::Processed);
    p.send(Command::SetCrossfade(4.0));
    play_entries(&p, vec![album_entry("short_a.flac", None, None), album_entry("short_b.flac", None, None)]);
    run_to_end(&h, &rx);
    assert_eq!(h.recorded().len() / 8, 2 * SHORT - SHORT / 2, "overlap = half the shorter track");
    assert_crossfade(&f32s(&h.recorded()), &decoded("short_a.flac"), &decoded("short_b.flac"), SHORT / 2);
}

#[test]
fn repeat_one_replays_are_not_crossfaded() {
    let (p, h, _rx) = setup(OutputMode::Processed);
    p.send(Command::SetCrossfade(1.0));
    p.send(Command::SetRepeat(RepeatMode::One));
    play_entries(&p, vec![album_entry("gap_a.flac", None, None)]);
    pull_frames(&h, 2 * GAP_A);
    let src = read_f32le("gap_src.f32le");
    let once = &src[..GAP_A * 2];
    let got = f32s(&h.recorded());
    assert_eq!(&got[..GAP_A * 2], once);
    assert_eq!(&got[GAP_A * 2..GAP_A * 4], once, "second play starts cleanly");
}

#[test]
fn bit_perfect_never_crossfades() {
    let (p, h, rx) = setup(OutputMode::BitPerfect);
    p.send(Command::SetCrossfade(1.0));
    play_entries(&p, vec![album_entry("gap_a.flac", None, None), album_entry("gap_b.flac", None, None)]);
    run_to_end(&h, &rx);
    let bpf = h.opened()[0].0.bytes_per_frame();
    assert_eq!(h.recorded().len() / bpf, GAP_A + GAP_B);
}

#[test]
fn seek_during_crossfade_plays_one_track() {
    let (p, h, rx) = setup(OutputMode::Processed);
    p.send(Command::SetCrossfade(1.0));
    play_entries(&p, vec![album_entry("gap_a.flac", None, None), album_entry("gap_b.flac", None, None)]);
    // Into the overlap: gap_b becomes current where the mix starts.
    pull_into_overlap(&p, &h);
    p.send(Command::Seek(500));
    wait_for(&p, |s| s.queue_index == Some(1) && s.position_ms == 500);
    h.clear_recorded();
    run_to_end(&h, &rx);
    assert_eq!(f32s(&h.recorded()), read_f32le("gap_src.f32le")[(GAP_A + 22_050) * 2..], "only gap_b, from 0.5 s, unmixed");
}

#[test]
fn removing_the_incoming_track_during_crossfade_resyncs() {
    let (p, h, rx) = setup(OutputMode::Processed);
    p.send(Command::SetCrossfade(1.0));
    play_entries(&p, vec![album_entry("gap_a.flac", None, None), album_entry("gap_b.flac", None, None), album_entry("short_a.flac", None, None)]);
    pull_frames(&h, 4_410);
    // The ring is full (2 s, minus the feeder's free-space margin): 4_410 + 88_200 written is past the point
    // where gap_a's last 2 s (overlap + lead) begin, so the crossfade state exists and its mix is under way.
    wait_buffered(&h, 80_000);
    let gap_b = p.state().queue_item_id.unwrap() + 1;
    p.send(Command::Remove(vec![gap_b]));
    // Pull only once the edit (and its same-position resync) is applied: a pull racing the resync
    // could read stale frames after the resync took its position, and they would be heard twice.
    wait_queue_len(&rx, 2);
    run_to_end(&h, &rx);
    assert_eq!(h.recorded().len() / 8, GAP_A + SHORT - SHORT / 2, "gap_a crossfades into short_a (overlap 0.25 s); no gap_b audio");
    assert_crossfade(&f32s(&h.recorded()), &decoded("gap_a.flac"), &decoded("short_a.flac"), SHORT / 2);
}

#[test]
fn seek_while_the_mix_is_audible_plays_only_the_seeked_track() {
    // The 0.5 s ring is shorter than the 1 s overlap, so gap_b becomes audible while the mix is still
    // being written: with the playhead 50 ms into gap_b, at most 0.55 s of the mix is written, and the
    // hand-over comes only after all 1 s. The seek lands with the crossfade active, gap_b at the front
    // and gap_a still feeding.
    let (p, h, rx) = setup_small_ring();
    p.send(Command::SetCrossfade(1.0));
    play_entries(&p, vec![album_entry("gap_a.flac", None, None), album_entry("gap_b.flac", None, None)]);
    pull_into_overlap(&p, &h);
    p.send(Command::Seek(500));
    wait_for(&p, |s| s.queue_index == Some(1) && s.position_ms == 500);
    h.clear_recorded();
    run_to_end(&h, &rx);
    assert_eq!(f32s(&h.recorded()), read_f32le("gap_src.f32le")[(GAP_A + 22_050) * 2..], "only gap_b, from 0.5 s, unmixed");
}

#[test]
fn removing_the_outgoing_track_while_the_mix_is_audible_keeps_the_incoming_track() {
    // As above: gap_b is audible (gap_a's marker is gone) while gap_a still feeds the mix.
    let (p, h, rx) = setup_small_ring();
    p.send(Command::SetCrossfade(1.0));
    play_entries(&p, vec![album_entry("gap_a.flac", None, None), album_entry("gap_b.flac", None, None), album_entry("short_a.flac", None, None)]);
    pull_into_overlap(&p, &h);
    let gap_a = p.state().queue_item_id.unwrap() - 1;
    p.send(Command::Remove(vec![gap_a]));
    wait_queue_len(&rx, 2);
    wait_for(&p, |s| s.queue_index == Some(0));
    assert_eq!(p.state().position_ms, 50, "resynced at the audible position");
    let from = 2_205;
    h.clear_recorded();
    run_to_end(&h, &rx);
    let gap_b = decoded("gap_b.flac");
    assert_crossfade(&f32s(&h.recorded()), &gap_b[from * 2..], &decoded("short_a.flac"), SHORT / 2);
}

#[test]
fn spectrum_frames_flow_only_while_viz_is_on() {
    use engine_core::dsp::spectrum::Analyzer;
    let (p, h, rx) = setup(OutputMode::Processed);
    p.send(Command::SetViz(true));
    queue(&p, &["flac16_44k.flac"]); // L 440 Hz, R 1 kHz, amplitude 1/8
    let (stop, join) = live_puller(&h);
    let start = Instant::now();
    let frame = loop {
        assert!(start.elapsed() < Duration::from_secs(5), "no spectrum frame");
        if let Ok(PlayerEvent::Spectrum(b)) = rx.recv_timeout(Duration::from_millis(50)) { break b; }
    };
    let a = Analyzer::new(44_100);
    assert!(frame[a.band_of(440.0)] > 150 && frame[a.band_of(1000.0)] > 150, "{frame:?}");
    p.send(Command::SetViz(false));
    std::thread::sleep(Duration::from_millis(150));
    let _ = rx.try_iter().count();
    std::thread::sleep(Duration::from_millis(300));
    assert!(!rx.try_iter().any(|e| matches!(e, PlayerEvent::Spectrum(_))), "off means no frames");
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    join.join().unwrap();
}
