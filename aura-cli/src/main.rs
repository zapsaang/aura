use aura_cli::args::Args;
use aura_common::AuraError;
use clap::Parser;

fn main() {
    let args = Args::parse();

    match aura_cli::run(args) {
        Ok(output) => println!("{output}"),
        Err(AuraError::StaleData { .. }) => {
            println!("[AURA: OFFLINE]");
            std::process::exit(1);
        }
        Err(AuraError::Offline(_)) => {
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
