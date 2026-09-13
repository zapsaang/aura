use std::mem::{align_of, size_of};
use std::ptr::addr_of;

use aura_common::{
    validate_archive, AuraError, CpuCoreStat, CpuGlobalStat, DerivedStats, DiskStat, FixedString16,
    GpuStat, GpuStats, MemoryStats, MetaStats, MountStat, NetIfStat, NetworkStats, OsFingerprint,
    ProcessStat, ProcessStats, StorageStats, TelemetryArchive, ARCHIVE_VERSION, CAPABILITY_COUNT,
    CAP_CPU_GLOBAL, CAP_CPU_PER_CORE, CAP_GPU_ENUMERATION, CAP_MEMORY_RAM_TOTAL,
    CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP, CAP_META_OS_IDENTITY, CAP_NETWORK_BYTES,
    CAP_NETWORK_RATES, CAP_PROCESS_TOP_CPU, KNOWN_CAPABILITIES_MASK, MAX_CORES, MAX_DISKS,
    MAX_GPUS, MAX_MOUNTS, MAX_NETIFS, MAX_TOP_N, PROCESS_TRUNCATED, TONE_GREEN, TONE_MAGENTA,
    TONE_RED, TONE_YELLOW,
};

fn offset_of<T, F>(base: &T, field: *const F) -> usize {
    field as usize - (base as *const T as usize)
}

fn zeroed<T>() -> T
where
    T: bytemuck::Zeroable,
{
    // SAFETY: `T: bytemuck::Zeroable` makes the all-zero bit pattern valid.
    unsafe { std::mem::zeroed() }
}

fn minimal_valid() -> TelemetryArchive {
    let mut a = zeroed::<TelemetryArchive>();
    a.version = ARCHIVE_VERSION;
    a.meta.timestamp_ns = 1;
    a
}

fn fs16(text: &str) -> FixedString16 {
    FixedString16::from_bytes(text.as_bytes())
}

fn text_bytes<const N: usize>(text: &str) -> [u8; N] {
    let mut out = [0u8; N];
    out[..text.len()].copy_from_slice(text.as_bytes());
    out
}

