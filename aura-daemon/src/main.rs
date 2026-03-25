use std::path::Path;
use std::time::Duration;

use clap::Parser;
use env_logger::Builder;
use log::{error, info, LevelFilter};

use aura_common::{AuraResult, DEFAULT_HEARTBEAT_MS, SHM_PATH};
use aura_daemon::{collectors, heartbeat, platform, state};

#[derive(Parser, Debug)]
#[command(author, version, about = "AURA daemon telemetry producer")]
struct Args {
    #[arg(short, long, default_value = SHM_PATH)]
    shm_path: String,

    #[arg(short = 'i', long, default_value_t = DEFAULT_HEARTBEAT_MS)]
    heartbeat_ms: u64,

    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    verbose: bool,

    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    foreground: bool,
}

fn run(args: Args) -> AuraResult<()> {
    info!("AURA daemon starting");
    info!("shared memory path: {}", args.shm_path);
    info!("heartbeat: {}ms", args.heartbeat_ms);

    let provider = platform::init()?;
    info!("platform provider: {}", provider.name());

    let shm = state::ShmHandle::new(Path::new(&args.shm_path))?;
    let mut collector_state = collectors::CollectorState::new();
    collectors::init(&mut collector_state)?;

    heartbeat::run(
        shm,
        collector_state,
        Duration::from_millis(args.heartbeat_ms),
        args.foreground,
    )
}

fn main() {
    let args = Args::parse();
    let level = if args.verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

    Builder::new()
        .filter_level(level)
        .format_timestamp_millis()
        .init();

    if let Err(e) = run(args) {
        error!("daemon failed: {e}");
        std::process::exit(1);
    }
}
