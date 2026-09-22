pub mod args;
pub mod format;
pub mod output;
pub mod reader;

use std::time::Duration;

use aura_common::{AuraError, AuraResult, OFFLINE_THRESHOLD_SECS};

use args::{Args, Module, OutputFormat};
use reader::TelemetryReader;

pub fn run(args: Args) -> AuraResult<String> {
    if args.format == OutputFormat::Raw && args.module == Module::All {
        return Err(AuraError::InvalidArgument(
            "--format raw requires a single module (got --module all)".to_string(),
        ));
    }

    let reader = match &args.shm_path {
        Some(path) => TelemetryReader::new(path)?,
        None => TelemetryReader::new_default()?,
    };
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
        OutputFormat::Raw => output::value::render_raw(args.module, &telemetry)?,
    };

    Ok(rendered)
}