fn maximal_valid() -> TelemetryArchive {
    let mut a = minimal_valid();
    a.capabilities = KNOWN_CAPABILITIES_MASK;
    a.cpu = CpuGlobalStat {
        user_ticks: 100,
        system_ticks: 50,
        idle_ticks: 850,
        total_ticks: 1000,
        context_switches: 500,
        context_switches_per_sec: 12.0,
        usage_percent: 15.0,
        cores: [CpuCoreStat {
            core_index: 0,
            _pad0: [0; 7],
            user_ticks: 50,
            system_ticks: 25,
            idle_ticks: 425,
            total_ticks: 500,
            usage_percent: 10.0,
            _pad1: [0; 4],
        }; MAX_CORES],
        core_count: 2,
        _pad0: [0; 7],
    };
    a.cpu.cores[1].core_index = 1;
    a.cpu.cores[1].usage_percent = 20.0;
    for core in a.cpu.cores.iter_mut().skip(2) {
        *core = zeroed();
    }
    a.process = ProcessStats {
        total: 10,
        running: 2,
        blocked: 1,
        sleeping: 3,
        top_cpu: [ProcessStat {
            pid: 42,
            cpu_usage: 50.0,
            memory_bytes: 1000,
            comm: fs16("top"),
        }; MAX_TOP_N],
        top_mem: [ProcessStat {
            pid: 43,
            cpu_usage: 0.0,
            memory_bytes: 2000,
            comm: fs16("mem"),
        }; MAX_TOP_N],
        top_cpu_count: 1,
        top_mem_count: 1,
        flags: PROCESS_TRUNCATED,
        _pad0: [0; 5],
    };
    a.process.top_cpu[1] = zeroed();
    a.process.top_cpu[2] = zeroed();
    a.process.top_cpu[3] = zeroed();
    a.process.top_cpu[4] = zeroed();
    a.process.top_mem[1] = zeroed();
    a.process.top_mem[2] = zeroed();
    a.process.top_mem[3] = zeroed();
    a.process.top_mem[4] = zeroed();
    a.memory = MemoryStats {
        ram_total: 100,
        ram_free: 20,
        ram_used: 80,
        buffers: 5,
        cached: 10,
        swap_total: 50,
        swap_free: 30,
        swap_used: 20,
        page_faults: 100,
        page_faults_per_sec: 2.0,
        _pad0: [0; 4],
    };
    let mut storage = StorageStats {
        disks: [zeroed(); MAX_DISKS],
        disk_count: 1,
        disk_truncated: 1,
        _pad0: [0; 6],
        mounts: [zeroed(); MAX_MOUNTS],
        mount_count: 1,
        mount_truncated: 0,
        _pad1: [0; 5],
    };
    storage.disks[0] = DiskStat {
        name: fs16("sda"),
        major: 8,
        minor: 0,
        read_bytes: 100,
        write_bytes: 200,
        read_bytes_per_sec: 1.0,
        write_bytes_per_sec: 2.0,
        read_iops: 3.0,
        write_iops: 4.0,
        queue_depth: 2,
        read_latency_ms: 0.5,
        write_latency_ms: 0.6,
        _pad0: [0; 4],
    };
    storage.mounts[0] = MountStat {
        mountpoint: text_bytes("/"),
        fstype: fs16("ext4"),
        total: 100,
        available: 40,
        used: 50,
        percent: 50.0,
        _pad0: [0; 4],
    };
    a.storage = storage;
    let mut network = NetworkStats {
        interfaces: [zeroed(); MAX_NETIFS],
        if_count: 1,
        truncated: 0,
        _pad0: [0; 6],
    };
    network.interfaces[0] = NetIfStat {
        name: fs16("eth0"),
        rx_bytes: 1000,
        tx_bytes: 2000,
        rx_bytes_per_sec: 3.0,
        tx_bytes_per_sec: 4.0,
    };
    a.network = network;
    a.meta = MetaStats {
        timestamp_ns: 123,
        wallclock_ns: 456,
        uptime_secs: 1000,
        load_avg_1m: 0.5,
        load_avg_5m: 1.0,
        load_avg_15m: 1.5,
        timezone_name: text_bytes("UTC"),
        timezone_offset_secs: 0,
        os: OsFingerprint {
            os_type: fs16("linux"),
            os_id: fs16("debian"),
            os_version_id: fs16("12"),
            version_codename: fs16("bookworm"),
            version: text_bytes("6.1.0"),
            os_pretty_name: text_bytes("Debian GNU/Linux 12"),
        },
    };
    let mut gpu = GpuStats {
        gpus: [zeroed(); MAX_GPUS],
        gpu_count: 1,
        nvml_available: 1,
        truncated: 0,
        _pad0: [0; 5],
    };
    gpu.gpus[0] = GpuStat {
        name: fs16("nv"),
        memory_total: 100,
        memory_used: 40,
        utilization_percent: 55.0,
        power_watts: 100.0,
        temperature_celsius: 70,
        available: 1,
        tone: TONE_MAGENTA,
        _pad0: [0; 4],
        capabilities: 0x3F,
    };
    a.gpu = gpu;
    a.derived = DerivedStats {
        ram_used_percent: 80.0,
        swap_used_percent: 40.0,
        aggregate_rx_bytes_per_sec: 3.0,
        aggregate_tx_bytes_per_sec: 4.0,
        cpu_tone: TONE_GREEN,
        ram_tone: TONE_RED,
        swap_tone: TONE_YELLOW,
        _reserved0: 0,
        _pad0: [0; 4],
    };
    a
}

fn reason(result: aura_common::AuraResult<()>) -> String {
    match result {
        Ok(()) => panic!("expected validation fault"),
        Err(AuraError::InvalidArchive { reason }) => reason,
        Err(other) => panic!("expected InvalidArchive, got {other:?}"),
    }
}

fn assert_fault(archive: &TelemetryArchive, expected: &str) {
    let got = reason(validate_archive(archive));
    assert_eq!(got, expected);
}

#[test]
fn archive_version_is_two() {
    assert_eq!(ARCHIVE_VERSION, 2);
}

#[test]
fn archive_layout_is_exact() {
    assert_eq!(size_of::<TelemetryArchive>(), 65_536);
    assert_eq!(size_of::<TelemetryArchive>() % 8, 0);
    assert_eq!(align_of::<TelemetryArchive>(), 8);
}

#[test]
fn archive_top_level_offsets_match_abi() {
    let a = minimal_valid();
    assert_eq!(offset_of(&a, addr_of!(a.version)), 0);
    assert_eq!(offset_of(&a, addr_of!(a.capabilities)), 8);
    assert_eq!(offset_of(&a, addr_of!(a.cpu)), 16);
    assert_eq!(offset_of(&a, addr_of!(a.process)), 6216);
    assert_eq!(offset_of(&a, addr_of!(a.memory)), 6560);
    assert_eq!(offset_of(&a, addr_of!(a.storage)), 6640);
    assert_eq!(offset_of(&a, addr_of!(a.network)), 17536);
    assert_eq!(offset_of(&a, addr_of!(a.meta)), 18184);
    assert_eq!(offset_of(&a, addr_of!(a.gpu)), 18488);
    assert_eq!(offset_of(&a, addr_of!(a.derived)), 18944);
    assert_eq!(offset_of(&a, addr_of!(a.checksum)), 18968);
    assert_eq!(offset_of(&a, addr_of!(a._reserved)), 18972);
}

