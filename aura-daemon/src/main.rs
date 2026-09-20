use std::path::PathBuf;

use aura_common::DEFAULT_HEARTBEAT_MS;
use aura_daemon::daemon::DaemonConfig;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version = env!("GIT_VERSION"), about = "AURA daemon telemetry producer")]
struct Args {
    #[arg(short, long)]
    shm_path: Option<PathBuf>,
    #[arg(short = 'i', long, default_value_t = DEFAULT_HEARTBEAT_MS)]
    heartbeat_ms: u64,
    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    verbose: bool,
    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    foreground: bool,
}

fn main() {
    let args = Args::parse();
    let config = DaemonConfig {
        shm_path: args.shm_path,
        heartbeat_ms: args.heartbeat_ms,
        verbose: args.verbose,
        foreground: args.foreground,
    };
    if let Err(error) = aura_daemon::run(config) {
        log::error!("daemon failed: {error}");
        std::process::exit(1);
    }
}
