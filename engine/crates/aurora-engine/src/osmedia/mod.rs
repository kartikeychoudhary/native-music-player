//! OS media integration (spec §3.10) through `souvlaki`: Windows SMTC, Linux MPRIS, macOS
//! MPNowPlayingInfoCenter. Pure mapping functions plus one thread that owns the controls.
pub mod app_id;
#[cfg(windows)]
mod win_window;

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError};
use engine_core::player::{Command, Commander, StateReader};
use protocol::{PlaybackStatus, PlayerState};
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection};

pub const SEEK_STEP_MS: i64 = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Action { Play, Pause, Toggle, Next, Prev, Stop, SeekTo(u64), SeekBy(i64) }

pub fn map_event(e: &MediaControlEvent) -> Option<Action> {
    let signed = |dir: &SeekDirection, ms: i64| if matches!(dir, SeekDirection::Forward) { ms } else { -ms };
    Some(match e {
        MediaControlEvent::Play => Action::Play,
        MediaControlEvent::Pause => Action::Pause,
        MediaControlEvent::Toggle => Action::Toggle,
        MediaControlEvent::Next => Action::Next,
        MediaControlEvent::Previous => Action::Prev,
        MediaControlEvent::Stop => Action::Stop,
        MediaControlEvent::Seek(dir) => Action::SeekBy(signed(dir, SEEK_STEP_MS)),
        MediaControlEvent::SeekBy(dir, d) => Action::SeekBy(signed(dir, i64::try_from(d.as_millis()).unwrap_or(i64::MAX))),
        MediaControlEvent::SetPosition(MediaPosition(d)) => Action::SeekTo(u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        _ => return None,
    })
}

pub fn to_command(a: &Action, state: &PlayerState) -> Command {
    match a {
        Action::Play => Command::Play(None),
        Action::Pause => Command::Pause,
        Action::Toggle => Command::Toggle,
        Action::Next => Command::Next,
        Action::Prev => Command::Prev,
        Action::Stop => Command::Stop,
        Action::SeekTo(ms) => Command::Seek(*ms),
        Action::SeekBy(d) => {
            let pos = i64::try_from(state.position_ms).unwrap_or(i64::MAX);
            let target = pos.saturating_add(*d).max(0) as u64;
            Command::Seek(state.duration_ms.map_or(target, |dur| target.min(dur)))
        }
    }
}

/// `file://` URL for a local file, percent-encoding everything but unreserved characters and `/`.
/// (Windows hands souvlaki a raw path instead, see `cover_url`, so it only uses this in tests.)
#[cfg_attr(windows, allow(dead_code))]
pub fn file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let enc: String = s.bytes().map(|b| {
        if b.is_ascii_alphanumeric() || b"/-._~:".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }
    }).collect();
    if enc.starts_with('/') { format!("file://{enc}") } else { format!("file:///{enc}") }
}

