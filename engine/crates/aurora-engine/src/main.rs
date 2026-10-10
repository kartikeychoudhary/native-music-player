use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
