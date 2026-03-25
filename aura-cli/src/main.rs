mod format;
mod output;
mod reader;

use std::path::PathBuf;
use std::time::Duration;

use aura_common::{AuraError, AuraResult, OFFLINE_THRESHOLD_SECS, SHM_PATH};
use clap::{Parser, ValueEnum};
use reader::TelemetryReader;

#[derive(Parser, Debug)]
#[command(author, version, about = "AURA CLI telemetry consumer")]
struct Args {
    #[arg(short = 'm', long, value_enum, default_value_t = Module::All)]
    module: Module,

    #[arg(long, value_enum, default_value_t = ColorMode::Ansi)]
    color: ColorMode,

    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,

    #[arg(short, long, default_value = SHM_PATH)]
    shm_path: PathBuf,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Module {
    Cpu,
    Mem,
    Swap,
    Disk,
    Net,
    All,
    Os,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    None,
    Ansi,
    Tmux,
    Zellij,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
    Value,
}

fn run(args: Args) -> AuraResult<String> {
    let reader = TelemetryReader::new(&args.shm_path)?;
    let telemetry = reader.read()?;

    let threshold = Duration::from_secs_f64(OFFLINE_THRESHOLD_SECS);
    if !reader.is_fresh(&telemetry, threshold) {
        return Err(AuraError::StaleData {
            age_ms: threshold.as_millis() as u64 + 1,
            threshold_ms: threshold.as_millis() as u64,
        });
    }

    let rendered = match args.format {
        OutputFormat::Human => output::render(args.module, args.color, &telemetry),
        OutputFormat::Json => format::json::render(args.module, &telemetry)?,
        OutputFormat::Value => output::value::render(args.module, &telemetry),
    };

    Ok(rendered)
}

fn main() {
    let args = Args::parse();

    match run(args) {
        Ok(output) => println!("{output}"),
        Err(AuraError::StaleData { .. }) => {
            println!("[AURA: OFFLINE]");
            std::process::exit(1);
        }
        Err(AuraError::MmapFailed(_)) => {
            println!("[AURA: OFFLINE]");
            std::process::exit(1);
        }
        Err(e) => {
            println!("[AURA: ERROR - {e}]");
            std::process::exit(1);
        }
    }
}