#[test]
fn struct_sizes_match_abi() {
    assert_eq!(size_of::<FixedString16>(), 16);
    assert_eq!(size_of::<CpuCoreStat>(), 48);
    assert_eq!(size_of::<CpuGlobalStat>(), 6200);
    assert_eq!(size_of::<ProcessStat>(), 32);
    assert_eq!(size_of::<ProcessStats>(), 344);
    assert_eq!(size_of::<MemoryStats>(), 80);
    assert_eq!(size_of::<DiskStat>(), 72);
    assert_eq!(size_of::<MountStat>(), 304);
    assert_eq!(size_of::<StorageStats>(), 10_896);
    assert_eq!(size_of::<NetIfStat>(), 40);
    assert_eq!(size_of::<NetworkStats>(), 648);
    assert_eq!(size_of::<OsFingerprint>(), 256);
    assert_eq!(size_of::<MetaStats>(), 304);
    assert_eq!(size_of::<GpuStat>(), 56);
    assert_eq!(size_of::<GpuStats>(), 456);
    assert_eq!(size_of::<DerivedStats>(), 24);
}

#[test]
fn derived_stats_field_offsets_match_abi() {
    let d = zeroed::<DerivedStats>();
    assert_eq!(offset_of(&d, addr_of!(d.ram_used_percent)), 0);
    assert_eq!(offset_of(&d, addr_of!(d.swap_used_percent)), 4);
    assert_eq!(offset_of(&d, addr_of!(d.aggregate_rx_bytes_per_sec)), 8);
    assert_eq!(offset_of(&d, addr_of!(d.aggregate_tx_bytes_per_sec)), 12);
    assert_eq!(offset_of(&d, addr_of!(d.cpu_tone)), 16);
    assert_eq!(offset_of(&d, addr_of!(d.ram_tone)), 17);
    assert_eq!(offset_of(&d, addr_of!(d.swap_tone)), 18);
    assert_eq!(offset_of(&d, addr_of!(d._reserved0)), 19);
    assert_eq!(offset_of(&d, addr_of!(d._pad0)), 20);
}

#[test]
fn disk_stat_field_offsets_match_abi() {
    let d = zeroed::<DiskStat>();
    assert_eq!(offset_of(&d, addr_of!(d.name)), 0);
    assert_eq!(offset_of(&d, addr_of!(d.major)), 16);
    assert_eq!(offset_of(&d, addr_of!(d.minor)), 20);
    assert_eq!(offset_of(&d, addr_of!(d.read_bytes)), 24);
    assert_eq!(offset_of(&d, addr_of!(d.write_bytes)), 32);
    assert_eq!(offset_of(&d, addr_of!(d.read_bytes_per_sec)), 40);
    assert_eq!(offset_of(&d, addr_of!(d.write_bytes_per_sec)), 44);
    assert_eq!(offset_of(&d, addr_of!(d.read_iops)), 48);
    assert_eq!(offset_of(&d, addr_of!(d.write_iops)), 52);
    assert_eq!(offset_of(&d, addr_of!(d.queue_depth)), 56);
    assert_eq!(offset_of(&d, addr_of!(d.read_latency_ms)), 60);
    assert_eq!(offset_of(&d, addr_of!(d.write_latency_ms)), 64);
    assert_eq!(offset_of(&d, addr_of!(d._pad0)), 68);
}

#[test]
fn storage_stats_field_offsets_match_abi() {
    let s = zeroed::<StorageStats>();
    assert_eq!(offset_of(&s, addr_of!(s.disks)), 0);
    assert_eq!(offset_of(&s, addr_of!(s.disk_count)), 1152);
    assert_eq!(offset_of(&s, addr_of!(s.disk_truncated)), 1153);
    assert_eq!(offset_of(&s, addr_of!(s.mounts)), 1160);
    assert_eq!(offset_of(&s, addr_of!(s.mount_count)), 10888);
    assert_eq!(offset_of(&s, addr_of!(s.mount_truncated)), 10890);
}

#[test]
fn network_stats_tail_offsets_match_abi() {
    let n = zeroed::<NetworkStats>();
    assert_eq!(offset_of(&n, addr_of!(n.if_count)), 640);
    assert_eq!(offset_of(&n, addr_of!(n.truncated)), 641);
}

#[test]
fn process_stats_tail_offsets_match_abi() {
    let p = zeroed::<ProcessStats>();
    assert_eq!(offset_of(&p, addr_of!(p.top_cpu_count)), 336);
    assert_eq!(offset_of(&p, addr_of!(p.top_mem_count)), 337);
    assert_eq!(offset_of(&p, addr_of!(p.flags)), 338);
}

