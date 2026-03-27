pub mod ansi;
pub mod cpu;
pub mod mem;
pub mod meta;
pub mod net;
pub mod value;

use aura_common::TelemetryArchive;

use crate::{ColorMode, Module};

type Renderer = fn(ColorMode, &TelemetryArchive) -> String;

const RENDERERS: [Renderer; 6] = [
    cpu::render,
    mem::render,
    mem::render_swap,
    net::render,
    render_all,
    meta::render,
];

pub fn render(module: Module, color: ColorMode, telemetry: &TelemetryArchive) -> String {
    let idx = module_index(module);
    RENDERERS[idx](color, telemetry)
}

const fn module_index(module: Module) -> usize {
    match module {
        Module::Cpu => 0,
        Module::Mem => 1,
        Module::Swap => 2,
        Module::Net => 3,
        Module::All => 4,
        Module::Os => 5,
    }
}

fn render_all(color: ColorMode, telemetry: &TelemetryArchive) -> String {
    [
        cpu::render(color, telemetry),
        mem::render(color, telemetry),
        net::render(color, telemetry),
        meta::render(color, telemetry),
    ]
    .join("\n")
}

pub(crate) fn trim_zero_terminated(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

#[cfg(test)]
mod tests {
    use aura_common::TelemetryArchive;

    use crate::{ColorMode, Module};

    use super::render;

    #[test]
    fn routing_renders_cpu_module() {
        let telemetry = unsafe { std::mem::zeroed::<TelemetryArchive>() };
        let out = render(Module::Cpu, ColorMode::None, &telemetry);
        assert!(out.contains("CPU"));
    }
}
