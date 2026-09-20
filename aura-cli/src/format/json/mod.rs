mod convert;
mod convert_dims;
mod schema;

use aura_common::{AuraError, AuraResult, TelemetryArchive};

use crate::args::Module;
use convert_dims::{gpu_json, meta_json, network_json, storage_json};

pub use schema::TelemetryJson;

pub fn render(module: Module, telemetry: &TelemetryArchive) -> AuraResult<String> {
    let json = TelemetryJson::from_telemetry(module, telemetry);
    serde_json::to_string_pretty(&json)
        .map_err(|e| AuraError::ParseError(format!("failed to serialize JSON output: {e}")))
}

impl TelemetryJson {
    fn from_telemetry(module: Module, t: &TelemetryArchive) -> Self {
        let pick = |m: Module| matches!(module, Module::All) || module == m;
        Self {
            version: t.version,
            capabilities: convert::capabilities(t.capabilities),
            cpu: pick(Module::Cpu).then(|| convert::cpu_json(t)),
            process: pick(Module::Process).then(|| convert::process_json(t)),
            memory: (pick(Module::Mem) || module == Module::Swap).then(|| convert::memory_json(t)),
            storage: pick(Module::Disk).then(|| storage_json(t)),
            network: pick(Module::Net).then(|| network_json(t)),
            meta: pick(Module::Os).then(|| meta_json(t)),
            gpu: pick(Module::Gpu).then(|| gpu_json(t)),
        }
    }
}