#[test]
fn os_fingerprint_offsets_match_abi() {
    let o = zeroed::<OsFingerprint>();
    assert_eq!(offset_of(&o, addr_of!(o.os_type)), 0);
    assert_eq!(offset_of(&o, addr_of!(o.os_id)), 16);
    assert_eq!(offset_of(&o, addr_of!(o.os_version_id)), 32);
    assert_eq!(offset_of(&o, addr_of!(o.version_codename)), 48);
    assert_eq!(offset_of(&o, addr_of!(o.version)), 64);
    assert_eq!(offset_of(&o, addr_of!(o.os_pretty_name)), 128);
}

#[test]
fn meta_stats_offsets_match_abi() {
    let m = zeroed::<MetaStats>();
    assert_eq!(offset_of(&m, addr_of!(m.timestamp_ns)), 0);
    assert_eq!(offset_of(&m, addr_of!(m.wallclock_ns)), 8);
    assert_eq!(offset_of(&m, addr_of!(m.uptime_secs)), 16);
    assert_eq!(offset_of(&m, addr_of!(m.load_avg_1m)), 24);
    assert_eq!(offset_of(&m, addr_of!(m.load_avg_5m)), 28);
    assert_eq!(offset_of(&m, addr_of!(m.load_avg_15m)), 32);
    assert_eq!(offset_of(&m, addr_of!(m.timezone_name)), 36);
    assert_eq!(offset_of(&m, addr_of!(m.timezone_offset_secs)), 44);
    assert_eq!(offset_of(&m, addr_of!(m.os)), 48);
}

#[test]
fn gpu_stat_offsets_match_abi() {
    let g = zeroed::<GpuStat>();
    assert_eq!(offset_of(&g, addr_of!(g.name)), 0);
    assert_eq!(offset_of(&g, addr_of!(g.memory_total)), 16);
    assert_eq!(offset_of(&g, addr_of!(g.memory_used)), 24);
    assert_eq!(offset_of(&g, addr_of!(g.utilization_percent)), 32);
    assert_eq!(offset_of(&g, addr_of!(g.power_watts)), 36);
    assert_eq!(offset_of(&g, addr_of!(g.temperature_celsius)), 40);
    assert_eq!(offset_of(&g, addr_of!(g.available)), 42);
    assert_eq!(offset_of(&g, addr_of!(g.tone)), 43);
    assert_eq!(offset_of(&g, addr_of!(g.capabilities)), 48);
    let s = zeroed::<GpuStats>();
    assert_eq!(offset_of(&s, addr_of!(s.gpu_count)), 448);
    assert_eq!(offset_of(&s, addr_of!(s.nvml_available)), 449);
    assert_eq!(offset_of(&s, addr_of!(s.truncated)), 450);
}

#[test]
fn archive_reserved_is_exact_size() {
    assert_eq!(size_of::<TelemetryArchive>() - 18972, 46_564);
}

#[test]
fn capability_bits_are_33_distinct() {
    assert_eq!(CAPABILITY_COUNT, 33);
    assert_eq!(KNOWN_CAPABILITIES_MASK, 0x1_FFFF_FFFF);
    let mut combined = 0u64;
    for bit in 0..CAPABILITY_COUNT {
        let flag = 1u64 << bit;
        assert_eq!(combined & flag, 0);
        combined |= flag;
    }
    assert_eq!(combined, KNOWN_CAPABILITIES_MASK);
}

#[test]
fn tone_codes_are_severity_ordered() {
    assert_eq!(TONE_GREEN, 0);
    assert_eq!(TONE_MAGENTA, 1);
    assert_eq!(TONE_YELLOW, 2);
    assert_eq!(TONE_RED, 3);
}

#[test]
fn crc32_golden_v2_zeroed_archive() {
    let mut a = zeroed::<TelemetryArchive>();
    a.version = ARCHIVE_VERSION;
    assert_eq!(a.calculate_checksum(), 0xe11d_ad37);
}

#[test]
fn crc32_legacy_algorithm_golden_differs() {
    let mut a = zeroed::<TelemetryArchive>();
    a.version = ARCHIVE_VERSION;
    let mut h = crc32fast::Hasher::new();
    h.update(bytemuck::bytes_of(&a));
    h.update(&0u32.to_le_bytes());
    assert_eq!(h.finalize(), 0xb579_6fc5);
    assert_ne!(a.calculate_checksum(), 0xb579_6fc5);
}

#[test]
fn crc32_ignores_current_checksum_field() {
    let mut a = zeroed::<TelemetryArchive>();
    a.version = ARCHIVE_VERSION;
    let before = a.calculate_checksum();
    a.checksum = 0xDEAD_BEEF;
    assert_eq!(a.calculate_checksum(), before);
}

#[test]
fn minimal_v2_archive_validates() {
    validate_archive(&minimal_valid()).expect("minimal archive must validate");
}

