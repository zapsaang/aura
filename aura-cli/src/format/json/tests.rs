use aura_common::TelemetryArchive;

use super::render;
use crate::Module;

#[test]
fn all_module_serializes_every_current_dimension() {
    let mut telemetry = TelemetryArchive::zeroed();
    telemetry.version = 7;
    telemetry.cpu.core_count = 1;

    let rendered = render(Module::All, &telemetry).expect("serialize telemetry");
    let value: serde_json::Value =
        serde_json::from_str(&rendered).expect("parse serialized telemetry");

    assert_eq!(value["version"], 7);
    assert_eq!(value["cpu"]["cores"].as_array().map(Vec::len), Some(1));
    for dimension in ["process", "memory", "network", "meta", "gpu"] {
        assert!(value.get(dimension).is_some(), "missing {dimension}");
    }
}

#[test]
fn cpu_module_serializes_only_version_and_cpu() {
    let telemetry = TelemetryArchive::zeroed();

    let rendered = render(Module::Cpu, &telemetry).expect("serialize telemetry");
    let value: serde_json::Value =
        serde_json::from_str(&rendered).expect("parse serialized telemetry");
    let object = value.as_object().expect("JSON root object");

    assert_eq!(object.len(), 2);
    assert!(object.contains_key("version"));
    assert!(object.contains_key("cpu"));
}
