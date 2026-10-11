mod common;
use common::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use engine_core::output::null::{NullBackend, NullHandle, NullMode};
use engine_core::player::{spawn_player, Command, NewEntry, PlayerConfig, PlayerEvent, PlayerHandle, Restore};
use protocol::*;

const GAP_A: usize = 110_583;
const GAP_B: usize = 6 * 44_100 - GAP_A;
const SHORT: usize = 22_050;

fn setup() -> (PlayerHandle, NullHandle, Receiver<PlayerEvent>) {
    let (backend, h) = NullBackend::new(NullMode::Manual);
    let (tx, rx) = crossbeam_channel::unbounded();
    (spawn_player(Arc::new(backend), PlayerConfig::default(), tx), h, rx)
}

fn items(names: &[&str]) -> Vec<NewEntry> { names.iter().map(|n| NewEntry::from_path(fixture(n))).collect() }

fn play(p: &PlayerHandle, names: &[&str]) {
    p.send(Command::SetQueue { items: items(names), start: 0 });
    p.send(Command::Play(None));
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

/// Pulls until Ended (or `max_frames`), returning the events seen.
fn run(h: &NullHandle, rx: &Receiver<PlayerEvent>, max_frames: usize) -> Vec<PlayerEvent> {
    let start = Instant::now();
    let (mut events, mut pulled) = (vec![], 0);
    loop {
        assert!(start.elapsed() < Duration::from_secs(60), "timed out; events: {events:?}");
        let n = h.pull(4096.min(max_frames - pulled));
        pulled += n;
        events.extend(rx.try_iter());
        if pulled >= max_frames || events.iter().any(|e| matches!(e, PlayerEvent::Ended)) { return events; }
        if n == 0 { std::thread::sleep(Duration::from_millis(1)); }
    }
}

fn track_names(events: &[PlayerEvent]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for e in events {
        if let PlayerEvent::State(s) = e && let Some(t) = &s.track {
            let name = std::path::Path::new(&t.path).file_stem().unwrap().to_string_lossy().into_owned();
            if out.last() != Some(&name) && s.status == PlaybackStatus::Playing { out.push(name); }
        }
    }
    out
}

/// Tracks that started playing after `current` (periodic state ticks may repeat `current` first).
fn upcoming(events: &[PlayerEvent], current: &str) -> Vec<String> {
    track_names(events).into_iter().skip_while(|n| n == current).collect()
}

fn last_queue(rx: &Receiver<PlayerEvent>) -> QueueChanged {
    let start = Instant::now();
    let mut last = None;
    while start.elapsed() < Duration::from_millis(500) {
        for e in rx.try_iter() { if let PlayerEvent::Queue(q) = e { last = Some(q); } }
        std::thread::sleep(Duration::from_millis(5));
    }
    last.expect("a queue.changed event")
}

/// Waits until the player has applied an edit (a queue.changed matching `done`), so pulls that
/// follow can't race the edit's resync. Returns every event seen, in order.
fn applied(rx: &Receiver<PlayerEvent>, done: impl Fn(&QueueChanged) -> bool) -> Vec<PlayerEvent> {
    let mut events = vec![];
    loop {
        let e = rx.recv_timeout(Duration::from_secs(10)).unwrap_or_else(|_| panic!("edit not applied; events: {events:?}"));
        let finished = matches!(&e, PlayerEvent::Queue(q) if done(q));
        events.push(e);
        if finished { return events; }
    }
}

/// Plays gap_a for 1 s and lets the ring fill past its end, so gap_b is already decoded (prefed).
fn into_prefeed(p: &PlayerHandle, h: &NullHandle, names: &[&str]) {
    play(p, names);
    pull_frames(h, 44_100);
    std::thread::sleep(Duration::from_millis(300));
}

#[test]
fn insert_next_during_prefeed_replaces_buffered_track() {
    let (p, h, rx) = setup();
    into_prefeed(&p, &h, &["gap_a.flac", "gap_b.flac"]);
    p.send(Command::InsertNext(items(&["short_a.flac"])));
    assert_eq!(last_queue(&rx).items.len(), 3);
    h.clear_recorded();
    let events = run(&h, &rx, usize::MAX);
    assert_eq!(h.recorded().len() / 8, (GAP_A - 44_100) + SHORT + GAP_B, "rest of gap_a, the inserted track, then gap_b; nothing stale");
    assert_eq!(upcoming(&events, "gap_a"), ["short_a", "gap_b"]);
}

#[test]
fn removing_prefed_next_resyncs() {
    let (p, h, rx) = setup();
    into_prefeed(&p, &h, &["gap_a.flac", "gap_b.flac", "short_b.flac"]);
    let gap_b_id = p.state().queue_item_id.unwrap() + 1;
    p.send(Command::Remove(vec![gap_b_id]));
    assert_eq!(last_queue(&rx).items.len(), 2);
    h.clear_recorded();
    let events = run(&h, &rx, usize::MAX);
    assert_eq!(h.recorded().len() / 8, (GAP_A - 44_100) + SHORT);
    assert_eq!(upcoming(&events, "gap_a"), ["short_b"]);
}

#[test]
fn moving_upcoming_tracks_drops_the_preopened_decoder() {
    let (p, h, rx) = setup();
    play(&p, &["gap_a.flac", "short_a.flac", "short_b.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    p.send(Command::Move { from: 2, to: 1 });
    let mut events = applied(&rx, |q| q.items[1].path.ends_with("short_b.flac"));
    events.extend(run(&h, &rx, usize::MAX));
    assert_eq!(track_names(&events), ["gap_a", "short_b", "short_a"]);
    assert_eq!(h.recorded().len() / 8, GAP_A + 2 * SHORT);
}

#[test]
fn removing_the_playing_track_starts_the_next() {
    let (p, h, rx) = setup();
    play(&p, &["gap_a.flac", "short_a.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    let id = p.state().queue_item_id.unwrap();
    p.send(Command::Remove(vec![id]));
    wait_for(&p, |s| s.track.as_ref().is_some_and(|t| t.path.ends_with("short_a.flac")) && s.status == PlaybackStatus::Playing);
    h.clear_recorded();
    run(&h, &rx, usize::MAX);
    assert_eq!(h.recorded().len() / 8, SHORT);
}

#[test]
fn repeat_one_replays_and_next_still_moves_on() {
    let (p, h, rx) = setup();
    p.send(Command::SetRepeat(RepeatMode::One));
    play(&p, &["short_a.flac", "short_b.flac"]);
    let events = run(&h, &rx, 3 * SHORT + 1000);
    assert!(events.iter().all(|e| !matches!(e, PlayerEvent::State(s) if s.queue_index == Some(1))), "stayed on the first track");
    assert_eq!(p.state().repeat, RepeatMode::One);
    p.send(Command::Next);
    wait_for(&p, |s| s.queue_index == Some(1));
}

#[test]
fn repeat_all_wraps_without_ending() {
    let (p, h, rx) = setup();
    p.send(Command::SetRepeat(RepeatMode::All));
    play(&p, &["short_a.flac", "short_b.flac"]);
    // Enough audio for several ring refills: the player observes the output between refills, so
    // the wraps are reported before `run` returns.
    let events = run(&h, &rx, 20 * SHORT);
    assert!(!events.iter().any(|e| matches!(e, PlayerEvent::Ended)));
    let mut seen = vec![];
    for e in &events { if let PlayerEvent::State(s) = e && s.status == PlaybackStatus::Playing && seen.last() != Some(&s.queue_index) { seen.push(s.queue_index); } }
    assert!(seen.windows(4).any(|w| w == [Some(0), Some(1), Some(0), Some(1)]), "{seen:?}");
}

#[test]
fn shuffle_keeps_the_playing_entry_first_and_restores_order() {
    let (p, _h, rx) = setup();
    play(&p, &["short_a.flac", "short_b.flac", "mono16_44k.flac", "flac16_44k.flac", "gap_a.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    let before = last_queue(&rx);
    let playing = p.state().queue_item_id.unwrap();
    p.send(Command::SetShuffle(true));
    let shuffled = last_queue(&rx);
    assert_eq!(shuffled.items[0].id, playing);
    assert_eq!(shuffled.current, Some(0));
    let mut a: Vec<u32> = before.items.iter().map(|i| i.id).collect();
    let mut b: Vec<u32> = shuffled.items.iter().map(|i| i.id).collect();
    a.sort();
    b.sort();
    assert_eq!(a, b);
    assert!(p.state().shuffle);
    p.send(Command::SetShuffle(false));
    assert_eq!(last_queue(&rx).items.iter().map(|i| i.id).collect::<Vec<_>>(), before.items.iter().map(|i| i.id).collect::<Vec<_>>());
}

#[test]
fn played_fires_once_after_half_the_track() {
    let (p, h, rx) = setup();
    play(&p, &["gap_a.flac"]); // 2.51 s → threshold 1.25 s
    pull_frames(&h, 44_100);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!rx.try_iter().any(|e| matches!(e, PlayerEvent::Played { .. })));
    let events = run(&h, &rx, usize::MAX);
    let played: Vec<_> = events.iter().filter(|e| matches!(e, PlayerEvent::Played { .. })).collect();
    assert_eq!(played.len(), 1);
    assert!(matches!(played[0], PlayerEvent::Played { path, .. } if path.ends_with("gap_a.flac")));
}

#[test]
fn seeking_forward_is_not_listening() {
    let (p, h, rx) = setup();
    play(&p, &["gap_a.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    p.send(Command::Seek(2_000));
    wait_for(&p, |s| s.position_ms >= 2_000);
    let events = run(&h, &rx, usize::MAX);
    assert!(!events.iter().any(|e| matches!(e, PlayerEvent::Played { .. })), "only ~0.5 s was heard");
}

#[test]
fn restore_starts_paused_at_the_saved_position() {
    let (p, h, _rx) = setup();
    p.send(Command::Restore(Box::new(Restore {
        items: vec![NewEntry { duration_ms: Some(2_507), ..NewEntry::from_path(fixture("gap_a.flac")) }, NewEntry::from_path(fixture("gap_b.flac"))],
        current: 1, position_ms: 1_000, shuffle: false, repeat: RepeatMode::All, volume: 0.5, mode: OutputMode::Processed,
    })));
    wait_for(&p, |s| s.status == PlaybackStatus::Paused);
    let s = p.state();
    assert_eq!((s.queue_index, s.position_ms, s.repeat, s.volume), (Some(1), 1_000, RepeatMode::All, 0.5));
    assert!(h.opened().is_empty(), "no stream until Play");
    p.send(Command::Seek(1_500));
    wait_for(&p, |s| s.position_ms == 1_500);
    p.send(Command::Play(None));
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    pull_frames(&h, 44_100);
    wait_for(&p, |s| s.position_ms >= 2_400 && s.track.as_ref().is_some_and(|t| t.path.ends_with("gap_b.flac")));
}

#[test]
fn adding_to_a_draining_queue_keeps_playing() {
    let (p, h, rx) = setup();
    play(&p, &["short_a.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    std::thread::sleep(Duration::from_millis(50)); // short_a is fully decoded; the player is draining to the end
    p.send(Command::Add(items(&["short_b.flac"])));
    let mut events = applied(&rx, |q| q.items.len() == 2);
    events.extend(run(&h, &rx, usize::MAX));
    assert_eq!(h.recorded().len() / 8, 2 * SHORT);
    assert_eq!(track_names(&events), ["short_a", "short_b"]);
}

#[test]
fn clear_stops_and_empties() {
    let (p, _h, rx) = setup();
    play(&p, &["gap_a.flac"]);
    wait_for(&p, |s| s.status == PlaybackStatus::Playing);
    p.send(Command::Clear);
    wait_for(&p, |s| s.status == PlaybackStatus::Stopped && s.track.is_none());
    assert!(last_queue(&rx).items.is_empty());
}

#[test]
fn played_threshold_rule() {
    use engine_core::player::played_threshold_ms;
    assert_eq!(played_threshold_ms(Some(200_000)), 100_000);
    assert_eq!(played_threshold_ms(Some(1_200_000)), 240_000);
    assert_eq!(played_threshold_ms(None), 240_000);
    assert_eq!(played_threshold_ms(Some(0)), 1);
}