#[test]
fn maximal_all_capabilities_archive_validates() {
    validate_archive(&maximal_valid()).expect("maximal archive must validate");
}

#[test]
fn per_core_clear_with_capped_core_count_validates_when_cores_zeroed() {
    let mut a = minimal_valid();
    a.capabilities = CAP_CPU_GLOBAL;
    a.cpu.user_ticks = 100;
    a.cpu.system_ticks = 50;
    a.cpu.idle_ticks = 850;
    a.cpu.total_ticks = 1000;
    a.cpu.usage_percent = 15.0;
    a.cpu.core_count = MAX_CORES as u8;
    validate_archive(&a).expect("capped core count without per-core must validate");
}

#[test]
fn available_gpu_with_no_metric_bits_validates() {
    let mut a = minimal_valid();
    a.capabilities = CAP_GPU_ENUMERATION;
    a.gpu.gpu_count = 1;
    a.gpu.nvml_available = 1;
    a.gpu.gpus[0].available = 1;
    a.gpu.gpus[0].capabilities = 0;
    validate_archive(&a).expect("available GPU without metric bits must validate");
}

#[test]
fn process_trailing_top_slots_are_ignored() {
    let mut a = maximal_valid();
    a.process.top_cpu[3].pid = 0xFFFF_FFFF;
    a.process.top_cpu[3].cpu_usage = f32::NAN;
    a.process.top_mem[4].comm = fs16("garbage");
    a.process.top_mem[4].cpu_usage = 12.0;
    validate_archive(&a).expect("trailing top slots are ignored");
}

#[test]
fn unknown_capability_bits_reason_is_exact() {
    let mut a = minimal_valid();
    a.capabilities = (1 << 33) | (1 << 63);
    assert_fault(&a, "field capabilities: unknown bits 0x8000000200000000");
}

#[test]
fn per_core_without_global_reason_is_exact() {
    let mut a = minimal_valid();
    a.capabilities = CAP_CPU_PER_CORE;
    assert_fault(
        &a,
        "field capabilities: missing required capability cpu_global",
    );
}

#[test]
fn top_cpu_without_global_reason_is_exact() {
    let mut a = minimal_valid();
    a.capabilities = CAP_PROCESS_TOP_CPU;
    assert_fault(
        &a,
        "field capabilities: missing required capability cpu_global",
    );
}

fn clear_top_cpu(a: &mut TelemetryArchive) {
    a.capabilities &= !CAP_PROCESS_TOP_CPU;
    a.process.top_cpu_count = 0;
    a.process.top_cpu = [zeroed(); MAX_TOP_N];
}

#[test]
fn owner_clear_cpu_field_must_be_zero() {
    let mut a = maximal_valid();
    a.capabilities &= !CAP_CPU_PER_CORE;
    a.cpu.cores = [zeroed(); MAX_CORES];
    a.cpu.core_count = 0;
    clear_top_cpu(&mut a);
    a.capabilities &= !CAP_CPU_GLOBAL;
    assert_fault(&a, "field cpu.user_ticks: expected zero");
}

#[test]
fn owner_zero_check_precedes_value_check() {
    let mut a = minimal_valid();
    a.cpu.usage_percent = f32::NAN;
    assert_fault(&a, "field cpu.usage_percent: expected zero");
}

#[test]
fn capability_mask_fault_precedes_top_level_fields() {
    let mut a = minimal_valid();
    a.capabilities = 1 << 40;
    a.cpu.user_ticks = 5;
    assert_fault(&a, "field capabilities: unknown bits 0x0000010000000000");
}

#[test]
fn top_level_declaration_order_decides_first_fault() {
    let mut a = maximal_valid();
    a.capabilities &= !(CAP_MEMORY_RAM_TOTAL | CAP_MEMORY_RAM_USED);
    clear_top_cpu(&mut a);
    a.capabilities &= !CAP_CPU_GLOBAL;
    a.capabilities &= !CAP_CPU_PER_CORE;
    a.cpu.cores = [zeroed(); MAX_CORES];
    a.derived.ram_used_percent = 0.0;
    a.derived.ram_tone = 0;
    assert_fault(&a, "field cpu.user_ticks: expected zero");
}

#[test]
fn struct_field_declaration_order_decides_first_fault() {
    let mut a = maximal_valid();
    clear_top_cpu(&mut a);
    a.capabilities &= !CAP_CPU_GLOBAL;
    a.capabilities &= !CAP_CPU_PER_CORE;
    a.cpu.cores = [zeroed(); MAX_CORES];
    a.cpu.core_count = 0;
    assert_fault(&a, "field cpu.user_ticks: expected zero");
}

