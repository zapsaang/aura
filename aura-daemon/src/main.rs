use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::Parser;
use env_logger::Builder;
use log::{error, info, LevelFilter};

use aura_common::{AuraResult, DEFAULT_HEARTBEAT_MS, SHM_PATH};
use aura_daemon::{collectors, heartbeat, state};

static SHUTDOWN_FLAG: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "linux")]
unsafe fn setup_signal_handlers() {
    extern "C" fn handler(_sig: libc::c_int) {
        SHUTDOWN_FLAG.store(true, Ordering::Relaxed);
    }
    libc::signal(libc::SIGINT, handler as *const () as usize);
    libc::signal(libc::SIGTERM, handler as *const () as usize);
}

#[cfg(not(target_os = "linux"))]
fn setup_signal_handlers() {}

#[derive(Parser, Debug)]
#[command(author, version = env!("GIT_VERSION"), about = "AURA daemon telemetry producer")]
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

    let shm = state::ShmHandle::new(Path::new(&args.shm_path))?;
    let mut collector_state = collectors::CollectorState::new();
    collectors::init(&mut collector_state)?;

    heartbeat::run(
        shm,
        collector_state,
        Duration::from_millis(args.heartbeat_ms),
        &SHUTDOWN_FLAG,
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

    unsafe {
        setup_signal_handlers();
    }

    if let Err(e) = run(args) {
        error!("daemon failed: {e}");
        std::process::exit(1);
    }
}
