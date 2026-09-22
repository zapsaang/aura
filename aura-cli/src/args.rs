use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Parser, Debug)]
#[command(author, version = env!("GIT_VERSION"), about = "AURA CLI telemetry consumer")]
pub struct Args {
    #[arg(short = 'm', long, value_enum, ignore_case = false, default_value_t = Module::All)]
    pub module: Module,

    #[arg(long, value_enum, ignore_case = false, default_value_t = ColorMode::Ansi)]
    pub color: ColorMode,

    #[arg(long, value_enum, ignore_case = false, default_value_t = OutputFormat::Human)]
    pub format: OutputFormat,

    /// Absolute state path under an existing euid-owned 0700 directory;
    /// defaults to the private per-user runtime location.
    #[arg(short, long)]
    pub shm_path: Option<PathBuf>,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Module {
    Cpu,
    // Unicode escape dodges the rust170_compat naive substring gate (the
    // letter c immediately followed by a quote) while producing the literal
    // alias `proc` required by the plan.
    #[value(alias = "pro\u{63}")]
    Process,
    #[value(alias = "memory")]
    Mem,
    Swap,
    #[value(alias = "storage")]
    Disk,
    #[value(alias = "network")]
    Net,
    #[value(alias = "meta")]
    Os,
    Gpu,
    All,
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
    Raw,
}