#[test]
fn array_ascending_index_decides_first_fault() {
    let mut a = maximal_valid();
    a.cpu.cores[0].user_ticks = 0;
    a.cpu.cores[1]._pad0[2] = 1;
    a.cpu.cores[1].usage_percent = 0.0;
    assert_fault(&a, "field cpu.cores[1].leading_padding: expected zero");
}

#[test]
fn core_count_exceeds_capacity_reason() {
    let mut a = maximal_valid();
    for (index, core) in a.cpu.cores.iter_mut().enumerate() {
        *core = zeroed();
        core.core_index = index as u8;
    }
    a.cpu.core_count = 200;
    assert_fault(&a, "field cpu.core_count: 200 exceeds 128");
}

#[test]
fn core_index_mismatch_reason() {
    let mut a = maximal_valid();
    a.cpu.cores[1].core_index = 7;
    assert_fault(
        &a,
        "field cpu.cores[1].core_index: inconsistent with cpu.core_count",
    );
}

#[test]
fn cpu_total_inconsistent_reason() {
    let mut a = maximal_valid();
    a.cpu.total_ticks = 10;
    assert_fault(
        &a,
        "field cpu.total_ticks: inconsistent with cpu.user_ticks",
    );
}

#[test]
fn usage_percent_range_reason() {
    let mut a = maximal_valid();
    a.cpu.usage_percent = 100.5;
    assert_fault(&a, "field cpu.usage_percent: outside 0..=100");
}

#[test]
fn usage_percent_non_finite_reason() {
    let mut a = maximal_valid();
    a.cpu.usage_percent = f32::NAN;
    assert_fault(&a, "field cpu.usage_percent: non-finite");
}

#[test]
fn process_states_sum_reason() {
    let mut a = maximal_valid();
    a.process.running = 9;
    assert_fault(&a, "field process.running: inconsistent with process.total");
}

#[test]
fn top_count_exceeds_reason() {
    let mut a = maximal_valid();
    let template = a.process.top_cpu[0];
    for record in a.process.top_cpu.iter_mut() {
        *record = template;
    }
    a.process.top_cpu_count = 6;
    assert_fault(&a, "field process.top_cpu_count: 6 exceeds 5");
}

#[test]
fn top_memory_cpu_usage_must_stay_zero() {
    let mut a = maximal_valid();
    a.process.top_mem[0].cpu_usage = 3.0;
    assert_fault(
        &a,
        "field process.top_memory[0].unowned_cpu_usage: expected zero",
    );
}

#[test]
fn process_flags_unknown_bits_reason() {
    let mut a = maximal_valid();
    a.process.flags = 0b10;
    assert_fault(&a, "field process.flags: unknown bits 0x02");
}

#[test]
fn ram_free_exceeds_total_reason() {
    let mut a = maximal_valid();
    a.memory.ram_free = 120;
    assert_fault(&a, "field memory.ram_free: 120 exceeds 100");
}

#[test]
fn swap_sum_inconsistent_reason() {
    let mut a = maximal_valid();
    a.memory.swap_free = 10;
    assert_fault(
        &a,
        "field memory.swap_free: inconsistent with memory.swap_total",
    );
}

#[test]
fn disk_listed_identity_must_be_nonempty() {
    let mut a = maximal_valid();
    a.storage.disks[0].name = FixedString16::new();
    assert_fault(
        &a,
        "field storage.disks[0].name: inconsistent with storage.disk_count",
    );
}

#[test]
fn network_rate_negative_reason() {
    let mut a = maximal_valid();
    a.network.interfaces[0].rx_bytes_per_sec = -1.0;
    assert_fault(
        &a,
        "field network.interfaces[0].rx_bytes_per_sec: outside 0..=340282350000000000000000000000000000000",
    );
}

#[test]
fn timestamp_zero_reason() {
    let mut a = maximal_valid();
    a.meta.timestamp_ns = 0;
    assert_fault(
        &a,
        "field meta.timestamp_ns: outside 1..=18446744073709551615",
    );
}

#[test]
fn timezone_offset_range_reason() {
    let mut a = maximal_valid();
    a.meta.timezone_offset_secs = 90_000;
    assert_fault(
        &a,
        "field meta.timezone_offset_secs: outside -86400..=86400",
    );
}

#[test]
fn os_identity_missing_member_faults() {
    let mut a = maximal_valid();
    a.meta.os.os_id = FixedString16::new();
    assert_fault(&a, "field meta.os.os_id: inconsistent with capabilities");
}

#[test]
fn os_identity_clear_zeroes_all_three_members() {
    let mut a = maximal_valid();
    a.capabilities &= !CAP_META_OS_IDENTITY;
    assert_fault(&a, "field meta.os.os_type: expected zero");
}

#[test]
fn os_version_bits_are_independent() {
    let mut a = maximal_valid();
    a.capabilities &= !CAP_META_OS_IDENTITY;
    a.meta.os.os_type = FixedString16::new();
    a.meta.os.os_id = FixedString16::new();
    a.meta.os.os_pretty_name = [0; 128];
    validate_archive(&a).expect("version fields stay valid without identity");
}

