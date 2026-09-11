pub mod color;
pub mod cpu;
pub mod gpu;
pub mod memory;
pub mod meta;
pub mod network;
pub mod process;
pub mod si;
pub mod storage;
pub mod value;

use aura_common::TelemetryArchive;

use crate::args::{ColorMode, Module};

pub fn render(module: Module, color: ColorMode, telemetry: &TelemetryArchive) -> String {
    match module {
        Module::Cpu => cpu::render(color, telemetry),
        Module::Process => process::render(color, telemetry),
        Module::Mem => memory::render(color, telemetry),
        Module::Swap => memory::render_swap(color, telemetry),
        Module::Disk => storage::render(color, telemetry),
        Module::Net => network::render(color, telemetry),
        Module::Os => meta::render(color, telemetry),
        Module::Gpu => gpu::render(color, telemetry),
        Module::All => render_all(color, telemetry),
    }
}

fn render_all(color: ColorMode, telemetry: &TelemetryArchive) -> String {
    [
        cpu::render(color, telemetry),
        process::render(color, telemetry),
        memory::render(color, telemetry),
        memory::render_swap(color, telemetry),
        storage::render(color, telemetry),
        network::render(color, telemetry),
        meta::render(color, telemetry),
        gpu::render(color, telemetry),
    ]
    .join("\n\n")
}
