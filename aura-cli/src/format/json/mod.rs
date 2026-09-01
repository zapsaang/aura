mod convert;
mod schema;

use aura_common::{AuraError, AuraResult, TelemetryArchive};

pub use schema::TelemetryJson;

use crate::Module;

pub fn render(module: Module, telemetry: &TelemetryArchive) -> AuraResult<String> {
    let json = TelemetryJson::from_telemetry(module, telemetry);
    serde_json::to_string_pretty(&json)
        .map_err(|e| AuraError::ParseError(format!("failed to serialize JSON output: {e}")))
}

#[cfg(test)]
mod tests;