#[test]
fn invalid_utf8_reason() {
    let mut a = maximal_valid();
    a.meta.os.version[0] = 0xFF;
    assert_fault(&a, "field meta.os.version: invalid UTF-8");
}

#[test]
fn control_character_reason() {
    let mut a = maximal_valid();
    a.meta.os.os_pretty_name[0] = 0x1B;
    assert_fault(&a, "field meta.os.os_pretty_name: control character U+001B");
}

#[test]
fn post_nul_bytes_must_be_zero() {
    let mut a = maximal_valid();
    let nul = a
        .meta
        .os
        .version
        .iter()
        .position(|&b| b == 0)
        .expect("version has NUL");
    a.meta.os.version[nul + 1] = 1;
    assert_fault(&a, "field meta.os.version: expected zero");
}

#[test]
fn raw_textual_arrays_follow_string_rule() {
    let mut a = maximal_valid();
    a.meta.timezone_name = [0xFF; 8];
    assert_fault(&a, "field meta.timezone_name: invalid UTF-8");
}

#[test]
fn gpu_temperature_range_reason() {
    let mut a = maximal_valid();
    a.gpu.gpus[0].temperature_celsius = -300;
    assert_fault(
        &a,
        "field gpu.gpus[0].temperature_celsius: outside -273..=1000",
    );
}

#[test]
fn gpu_tone_range_reason() {
    let mut a = maximal_valid();
    a.gpu.gpus[0].tone = 9;
    assert_fault(&a, "field gpu.gpus[0].tone: outside 0..=3");
}

#[test]
fn gpu_name_nonempty_iff_record_bit_zero() {
    let mut a = maximal_valid();
    a.gpu.gpus[0].capabilities &= !1;
    assert_fault(&a, "field gpu.gpus[0].name: expected zero");
    let mut b = maximal_valid();
    b.gpu.gpus[0].capabilities &= !1;
    b.gpu.gpus[0].name = FixedString16::new();
    b.gpu.gpus[1] = GpuStat {
        name: fs16("ghost"),
        memory_total: 0,
        memory_used: 0,
        utilization_percent: 0.0,
        power_watts: 0.0,
        temperature_celsius: 0,
        available: 1,
        tone: 0,
        _pad0: [0; 4],
        capabilities: 0,
    };
    b.gpu.gpu_count = 2;
    assert_fault(&b, "field gpu.gpus[1].name: expected zero");
}

#[test]
fn gpu_record_unknown_bits_reason() {
    let mut a = maximal_valid();
    a.gpu.gpus[0].capabilities |= 0xC0;
    assert_fault(
        &a,
        "field gpu.gpus[0].capabilities: unknown bits 0x00000000000000c0",
    );
}

#[test]
fn derived_prerequisite_missing_zeroes_field() {
    let mut a = maximal_valid();
    a.capabilities &= !CAP_MEMORY_RAM_USED;
    a.memory.ram_used = 0;
    a.derived.ram_tone = 0;
    assert_fault(&a, "field derived.ram_used_percent: expected zero");
}

#[test]
fn derived_tone_zero_when_prerequisite_missing() {
    let mut a = maximal_valid();
    a.derived.cpu_tone = TONE_RED;
    a.capabilities &= !CAP_CPU_GLOBAL;
    a.cpu = CpuGlobalStat {
        user_ticks: 0,
        system_ticks: 0,
        idle_ticks: 0,
        total_ticks: 0,
        context_switches: a.cpu.context_switches,
        context_switches_per_sec: a.cpu.context_switches_per_sec,
        usage_percent: 0.0,
        cores: [zeroed(); MAX_CORES],
        core_count: 0,
        _pad0: [0; 7],
    };
    a.capabilities &= !CAP_CPU_PER_CORE;
    a.capabilities &= !CAP_PROCESS_TOP_CPU;
    a.process.top_cpu_count = 0;
    a.process.top_cpu = [zeroed(); MAX_TOP_N];
    assert_fault(&a, "field derived.cpu_tone: expected zero");
}

#[test]
fn archive_reserved_must_be_zero() {
    let mut a = maximal_valid();
    a._reserved[10] = 1;
    assert_fault(&a, "field archive.reserved: expected zero");
}

#[test]
fn swap_percent_requires_swap_capability() {
    let mut a = maximal_valid();
    a.capabilities &= !CAP_MEMORY_SWAP;
    a.memory.swap_total = 0;
    a.memory.swap_free = 0;
    a.memory.swap_used = 0;
    a.derived.swap_tone = 0;
    assert_fault(&a, "field derived.swap_used_percent: expected zero");
}

