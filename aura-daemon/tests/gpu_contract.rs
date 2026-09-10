//! Contract tests for the GPU collector's probe-driven degradation semantics.
//!
//! A scripted `NvmlProbe` stands in for NVML so every degradation and
//! conversion case is deterministic and runnable without hardware.

#![cfg(all(feature = "gpu-nvml", target_os = "linux"))]

use aura_common::{GpuStats, MAX_GPUS};
use aura_common::{GPU_CAP_MEMORY_TOTAL, GPU_CAP_MEMORY_USED, GPU_CAP_NAME, GPU_CAP_POWER};
use aura_common::{GPU_CAP_TEMPERATURE, GPU_CAP_UTILIZATION};
use aura_daemon::collectors::gpu::{collect_gpu, init_gpu, DeviceReading, NvmlProbe, ProbeFailure};

#[derive(Clone, Copy, Default)]
struct DeviceScript {
    handle_fails: bool,
    name_fails: bool,
    memory_fails: bool,
    utilization_fails: bool,
    power_fails: bool,
    temperature_fails: bool,
}

struct MockProbe {
    count: Result<u32, ProbeFailure>,
    scripts: Vec<DeviceScript>,
    visited: Vec<u32>,
}

impl Default for MockProbe {
    fn default() -> Self {
        Self {
            count: Err(ProbeFailure),
            scripts: Vec::new(),
            visited: Vec::new(),
        }
    }
}

impl MockProbe {
    fn with_count(count: u32) -> Self {
        Self {
            count: Ok(count),
            scripts: vec![DeviceScript::default(); count.min(MAX_GPUS as u32) as usize],
            visited: Vec::new(),
        }
    }
}

impl NvmlProbe for MockProbe {
    fn device_count(&mut self) -> Result<u32, ProbeFailure> {
        self.count
    }

    fn read_device(&mut self, index: u32) -> Result<DeviceReading, ProbeFailure> {
        self.visited.push(index);
        let script = self
            .scripts
            .get(index as usize)
            .copied()
            .unwrap_or_default();
        if script.handle_fails {
            return Err(ProbeFailure);
        }
        let base = u64::from(index) + 1;
        Ok(DeviceReading {
            name: if script.name_fails {
                Err(ProbeFailure)
            } else {
                Ok(format!("Mock GPU {index}"))
            },
            memory_bytes: if script.memory_fails {
                Err(ProbeFailure)
            } else {
                Ok((base * 4096, base * 1024))
            },
            utilization_percent: if script.utilization_fails {
                Err(ProbeFailure)
            } else {
                Ok(37)
            },
            power_milliwatts: if script.power_fails {
                Err(ProbeFailure)
            } else {
                Ok(250_000)
            },
            temperature_celsius: if script.temperature_fails {
                Err(ProbeFailure)
            } else {
                Ok(65)
            },
        })
    }
}

fn fresh() -> GpuStats {
    let mut gpu = GpuStats {
        gpus: [aura_common::GpuStat {
            name: aura_common::FixedString16::new(),
            memory_total: 0,
            memory_used: 0,
            utilization_percent: 0.0,
            power_watts: 0.0,
            temperature_celsius: 0,
            available: 0,
            tone: 0,
            _pad0: [0; 4],
            capabilities: 0,
        }; MAX_GPUS],
        gpu_count: 0,
        nvml_available: 0,
        truncated: 0,
        _pad0: [0; 5],
    };
    init_gpu(&mut gpu, Err::<MockProbe, ProbeFailure>(ProbeFailure));
    gpu
}

fn assert_cleared(gpu: &GpuStats) {
    assert_eq!(gpu.gpu_count, 0);
    assert_eq!(gpu.nvml_available, 0);
    assert_eq!(gpu.truncated, 0);
    for record in &gpu.gpus {
        assert_eq!(record.available, 0);
        assert_eq!(record.capabilities, 0);
        assert_eq!(record.name.bytes, [0u8; 16]);
        assert_eq!(record.memory_total, 0);
        assert_eq!(record.memory_used, 0);
        assert_eq!(record.utilization_percent, 0.0);
        assert_eq!(record.power_watts, 0.0);
        assert_eq!(record.temperature_celsius, 0);
        assert_eq!(record.tone, 0);
        assert_eq!(record._pad0, [0u8; 4]);
    }
}

