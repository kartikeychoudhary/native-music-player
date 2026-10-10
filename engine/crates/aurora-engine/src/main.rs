use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use engine_core::output::null::{NullBackend, NullMode};
use engine_core::output::{system::SystemBackend, OutputBackend};
use engine_core::player::{spawn_player, Command, PlayerConfig, PlayerEvent};

#[derive(Parser)]
#[command(name = "aurora-engine", version, about = "Aurora audio engine")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
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
    let cli = Cli::parse();
    let code = match cli.cmd {
        Some(Cmd::Probe { file }) => probe(&file),
        Some(Cmd::Devices { null }) => devices(null),
        Some(Cmd::Play { files, bit_perfect, device, null, speed }) => play(files, bit_perfect, device, null, speed),
        None => {
            eprintln!("rpc mode arrives in a later task");
            1
        }
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

fn play(files: Vec<PathBuf>, bit_perfect: bool, device: Option<String>, null: bool, speed: f64) -> i32 {
    let (tx, rx) = crossbeam_channel::unbounded();
    let player = spawn_player(backend(null, speed), PlayerConfig::default(), tx);
    if bit_perfect { player.send(Command::SetMode(protocol::OutputMode::BitPerfect)); }
    if device.is_some() { player.send(Command::SetDevice(device)); }
    player.send(Command::SetQueue { paths: files, start: 0 });
    player.send(Command::Play(None));
    for ev in rx.iter() {
        let line = match &ev {
            PlayerEvent::SignalPath(s) => serde_json::json!({ "event": "signalPath", "data": s }),
            PlayerEvent::Error(e) => serde_json::json!({ "event": "error", "data": e }),
            PlayerEvent::Ended => { println!("{}", serde_json::json!({ "event": "ended" })); break; }
            _ => continue,
        };
        println!("{line}");
    }
    player.shutdown();
    0
}