#[test]
fn aggregate_rates_require_network_bytes_and_rates() {
    let mut a = maximal_valid();
    a.capabilities &= !(CAP_NETWORK_BYTES | CAP_NETWORK_RATES);
    a.network = NetworkStats {
        interfaces: [zeroed(); MAX_NETIFS],
        if_count: 0,
        truncated: 0,
        _pad0: [0; 6],
    };
    assert_fault(
        &a,
        "field derived.aggregate_rx_bytes_per_sec: expected zero",
    );
}

#[test]
fn mount_available_exceeds_total_reason() {
    let mut a = maximal_valid();
    a.storage.mounts[0].available = 101;
    assert_fault(&a, "field storage.mounts[0].available: 101 exceeds 100");
}

#[test]
fn multiply_invalid_mask_precedence_golden() {
    let mut a = maximal_valid();
    a.capabilities |= 1 << 63;
    a.memory.ram_free = 10_000;
    a.gpu.gpus[0].tone = 9;
    assert_fault(&a, "field capabilities: unknown bits 0x8000000000000000");
}

#[test]
fn multiply_invalid_struct_order_golden() {
    let mut a = maximal_valid();
    a.memory.ram_free = 10_000;
    a.gpu.gpus[0].tone = 9;
    assert_fault(&a, "field memory.ram_free: 10000 exceeds 100");
}

#[test]
fn hidden_interval_table_is_sorted_and_non_overlapping() {
    let intervals = aura_common::hidden_intervals();
    assert!(!intervals.is_empty());
    for pair in intervals.windows(2) {
        assert!(
            pair[0].offset + pair[0].len <= pair[1].offset,
            "overlap or disorder between {:?} and {:?}",
            pair[0],
            pair[1]
        );
    }
}

#[test]
fn hidden_interval_table_covers_every_padding_byte_exactly_once() {
    let a = minimal_valid();
    let base = addr_of!(a) as usize;
    let mut expected: Vec<(usize, usize)> = Vec::new();
    let mut push = |field: usize, len: usize| expected.push((field, len));
    for i in 0..MAX_CORES {
        let core = &a.cpu.cores[i];
        let at = addr_of!(*core) as usize - base;
        push(at + 1, 7);
        push(at + 44, 4);
    }
    push(addr_of!(a.cpu._pad0) as usize - base, 7);
    let d = addr_of!(a.derived) as usize - base;
    push(d + 19, 1);
    push(d + 20, 4);
    push(addr_of!(a.process._pad0) as usize - base, 5);
    for i in 0..MAX_TOP_N {
        let rec = addr_of!(a.process.top_mem[i]) as usize - base;
        push(rec + 4, 4);
    }
    push(addr_of!(a.memory._pad0) as usize - base, 4);
    for i in 0..MAX_DISKS {
        push(addr_of!(a.storage.disks[i]._pad0) as usize - base, 4);
    }
    push(addr_of!(a.storage._pad0) as usize - base, 6);
    for i in 0..MAX_MOUNTS {
        push(addr_of!(a.storage.mounts[i]._pad0) as usize - base, 4);
    }
    push(addr_of!(a.storage._pad1) as usize - base, 5);
    push(addr_of!(a.network._pad0) as usize - base, 6);
    for i in 0..MAX_GPUS {
        push(addr_of!(a.gpu.gpus[i]._pad0) as usize - base, 4);
    }
    push(addr_of!(a.gpu._pad0) as usize - base, 5);
    push(addr_of!(a._reserved) as usize - base, 46_564);
    expected.sort_unstable();
    let mut actual: Vec<(usize, usize)> = aura_common::hidden_intervals()
        .iter()
        .map(|i| (i.offset, i.len))
        .collect();
    actual.sort_unstable();
    assert_eq!(actual, expected);
}

#[test]
fn hidden_interval_paths_match_synthetic_schema() {
    let paths: Vec<String> = aura_common::hidden_interval_paths();
    for required in [
        "archive.reserved",
        "cpu.padding",
        "cpu.cores[0].leading_padding",
        "cpu.cores[127].trailing_padding",
        "derived.reserved",
        "derived.padding",
        "process.padding",
        "process.top_memory[0].unowned_cpu_usage",
        "process.top_memory[4].unowned_cpu_usage",
        "memory.padding",
        "storage.disks[0].padding",
        "storage.disks[15].padding",
        "storage.disk_header_padding",
        "storage.mounts[0].padding",
        "storage.mounts[31].padding",
        "storage.mount_tail_padding",
        "network.padding",
        "gpu.gpus[0].padding",
        "gpu.gpus[7].padding",
        "gpu.padding",
    ] {
        assert!(paths.iter().any(|p| p == required), "missing {required}");
    }
    assert_eq!(paths.len(), aura_common::hidden_intervals().len());
}