#[test]
fn init_failure_clears_enumeration() {
    let mut gpu = fresh();
    gpu.nvml_available = 1;
    gpu.gpu_count = 3;
    let stored = init_gpu(&mut gpu, Err::<MockProbe, ProbeFailure>(ProbeFailure));
    assert!(stored.is_none());
    assert_cleared(&gpu);
}

#[test]
fn post_init_count_failure_clears_without_probe() {
    let mut gpu = fresh();
    let probe = MockProbe {
        count: Err(ProbeFailure),
        ..MockProbe::default()
    };
    let stored = init_gpu(&mut gpu, Ok(probe));
    assert!(stored.is_none());
    assert_cleared(&gpu);
}

#[test]
fn successful_init_sets_available() {
    let mut gpu = fresh();
    let stored = init_gpu(&mut gpu, Ok(MockProbe::with_count(0)));
    assert!(stored.is_some());
    assert_eq!(gpu.nvml_available, 1);
    assert_eq!(gpu.gpu_count, 0);
    assert_eq!(gpu.truncated, 0);
}

#[test]
fn zero_devices_collects_empty() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(0);
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.nvml_available, 1);
    assert_eq!(gpu.gpu_count, 0);
    assert_eq!(gpu.truncated, 0);
    assert!(probe.visited.is_empty());
    for record in &gpu.gpus {
        assert_eq!(record.available, 0);
        assert_eq!(record.capabilities, 0);
    }
}

#[test]
fn eight_devices_fit_without_truncation() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(8);
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpu_count, 8);
    assert_eq!(gpu.truncated, 0);
    assert_eq!(probe.visited, (0..8).collect::<Vec<u32>>());
    for (index, record) in gpu.gpus.iter().enumerate() {
        let slot = index as u64 + 1;
        assert_eq!(record.available, 1);
        assert_eq!(record.memory_total, slot * 4096);
        assert_eq!(record.name.as_str(), format!("Mock GPU {index}"));
    }
}

#[test]
fn more_than_eight_devices_truncates() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(9);
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpu_count, 8);
    assert_eq!(gpu.truncated, 1);
    assert_eq!(probe.visited, (0..8).collect::<Vec<u32>>());
}

#[test]
fn count_failure_during_collect_clears_gpu() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(2);
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpu_count, 2);
    probe.count = Err(ProbeFailure);
    collect_gpu(&mut gpu, &mut probe);
    assert_cleared(&gpu);
}

#[test]
fn handle_hole_skips_truncates_and_compacts() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(4);
    probe.scripts[2].handle_fails = true;
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpu_count, 3);
    assert_eq!(gpu.truncated, 1);
    assert_eq!(probe.visited, vec![0, 1, 2, 3]);
    assert_eq!(gpu.gpus[0].name.as_str(), "Mock GPU 0");
    assert_eq!(gpu.gpus[1].name.as_str(), "Mock GPU 1");
    assert_eq!(gpu.gpus[2].name.as_str(), "Mock GPU 3");
    assert_eq!(gpu.gpus[2].memory_total, 4 * 4096);
    assert_eq!(gpu.gpus[3].available, 0);
}

#[test]
fn trailing_handle_failure_still_truncates() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(3);
    probe.scripts[2].handle_fails = true;
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpu_count, 2);
    assert_eq!(gpu.truncated, 1);
}

#[test]
fn all_metric_failures_keep_available_record() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(1);
    probe.scripts[0] = DeviceScript {
        handle_fails: false,
        name_fails: true,
        memory_fails: true,
        utilization_fails: true,
        power_fails: true,
        temperature_fails: true,
    };
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpu_count, 1);
    assert_eq!(gpu.truncated, 0);
    let record = &gpu.gpus[0];
    assert_eq!(record.available, 1);
    assert_eq!(record.capabilities, 0);
    assert_eq!(record.name.bytes, [0u8; 16]);
    assert_eq!(record.memory_total, 0);
    assert_eq!(record.temperature_celsius, 0);
}