/// The cover URL handed to souvlaki. Its Windows backend strips a leading `file://` and opens the
/// remainder as a plain path (no decoding, no leading-slash fix-up, backslashes only), so there it
/// needs `file://C:\dir\x.webp` rather than the percent-encoded form MPRIS and macOS expect.
fn cover_url(p: &Path) -> String {
    #[cfg(windows)]
    { format!("file://{}", p.to_string_lossy().replace('/', "\\")) }
    #[cfg(not(windows))]
    { file_url(p) }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NowPlaying { pub title: String, pub artist: Option<String>, pub album: Option<String>, pub cover: Option<PathBuf>, pub duration_ms: Option<u64> }

pub fn now_playing(state: &PlayerState, cover: impl Fn(i64) -> Option<PathBuf>) -> Option<NowPlaying> {
    let t = state.track.as_ref()?;
    Some(NowPlaying {
        title: t.title.clone(), artist: t.artist.clone(), album: t.album.clone(),
        cover: t.artwork_id.and_then(cover), duration_ms: state.duration_ms.or(t.duration_ms),
    })
}

pub fn playback(state: &PlayerState) -> MediaPlayback {
    let progress = Some(MediaPosition(Duration::from_millis(state.position_ms)));
    match (state.status, state.track.is_some()) {
        (PlaybackStatus::Playing, true) => MediaPlayback::Playing { progress },
        (PlaybackStatus::Paused, true) => MediaPlayback::Paused { progress },
        _ => MediaPlayback::Stopped,
    }
}

pub fn enabled() -> bool { std::env::var("AURORA_OSMEDIA").as_deref() != Ok("0") }

fn metadata<'a>(n: &'a NowPlaying, cover_url: Option<&'a str>) -> MediaMetadata<'a> {
    MediaMetadata {
        title: Some(&n.title), artist: n.artist.as_deref(), album: n.album.as_deref(),
        cover_url, duration: n.duration_ms.map(Duration::from_millis),
    }
}

/// Pushes a track to the OS. A cover the OS can't load must not cost the title, so retry without it.
fn show(controls: &mut MediaControls, n: &NowPlaying) {
    let url = n.cover.as_deref().map(cover_url);
    if let Err(e) = controls.set_metadata(metadata(n, url.as_deref())) {
        tracing::warn!(error = ?e, cover = ?url, "OS now-playing update failed");
        if url.is_some() && let Err(e) = controls.set_metadata(metadata(n, None)) {
            tracing::warn!(error = ?e, "OS now-playing update failed without cover");
        }
    }
}

/// Owns the OS media controls on a dedicated thread: OS events become player commands; the latest
/// state from `states` updates what the OS shows. Exits quietly when `states` closes.
/// Must stay panic-free: the engine's panic hook exits the whole process on a panic in any thread.
pub fn spawn(states: Receiver<PlayerState>, commander: Commander, reader: StateReader, cover: impl Fn(i64) -> Option<PathBuf> + Send + 'static) -> Option<JoinHandle<()>> {
    std::thread::Builder::new().name("aurora-osmedia".into()).spawn(move || {
        #[cfg(windows)]
        let window = match win_window::HiddenWindow::new() {
            Ok(w) => w,
            Err(e) => { tracing::warn!(error = %e, "media keys unavailable: no window for SMTC"); return; }
        };
        #[cfg(windows)]
        let hwnd = Some(window.hwnd());
        #[cfg(not(windows))]
        let hwnd = None;
        let mut controls = match MediaControls::new(PlatformConfig { display_name: "Aurora", dbus_name: "aurora", hwnd }) {
            Ok(c) => c,
            Err(e) => { tracing::warn!(error = ?e, "media keys unavailable"); return; }
        };
        let (cmd, rd) = (commander.clone(), reader.clone());
        if let Err(e) = controls.attach(move |ev: MediaControlEvent| {
            if let Some(a) = map_event(&ev) { cmd.send(to_command(&a, &rd.get())); }
        }) {
            tracing::warn!(error = ?e, "media keys unavailable");
            return;
        }
        let mut shown: Option<NowPlaying> = None;
        let mut status: Option<(PlaybackStatus, bool)> = None;
        loop {
            match states.recv_timeout(Duration::from_millis(50)) {
                Ok(first) => {
                    let s = states.try_iter().last().unwrap_or(first);
                    let np = now_playing(&s, &cover);
                    let track_changed = np != shown;
                    if track_changed && let Some(n) = &np { show(&mut controls, n); }
                    shown = np;
                    let now = (s.status, s.track.is_some());
                    if track_changed || status != Some(now) {
                        let _ = controls.set_playback(playback(&s));
                        status = Some(now);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            #[cfg(windows)]
            win_window::pump();
        }
    }).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{PlaybackStatus, TrackSummary};
    use souvlaki::{MediaControlEvent as E, MediaPosition, SeekDirection};

    fn playing(pos: u64, dur: Option<u64>) -> PlayerState {
        PlayerState {
            status: PlaybackStatus::Playing, position_ms: pos, duration_ms: dur,
            track: Some(TrackSummary { path: "/m/a.flac".into(), title: "Song".into(), duration_ms: dur, track_id: Some(1), artist: Some("Alpha".into()), album: Some("Light".into()), artwork_id: Some(7) }),
            ..Default::default()
        }
    }

    #[test]
    fn maps_os_events_to_actions() {
        assert_eq!(map_event(&E::Toggle), Some(Action::Toggle));
        assert_eq!(map_event(&E::Previous), Some(Action::Prev));
        assert_eq!(map_event(&E::Seek(SeekDirection::Backward)), Some(Action::SeekBy(-SEEK_STEP_MS)));
        assert_eq!(map_event(&E::SeekBy(SeekDirection::Forward, Duration::from_secs(3))), Some(Action::SeekBy(3_000)));
        assert_eq!(map_event(&E::SetPosition(MediaPosition(Duration::from_millis(1_500)))), Some(Action::SeekTo(1_500)));
        assert_eq!(map_event(&E::Raise), None);
        assert_eq!(map_event(&E::Quit), None);
    }

    #[test]
    fn relative_seeks_are_clamped() {
        let s = playing(5_000, Some(8_000));
        assert!(matches!(to_command(&Action::SeekBy(-10_000), &s), Command::Seek(0)));
        assert!(matches!(to_command(&Action::SeekBy(10_000), &s), Command::Seek(8_000)));
        assert!(matches!(to_command(&Action::SeekBy(1_000), &playing(5_000, None)), Command::Seek(6_000)));
        assert!(matches!(to_command(&Action::Toggle, &s), Command::Toggle));
    }

    #[test]
    fn extreme_seeks_do_not_panic_and_clamp() {
        let huge = Duration::from_secs(u64::MAX / 2);
        let fwd = map_event(&E::SeekBy(SeekDirection::Forward, huge)).unwrap();
        let back = map_event(&E::SeekBy(SeekDirection::Backward, huge)).unwrap();
        assert_eq!((fwd.clone(), back.clone()), (Action::SeekBy(i64::MAX), Action::SeekBy(-i64::MAX)));
        let s = playing(5_000, Some(8_000));
        assert!(matches!(to_command(&fwd, &s), Command::Seek(8_000)));
        assert!(matches!(to_command(&back, &s), Command::Seek(0)));
        let unknown = playing(5_000, None);
        assert!(matches!(to_command(&fwd, &unknown), Command::Seek(t) if t == i64::MAX as u64));
        assert!(matches!(to_command(&back, &unknown), Command::Seek(0)));
        assert!(matches!(to_command(&fwd, &playing(u64::MAX, None)), Command::Seek(t) if t == i64::MAX as u64));
        assert_eq!(map_event(&E::SetPosition(MediaPosition(Duration::MAX))), Some(Action::SeekTo(u64::MAX)));
    }

    #[test]
    fn now_playing_uses_track_metadata_and_cover_lookup() {
        let np = now_playing(&playing(0, Some(8_000)), |id| (id == 7).then(|| PathBuf::from("/art/x-300.webp"))).unwrap();
        assert_eq!((np.title.as_str(), np.artist.as_deref(), np.album.as_deref(), np.duration_ms), ("Song", Some("Alpha"), Some("Light"), Some(8_000)));
        assert_eq!(np.cover, Some(PathBuf::from("/art/x-300.webp")));
        assert!(now_playing(&PlayerState::default(), |_| None).is_none());
    }

    #[test]
    fn playback_status_mapping() {
        assert!(matches!(playback(&PlayerState::default()), MediaPlayback::Stopped));
        assert!(matches!(playback(&playing(1_000, None)), MediaPlayback::Playing { progress: Some(MediaPosition(d)) } if d == Duration::from_secs(1)));
    }

    #[test]
    fn cover_urls_are_file_urls() {
        #[cfg(windows)]
        assert_eq!(file_url(Path::new(r"C:\Art Cache\ab\x-300.webp")), "file:///C:/Art%20Cache/ab/x-300.webp");
        #[cfg(not(windows))]
        assert_eq!(file_url(Path::new("/art cache/ab/x-300.webp")), "file:///art%20cache/ab/x-300.webp");
    }
}
