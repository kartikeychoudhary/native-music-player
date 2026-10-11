mod osmedia;
mod rpc;

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use engine_core::output::null::{NullBackend, NullMode};
use engine_core::output::{system::SystemBackend, OutputBackend};
use engine_core::player::{spawn_player, Command, PlayerConfig, PlayerEvent};
use protocol::PlaybackStatus;

#[derive(Parser)]
#[command(name = "aurora-engine", version, about = "Aurora audio engine")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the JSON-RPC server on stdio (default).
    Rpc {
        /// Folder for the library database, artwork cache and session. Without it nothing is persisted.
        #[arg(long, value_name = "DIR")]
        data_dir: Option<PathBuf>,
    },
    /// Print what Aurora sees in a file.
    Probe { file: PathBuf },
    /// List output devices as JSON.
    Devices { #[arg(long)] null: bool },
    /// Play files from the command line, printing events as JSON lines.
    Play {
        files: Vec<PathBuf>,
        #[arg(long)] bit_perfect: bool,
        #[arg(long)] device: Option<String>,
        /// Use the null output instead of a real device.
        #[arg(long)] null: bool,
        /// Null-output speed multiplier.
        #[arg(long, default_value_t = 1.0)] speed: f64,
    },
    /// Panics on purpose; used only by tests of the panic hook.
    #[command(hide = true, name = "__panic")]
    Panic,
}

fn init_logging() {
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_env("AURORA_LOG").unwrap_or_else(|_| "info".into()))
        .init();
}

fn main() {
    init_logging();
    // A panic on any thread (including an output callback) is logged with a backtrace and the
    // process exits, so Electron's supervisor can restart the engine.
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        tracing::error!(panic = %info, backtrace = %bt, "engine panic");
        eprintln!("aurora-engine panic: {info}\n{bt}");
        std::process::exit(70);
    }));
    let cli = Cli::parse();
    let code = match cli.cmd {
        Some(Cmd::Probe { file }) => probe(&file),
        Some(Cmd::Devices { null }) => devices(null),
        Some(Cmd::Play { files, bit_perfect, device, null, speed }) => play(files, bit_perfect, device, null, speed),
        Some(Cmd::Panic) => panic!("test panic"),
        Some(Cmd::Rpc { data_dir }) => rpc::run(rpc::RunOptions { data_dir }),
        None => rpc::run(rpc::RunOptions::default()),
    };
    std::process::exit(code);
}

fn probe(file: &std::path::Path) -> i32 {
    match engine_core::decode::open(file) {
        Ok(d) => {
            let i = d.info();
            let v = serde_json::json!({
                "source": engine_core::decode::source_summary(i),
                "decoder": i.decoder_name,
                "totalFrames": i.total_frames,
            });
            println!("{v}");
            0
        }
        Err(e) => {
            eprintln!("{}: {e}", e.code().name());
            2
        }
    }
}

fn backend(null: bool, speed: f64) -> Arc<dyn OutputBackend> {
    if null { Arc::new(NullBackend::new(NullMode::Realtime { speed }).0) } else { Arc::new(SystemBackend::new()) }
}

fn devices(null: bool) -> i32 {
    match backend(null, 1.0).devices() {
        Ok(list) => { println!("{}", serde_json::to_string(&list).unwrap()); 0 }
        Err(e) => { eprintln!("{e}"); 1 }
    }
}

/// Decides when `play` is done. The player thread keeps its event sender alive, so the CLI
/// loop must end on events: `Ended`, or a `stopped` state after playback began or an error
/// occurred, or a `paused` state after an error (device loss pauses instead of stopping).
/// The initial stopped state published by SetQueue precedes all of these and must not end it.
#[derive(Default)]
struct PlayProgress { saw_playing: bool, saw_error: bool }

impl PlayProgress {
    /// Records `ev`; returns true when the loop should stop.
    fn observe(&mut self, ev: &PlayerEvent) -> bool {
        match ev {
            PlayerEvent::Ended => true,
            PlayerEvent::Error(_) => { self.saw_error = true; false }
            PlayerEvent::State(st) => match st.status {
                PlaybackStatus::Playing => { self.saw_playing = true; false }
                PlaybackStatus::Stopped => self.saw_playing || self.saw_error,
                PlaybackStatus::Paused => self.saw_error,
            },
            _ => false,
        }
    }
}

fn play(files: Vec<PathBuf>, bit_perfect: bool, device: Option<String>, null: bool, speed: f64) -> i32 {
    if files.is_empty() {
        eprintln!("play: no files given");
        return 2;
    }
    let (tx, rx) = crossbeam_channel::unbounded();
    let player = spawn_player(backend(null, speed), PlayerConfig::default(), tx);
    if bit_perfect { player.send(Command::SetMode(protocol::OutputMode::BitPerfect)); }
    if device.is_some() { player.send(Command::SetDevice(device)); }
    player.send(Command::SetQueue { items: files.into_iter().map(engine_core::player::NewEntry::from_path).collect(), start: 0 });
    player.send(Command::Play(None));
    let mut progress = PlayProgress::default();
    let mut finished = false;
    for ev in rx.iter() {
        let stop = progress.observe(&ev);
        match &ev {
            PlayerEvent::SignalPath(s) => println!("{}", serde_json::json!({ "event": "signalPath", "data": s })),
            PlayerEvent::Error(e) => println!("{}", serde_json::json!({ "event": "error", "data": e })),
            PlayerEvent::Ended => println!("{}", serde_json::json!({ "event": "ended" })),
            _ => {}
        }
        if stop { finished = true; break; }
    }
    player.shutdown();
    // Exit 1 on any error, or if the event stream died before a terminal state (player thread gone).
    i32::from(progress.saw_error || !finished)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::player::PlayerEvent;
    use protocol::{EngineErrorEvent, PlayerState};

    fn state(status: PlaybackStatus) -> PlayerEvent { PlayerEvent::State(PlayerState { status, ..Default::default() }) }
    fn error() -> PlayerEvent {
        PlayerEvent::Error(EngineErrorEvent { code: 1, name: "X".into(), message: "m".into(), path: None })
    }
    fn run(events: &[PlayerEvent]) -> Vec<bool> {
        let mut p = PlayProgress::default();
        events.iter().map(|e| p.observe(e)).collect()
    }

    #[test]
    fn initial_stopped_does_not_stop() {
        assert_eq!(run(&[state(PlaybackStatus::Stopped)]), [false]);
    }
    #[test]
    fn playing_then_stopped_stops() {
        assert_eq!(run(&[state(PlaybackStatus::Stopped), state(PlaybackStatus::Playing), state(PlaybackStatus::Stopped)]), [false, false, true]);
    }
    #[test]
    fn error_then_paused_stops() {
        assert_eq!(run(&[state(PlaybackStatus::Playing), error(), state(PlaybackStatus::Paused)]), [false, false, true]);
    }
    #[test]
    fn error_then_stopped_stops() {
        assert_eq!(run(&[error(), state(PlaybackStatus::Stopped)]), [false, true]);
    }
    #[test]
    fn ended_stops() {
        assert_eq!(run(&[PlayerEvent::Ended]), [true]);
    }
    #[test]
    fn paused_without_error_does_not_stop() {
        assert_eq!(run(&[state(PlaybackStatus::Playing), state(PlaybackStatus::Paused)]), [false, false]);
    }
}