#[test]
fn name_failure_clears_only_name() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(1);
    probe.scripts[0].name_fails = true;
    collect_gpu(&mut gpu, &mut probe);
    let record = &gpu.gpus[0];
    assert_eq!(record.capabilities & GPU_CAP_NAME, 0);
    assert_eq!(record.name.bytes, [0u8; 16]);
    assert_ne!(record.capabilities & GPU_CAP_MEMORY_TOTAL, 0);
    assert_eq!(gpu.truncated, 0);
}

#[test]
fn memory_failure_clears_both_memory_bits() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(1);
    probe.scripts[0].memory_fails = true;
    collect_gpu(&mut gpu, &mut probe);
    let record = &gpu.gpus[0];
    assert_eq!(
        record.capabilities & (GPU_CAP_MEMORY_TOTAL | GPU_CAP_MEMORY_USED),
        0
    );
    assert_eq!(record.memory_total, 0);
    assert_eq!(record.memory_used, 0);
    assert_ne!(record.capabilities & GPU_CAP_UTILIZATION, 0);
}

#[test]
fn metric_failures_never_set_truncation() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(2);
    probe.scripts[0].power_fails = true;
    probe.scripts[1].temperature_fails = true;
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpu_count, 2);
    assert_eq!(gpu.truncated, 0);
}

#[test]
fn conversion_formulas_are_exact() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(1);
    collect_gpu(&mut gpu, &mut probe);
    let record = &gpu.gpus[0];
    assert_eq!(record.memory_total, 4096);
    assert_eq!(record.memory_used, 1024);
    assert_eq!(record.utilization_percent, 37.0);
    assert_eq!(record.power_watts, 250.0);
    assert_eq!(record.temperature_celsius, 65);
    let all = GPU_CAP_NAME
        | GPU_CAP_MEMORY_TOTAL
        | GPU_CAP_MEMORY_USED
        | GPU_CAP_UTILIZATION
        | GPU_CAP_POWER
        | GPU_CAP_TEMPERATURE;
    assert_eq!(record.capabilities, all);
}

#[test]
fn power_conversion_divides_milliwatts() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(1);
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpus[0].power_watts, 250_000f32 / 1000.0);
}

#[test]
fn later_cycle_shrinks_and_zeroes_stale_records() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(4);
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpu_count, 4);
    probe.count = Ok(1);
    probe.scripts.truncate(1);
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpu_count, 1);
    assert_eq!(gpu.truncated, 0);
    for record in &gpu.gpus[1..] {
        assert_eq!(record.available, 0);
        assert_eq!(record.capabilities, 0);
        assert_eq!(record.memory_total, 0);
    }
}

#[test]
fn unvisited_indices_beyond_eight_are_never_read() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(64);
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpu_count, 8);
    assert_eq!(gpu.truncated, 1);
    assert!(probe.visited.iter().all(|&index| index < 8));
    assert_eq!(probe.visited.len(), 8);
}

#[test]
fn mixed_partial_failures_across_devices() {
    let mut gpu = fresh();
    let mut probe = MockProbe::with_count(3);
    probe.scripts[0].utilization_fails = true;
    probe.scripts[1].handle_fails = true;
    probe.scripts[2].name_fails = true;
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpu_count, 2);
    assert_eq!(gpu.truncated, 1);
    let first = &gpu.gpus[0];
    assert_eq!(first.capabilities & GPU_CAP_UTILIZATION, 0);
    assert_eq!(first.utilization_percent, 0.0);
    assert_ne!(first.capabilities & GPU_CAP_POWER, 0);
    let second = &gpu.gpus[1];
    assert_eq!(second.name.as_str(), "");
    assert_eq!(second.capabilities & GPU_CAP_NAME, 0);
    assert_eq!(second.memory_total, 3 * 4096);
}

#[test]
fn reinit_after_success_reclears_state() {
    let mut gpu = fresh();
    let probe = MockProbe::with_count(2);
    let stored = init_gpu(&mut gpu, Ok(probe));
    assert!(stored.is_some());
    let mut probe = stored.unwrap();
    collect_gpu(&mut gpu, &mut probe);
    assert_eq!(gpu.gpu_count, 2);
    let stored = init_gpu(&mut gpu, Err::<MockProbe, ProbeFailure>(ProbeFailure));
    assert!(stored.is_none());
    assert_cleared(&gpu);
}
