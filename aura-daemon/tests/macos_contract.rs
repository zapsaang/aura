//! macOS public-API collector contract tests.
//!
//! Every test injects stable bytes through a fake host probe into the
//! production macOS collector paths, so the suite runs identically on any
//! host OS while locking the exact public-FFI semantics required on macOS.

use aura_common::{
    validate_archive, AuraError, AuraResult, TelemetryArchive, CAP_CPU_CONTEXT_SWITCHES,
    CAP_CPU_GLOBAL, CAP_CPU_PER_CORE, CAP_GPU_ENUMERATION, CAP_MEMORY_BUFFERS, CAP_MEMORY_CACHED,
    CAP_MEMORY_PAGE_FAULTS, CAP_MEMORY_RAM_FREE, CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED,
    CAP_MEMORY_SWAP, CAP_NETWORK_BYTES, CAP_NETWORK_RATES, CAP_PROCESS_BLOCKED,
    CAP_PROCESS_RUNNING, CAP_PROCESS_SLEEPING, CAP_PROCESS_TOP_CPU, CAP_PROCESS_TOP_MEMORY,
    CAP_PROCESS_TOTAL, MAX_CORES, MAX_NETIFS,
};
use aura_daemon::collectors::cpu::macos::{collect_cpu_from_probe, MacosCpuProbe};
use aura_daemon::collectors::cpu::CpuAvailability;
use aura_daemon::collectors::memory::macos::{
    collect_memory_from_probe, parse_timeval, MacosMemoryProbe, HOST_VM_INFO64_COUNT,
    HW_PAGESIZE_LEN, KERN_BOOTTIME_LEN, MACOS_ENOENT, MACOS_ENOTSUP, MACOS_EPERM,
    SYSCTL_HW_MEMSIZE, SYSCTL_HW_PAGESIZE, SYSCTL_VM_SWAPUSAGE, XSW_USAGE_LEN,
};
use aura_daemon::collectors::memory::MemoryAvailability;
use aura_daemon::collectors::network::macos::{
    collect_network_from_probe, iflist2_capacity, init_iflist2_buffer, MacosNetworkProbe, AF_LINK,
    IFLIST2_CAPACITY_MAX, IFM_IBYTES_OFFSET, IFM_OBYTES_OFFSET, IF_MSGHDR2_LEN, MACOS_ENOMEM,
    RTA_IFP, RTM_IFINFO2, RTM_VERSION,
};
use aura_daemon::collectors::process::{mark_unavailable, ProcessAvailability};
use aura_daemon::collectors::storage::StorageAvailability;
use aura_daemon::collectors::{
    CollectorScratch, CollectorSources, CycleCollector, FixedCollectorState, MetaGpuAvailability,
    NetIfKey, NetworkAvailability, ProviderOutcome, SystemCollector,
};
use aura_daemon::finalize::{Clock, ClockSample, SystemFinalizer};
use aura_daemon::lifecycle::Finalizer;

// ---------------------------------------------------------------------
// Fake probes
// ---------------------------------------------------------------------

struct FakeCpuProbe {
    result: Result<(u32, Vec<i32>), i32>,
}

impl MacosCpuProbe for FakeCpuProbe {
    fn processor_load_info(&mut self) -> Result<(u32, &[i32]), i32> {
        match &self.result {
            Ok((count, info)) => Ok((*count, info.as_slice())),
            Err(code) => Err(*code),
        }
    }
}

fn cpu_probe(cores: &[[i32; 4]]) -> FakeCpuProbe {
    let mut info = Vec::with_capacity(cores.len() * 4);
    for core in cores {
        info.extend_from_slice(core);
    }
    FakeCpuProbe {
        result: Ok((cores.len() as u32, info)),
    }
}

type SysctlResult = Result<Vec<u8>, i32>;

struct FakeMemoryProbe {
    vm: Result<(u32, Vec<i32>), i32>,
    sysctls: Vec<(&'static [u8], SysctlResult)>,
    page_size: u64,
}

impl MacosMemoryProbe for FakeMemoryProbe {
    fn vm_info64(&mut self) -> Result<(u32, &[i32]), i32> {
        match &self.vm {
            Ok((count, lanes)) => Ok((*count, lanes.as_slice())),
            Err(code) => Err(*code),
        }
    }

    fn sysctlbyname(&mut self, name: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        for (candidate, result) in &self.sysctls {
            if *candidate == name {
                return match result {
                    Ok(bytes) => {
                        let written = bytes.len().min(out.len());
                        out[..written].copy_from_slice(&bytes[..written]);
                        Ok(written)
                    }
                    Err(code) => Err(*code),
                };
            }
        }
        Err(MACOS_ENOENT)
    }

    fn page_size(&self) -> u64 {
        self.page_size
    }
}

fn vm_lanes(free: u32, active: u32, inactive: u32, wire: u32, faults: u64) -> Vec<i32> {
    let mut lanes = vec![0i32; HOST_VM_INFO64_COUNT as usize];
    lanes[0] = free as i32;
    lanes[1] = active as i32;
    lanes[2] = inactive as i32;
    lanes[3] = wire as i32;
    let bytes = faults.to_le_bytes();
    lanes[12] = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    lanes[13] = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    lanes
}

fn memsize_bytes(value: u64) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn swap_bytes(total: u64, avail: u64, used: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(XSW_USAGE_LEN);
    bytes.extend_from_slice(&total.to_le_bytes());
    bytes.extend_from_slice(&avail.to_le_bytes());
    bytes.extend_from_slice(&used.to_le_bytes());
    bytes.extend_from_slice(&4096u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes
}

fn memory_probe(faults: u64, swap: Result<Vec<u8>, i32>) -> FakeMemoryProbe {
    FakeMemoryProbe {
        vm: Ok((HOST_VM_INFO64_COUNT, vm_lanes(100, 200, 300, 50, faults))),
        sysctls: vec![
            (SYSCTL_HW_MEMSIZE, Ok(memsize_bytes(16 * 1024 * 1024))),
            (SYSCTL_VM_SWAPUSAGE, swap),
        ],
        page_size: 4096,
    }
}

const TEST_PAGE: usize = 4096;

struct FakeNetworkProbe {
    dump: Result<Vec<u8>, i32>,
    capacity: usize,
}

impl FakeNetworkProbe {
    fn ok(bytes: Vec<u8>) -> Self {
        Self {
            dump: Ok(bytes),
            capacity: IFLIST2_CAPACITY_MAX,
        }
    }

    fn err(code: i32) -> Self {
        Self {
            dump: Err(code),
            capacity: IFLIST2_CAPACITY_MAX,
        }
    }
}

impl MacosNetworkProbe for FakeNetworkProbe {
    fn iflist2_dump(&mut self) -> Result<&[u8], i32> {
        match &self.dump {
            Ok(bytes) if bytes.len() > self.capacity => Err(MACOS_ENOMEM),
            Ok(bytes) => Ok(bytes.as_slice()),
            Err(code) => Err(*code),
        }
    }
}

/// sockaddr_dl with the interface name inline; sa_len excludes padding and
/// the wire stride is max(sa_len, 8) rounded up to 8 (Darwin rounding).
fn sdl_sockaddr(index: u16, name: &[u8]) -> Vec<u8> {
    let sa_len = 8 + name.len();
    let mut sa = vec![0u8; (sa_len.max(8) + 7) / 8 * 8];
    sa[0] = sa_len as u8;
    sa[1] = AF_LINK;
    sa[2..4].copy_from_slice(&index.to_le_bytes());
    sa[5] = name.len() as u8;
    sa[8..8 + name.len()].copy_from_slice(name);
    sa
}

/// Non-AF_LINK sockaddr with an explicit sa_len and wire stride.
fn raw_sockaddr(sa_len: u8, family: u8) -> Vec<u8> {
    let mut sa = vec![0u8; ((sa_len as usize).max(8) + 7) / 8 * 8];
    sa[0] = sa_len;
    sa[1] = family;
    sa
}

/// One RTM_IFINFO2 message: 224-byte fixed header + sockaddr chain.
fn ifinfo2_record(index: u32, addrs: i32, rx: u64, tx: u64, sockaddrs: &[u8]) -> Vec<u8> {
    let msglen = IF_MSGHDR2_LEN + sockaddrs.len();
    let mut record = vec![0u8; msglen];
    record[0..2].copy_from_slice(&(msglen as u16).to_le_bytes());
    record[2] = RTM_VERSION;
    record[3] = RTM_IFINFO2;
    record[8..12].copy_from_slice(&index.to_le_bytes());
    record[16..20].copy_from_slice(&addrs.to_le_bytes());
    record[IFM_IBYTES_OFFSET..IFM_IBYTES_OFFSET + 8].copy_from_slice(&rx.to_le_bytes());
    record[IFM_OBYTES_OFFSET..IFM_OBYTES_OFFSET + 8].copy_from_slice(&tx.to_le_bytes());
    record[IF_MSGHDR2_LEN..].copy_from_slice(sockaddrs);
    record
}

/// One complete IFINFO2 message whose RTA_IFP is a valid sockaddr_dl.
fn if_dump_entry(index: u16, name: &[u8], rx: u64, tx: u64) -> Vec<u8> {
    ifinfo2_record(
        u32::from(index),
        RTA_IFP,
        rx,
        tx,
        &sdl_sockaddr(index, name),
    )
}

/// Valid non-IFINFO2 routing message of the given type.
fn other_record(msgtype: u8) -> Vec<u8> {
    let mut record = vec![0u8; 16];
    record[0..2].copy_from_slice(&16u16.to_le_bytes());
    record[2] = RTM_VERSION;
    record[3] = msgtype;
    record
}

fn single_if_probe(index: u16, name: &[u8], rx: u64, tx: u64) -> FakeNetworkProbe {
    FakeNetworkProbe::ok(if_dump_entry(index, name, rx, tx))
}

struct StepClock {
    next: u64,
}

impl Clock for StepClock {
    fn sample(&mut self) -> AuraResult<ClockSample> {
        let now = self.next;
        self.next += 1_000_000_000;
        Ok(ClockSample {
            monotonic_ns: now,
            wallclock_ns: now + 500,
        })
    }
}

fn finalizer(start_ns: u64) -> SystemFinalizer<StepClock> {
    SystemFinalizer::new(StepClock { next: start_ns })
}

fn assert_fatal<T>(result: AuraResult<T>) -> AuraError {
    match result {
        Err(error @ AuraError::Fatal(_)) => error,
        Err(other) => panic!("expected Fatal error, got {other:?}"),
        Ok(_) => panic!("expected Fatal error, got Ok"),
    }
}

// ---------------------------------------------------------------------
// CPU: host_processor_info(PROCESSOR_CPU_LOAD_INFO)
// ---------------------------------------------------------------------

#[test]
fn cpu_maps_per_core_user_nice_system_idle() {
    let mut probe = cpu_probe(&[[10, 20, 30, 5], [40, 50, 60, 7]]);
    let mut out = TelemetryArchive::zeroed().cpu;
    let availability = collect_cpu_from_probe(&mut probe, &mut out).expect("cpu collect");
    assert!(!availability.context_switches);
    assert!(!availability.over_capacity);
    assert_eq!(out.core_count, 2);
    assert_eq!(out.cores[0].user_ticks, 15);
    assert_eq!(out.cores[0].system_ticks, 20);
    assert_eq!(out.cores[0].idle_ticks, 30);
    assert_eq!(out.cores[0].total_ticks, 65);
    assert_eq!(out.cores[1].user_ticks, 47);
    assert_eq!(out.cores[1].total_ticks, 157);
    assert_eq!(out.cores[1].core_index, 1);
}

#[test]
fn cpu_aggregate_is_sum_over_all_cores() {
    let mut probe = cpu_probe(&[[10, 20, 30, 5], [40, 50, 60, 7], [1, 2, 3, 4]]);
    let mut out = TelemetryArchive::zeroed().cpu;
    collect_cpu_from_probe(&mut probe, &mut out).expect("cpu collect");
    assert_eq!(out.user_ticks, 15 + 47 + 5);
    assert_eq!(out.system_ticks, 20 + 50 + 2);
    assert_eq!(out.idle_ticks, 30 + 60 + 3);
    assert_eq!(out.total_ticks, 65 + 157 + 10);
    assert_eq!(out.context_switches, 0);
}

#[test]
fn cpu_over_128_cores_keeps_aggregate_drops_per_core() {
    let mut cores = [[0i32; 4]; 129];
    for (index, core) in cores.iter_mut().enumerate() {
        *core = [index as i32, 1, 1, 1];
    }
    let mut probe = cpu_probe(&cores);
    let mut out = TelemetryArchive::zeroed().cpu;
    let availability = collect_cpu_from_probe(&mut probe, &mut out).expect("cpu collect");
    assert!(availability.over_capacity);
    let expected_user: u64 = (0..129u64).sum::<u64>() + 129;
    assert_eq!(out.user_ticks, expected_user);
    assert_eq!(out.system_ticks, 129);
    assert_eq!(out.idle_ticks, 129);
    assert_eq!(out.total_ticks, expected_user + 258);
    assert_eq!(out.core_count, 0);
    assert!(out
        .cores
        .iter()
        .all(|core| core.total_ticks == 0 && core.user_ticks == 0));
}

#[test]
fn cpu_exactly_128_cores_keeps_per_core() {
    let cores = [[1i32, 2, 3, 4]; 128];
    let mut probe = cpu_probe(&cores);
    let mut out = TelemetryArchive::zeroed().cpu;
    let availability = collect_cpu_from_probe(&mut probe, &mut out).expect("cpu collect");
    assert!(!availability.over_capacity);
    assert_eq!(out.core_count, MAX_CORES as u8);
    assert_eq!(out.cores[127].user_ticks, 5);
    assert_eq!(out.cores[127].total_ticks, 10);
}

#[test]
fn cpu_kernel_error_is_fatal() {
    let mut probe = FakeCpuProbe { result: Err(5) };
    let mut out = TelemetryArchive::zeroed().cpu;
    let error = assert_fatal(collect_cpu_from_probe(&mut probe, &mut out));
    assert!(error.to_string().contains("host_processor_info"));
}

#[test]
fn cpu_zero_processor_count_is_fatal() {
    let mut probe = cpu_probe(&[]);
    let mut out = TelemetryArchive::zeroed().cpu;
    assert_fatal(collect_cpu_from_probe(&mut probe, &mut out));
}

#[test]
fn cpu_info_count_mismatch_is_fatal() {
    let mut probe = FakeCpuProbe {
        result: Ok((2, vec![0i32; 7])),
    };
    let mut out = TelemetryArchive::zeroed().cpu;
    assert_fatal(collect_cpu_from_probe(&mut probe, &mut out));
}

#[test]
fn cpu_negative_tick_is_fatal() {
    let mut probe = cpu_probe(&[[-1, 0, 0, 0]]);
    let mut out = TelemetryArchive::zeroed().cpu;
    assert_fatal(collect_cpu_from_probe(&mut probe, &mut out));
}

// ---------------------------------------------------------------------
// Memory: host_statistics64(HOST_VM_INFO64) + sysctlbyname
// ---------------------------------------------------------------------

#[test]
fn memory_happy_path_maps_all_fields() {
    let mut probe = memory_probe(9_999, Ok(swap_bytes(8_000, 5_000, 3_000)));
    let mut out = TelemetryArchive::zeroed().memory;
    let availability = collect_memory_from_probe(&mut probe, &mut out).expect("memory collect");
    assert_eq!(
        availability,
        MemoryAvailability {
            buffers: false,
            cached: true,
            swap: true,
            page_faults: true,
        }
    );
    assert_eq!(out.ram_total, 16 * 1024 * 1024);
    assert_eq!(out.ram_free, 100 * 4096);
    assert_eq!(out.ram_used, 16 * 1024 * 1024 - 100 * 4096);
    assert_eq!(out.cached, 300 * 4096);
    assert_eq!(out.buffers, 0);
    assert_eq!(out.page_faults, 9_999);
    assert_eq!(out.swap_total, 8_000);
    assert_eq!(out.swap_free, 5_000);
    assert_eq!(out.swap_used, 3_000);
}

#[test]
fn memory_vm_info_count_mismatch_is_fatal() {
    let mut probe = memory_probe(1, Ok(swap_bytes(8, 5, 3)));
    probe.vm = Ok((HOST_VM_INFO64_COUNT - 1, vm_lanes(1, 2, 3, 4, 1)));
    let mut out = TelemetryArchive::zeroed().memory;
    let error = assert_fatal(collect_memory_from_probe(&mut probe, &mut out));
    assert!(error.to_string().contains("HOST_VM_INFO64"));
}

#[test]
fn memory_vm_info_error_is_fatal() {
    let mut probe = memory_probe(1, Ok(swap_bytes(8, 5, 3)));
    probe.vm = Err(5);
    let mut out = TelemetryArchive::zeroed().memory;
    assert_fatal(collect_memory_from_probe(&mut probe, &mut out));
}

#[test]
fn memory_memsize_missing_is_fatal() {
    let mut probe = memory_probe(1, Ok(swap_bytes(8, 5, 3)));
    probe.sysctls[0].1 = Err(MACOS_ENOENT);
    let mut out = TelemetryArchive::zeroed().memory;
    assert_fatal(collect_memory_from_probe(&mut probe, &mut out));
}

#[test]
fn memory_memsize_wrong_size_is_fatal() {
    let mut probe = memory_probe(1, Ok(swap_bytes(8, 5, 3)));
    probe.sysctls[0].1 = Ok(vec![0u8; 4]);
    let mut out = TelemetryArchive::zeroed().memory;
    assert_fatal(collect_memory_from_probe(&mut probe, &mut out));
}

#[test]
fn memory_swap_enoent_is_swap_local_unavailable() {
    let mut probe = memory_probe(7, Err(MACOS_ENOENT));
    let mut out = TelemetryArchive::zeroed().memory;
    let availability = collect_memory_from_probe(&mut probe, &mut out).expect("memory collect");
    assert!(!availability.swap);
    assert!(availability.cached && availability.page_faults);
    assert_eq!(out.ram_total, 16 * 1024 * 1024);
    assert_eq!(out.swap_total, 0);
    assert_eq!(out.swap_free, 0);
    assert_eq!(out.swap_used, 0);
    assert_eq!(out.page_faults, 7);
}

#[test]
fn memory_swap_enotsup_is_swap_local_unavailable() {
    let mut probe = memory_probe(7, Err(MACOS_ENOTSUP));
    let mut out = TelemetryArchive::zeroed().memory;
    let availability = collect_memory_from_probe(&mut probe, &mut out).expect("memory collect");
    assert!(!availability.swap);
}

#[test]
fn memory_swap_eperm_is_swap_local_unavailable() {
    let mut probe = memory_probe(7, Err(MACOS_EPERM));
    let mut out = TelemetryArchive::zeroed().memory;
    let availability = collect_memory_from_probe(&mut probe, &mut out).expect("memory collect");
    assert!(!availability.swap);
}

#[test]
fn memory_swap_other_error_is_fatal() {
    let mut probe = memory_probe(7, Err(5));
    let mut out = TelemetryArchive::zeroed().memory;
    assert_fatal(collect_memory_from_probe(&mut probe, &mut out));
}

#[test]
fn memory_swap_wrong_size_is_fatal() {
    let mut probe = memory_probe(7, Ok(vec![0u8; XSW_USAGE_LEN - 4]));
    let mut out = TelemetryArchive::zeroed().memory;
    assert_fatal(collect_memory_from_probe(&mut probe, &mut out));
}

#[test]
fn memory_swap_inconsistent_is_fatal() {
    let mut probe = memory_probe(7, Ok(swap_bytes(8_000, 5_000, 2_999)));
    let mut out = TelemetryArchive::zeroed().memory;
    assert_fatal(collect_memory_from_probe(&mut probe, &mut out));
}

#[test]
fn boottime_timeval_requires_exact_16_bytes() {
    let mut bytes = [0u8; KERN_BOOTTIME_LEN];
    bytes[..8].copy_from_slice(&1_700_000_000i64.to_le_bytes());
    bytes[8..12].copy_from_slice(&250_000i32.to_le_bytes());
    let (sec, usec) = parse_timeval(&bytes).expect("valid timeval");
    assert_eq!(sec, 1_700_000_000);
    assert_eq!(usec, 250_000);
    assert!(parse_timeval(&bytes[..12]).is_err());
    assert!(parse_timeval(&[0u8; 20]).is_err());
}

#[test]
fn pagesize_sysctl_descriptor_is_exactly_four_bytes() {
    assert_eq!(SYSCTL_HW_PAGESIZE, b"hw.pagesize");
    assert_eq!(HW_PAGESIZE_LEN, 4);
}

// ---------------------------------------------------------------------
// Network: NET_RT_IFLIST2 routing dump through the fixed init buffer
// ---------------------------------------------------------------------

#[test]
fn iflist2_capacity_adds_one_page_rounds_up_and_caps() {
    assert_eq!(iflist2_capacity(3 * TEST_PAGE, TEST_PAGE), 4 * TEST_PAGE);
    assert_eq!(
        iflist2_capacity(3 * TEST_PAGE + 1, TEST_PAGE),
        5 * TEST_PAGE
    );
    assert_eq!(iflist2_capacity(0, TEST_PAGE), TEST_PAGE);
    assert_eq!(
        iflist2_capacity(IFLIST2_CAPACITY_MAX, TEST_PAGE),
        IFLIST2_CAPACITY_MAX
    );
    assert_eq!(
        iflist2_capacity(2 * IFLIST2_CAPACITY_MAX, TEST_PAGE),
        IFLIST2_CAPACITY_MAX
    );
}

#[test]
fn iflist2_init_count_query_failure_is_fatal() {
    assert_fatal(init_iflist2_buffer(Err(5), TEST_PAGE));
}

#[test]
fn iflist2_init_buffer_matches_computed_capacity() {
    let buffer = init_iflist2_buffer(Ok(3 * TEST_PAGE), TEST_PAGE).expect("init buffer");
    assert_eq!(buffer.len(), 4 * TEST_PAGE);
    assert!(buffer.iter().all(|byte| *byte == 0));
}

#[test]
fn network_preserves_routing_message_order() {
    let dump = [
        if_dump_entry(3, b"en2", 30, 40),
        if_dump_entry(1, b"en0", 10, 20),
        if_dump_entry(2, b"en1", 50, 60),
    ]
    .concat();
    let mut probe = FakeNetworkProbe::ok(dump);
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    let availability =
        collect_network_from_probe(&mut probe, &mut out, &mut keys).expect("network collect");
    assert_eq!(
        availability,
        NetworkAvailability {
            bytes: true,
            rates: true,
        }
    );
    assert_eq!(out.if_count, 3);
    assert_eq!(out.truncated, 0);
    assert_eq!(out.interfaces[0].name.as_str(), "en2");
    assert_eq!(out.interfaces[1].name.as_str(), "en0");
    assert_eq!(out.interfaces[2].name.as_str(), "en1");
    assert_eq!(out.interfaces[0].rx_bytes, 30);
    assert_eq!(out.interfaces[0].tx_bytes, 40);
    assert_eq!(out.interfaces[2].rx_bytes, 50);
    assert!(keys[0].matches(&NetIfKey::from_macos(3, b"en2")));
    assert!(keys[1].matches(&NetIfKey::from_macos(1, b"en0")));
    assert!(keys[2].matches(&NetIfKey::from_macos(2, b"en1")));
    assert!(keys[3].is_empty());
}

#[test]
fn network_skips_other_valid_message_types() {
    let dump = [
        other_record(14), // RTM_IFINFO (legacy 32-bit counters)
        if_dump_entry(1, b"en0", 10, 20),
        other_record(12), // RTM_NEWADDR
    ]
    .concat();
    let mut probe = FakeNetworkProbe::ok(dump);
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    collect_network_from_probe(&mut probe, &mut out, &mut keys).expect("network collect");
    assert_eq!(out.if_count, 1);
    assert_eq!(out.truncated, 0);
    assert_eq!(out.interfaces[0].name.as_str(), "en0");
    assert_eq!(out.interfaces[0].rx_bytes, 10);
}

#[test]
fn network_wrong_rtm_version_is_fatal() {
    let mut record = if_dump_entry(1, b"en0", 10, 20);
    record[2] = RTM_VERSION + 1;
    let mut probe = FakeNetworkProbe::ok(record);
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    assert_fatal(collect_network_from_probe(&mut probe, &mut out, &mut keys));
}

#[test]
fn network_malformed_global_lengths_are_fatal() {
    let mut zero_len = vec![0u8; 16];
    zero_len[2] = RTM_VERSION;
    zero_len[3] = RTM_IFINFO2;
    let mut probe = FakeNetworkProbe::ok(zero_len);
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    assert_fatal(collect_network_from_probe(&mut probe, &mut out, &mut keys));

    let mut oversized = if_dump_entry(1, b"en0", 10, 20);
    oversized[0..2].copy_from_slice(&u16::MAX.to_le_bytes());
    let mut probe = FakeNetworkProbe::ok(oversized);
    assert_fatal(collect_network_from_probe(&mut probe, &mut out, &mut keys));

    let trailing = [if_dump_entry(1, b"en0", 10, 20), vec![0u8; 3]].concat();
    let mut probe = FakeNetworkProbe::ok(trailing);
    assert_fatal(collect_network_from_probe(&mut probe, &mut out, &mut keys));
}

#[test]
fn network_rtax_walk_applies_darwin_sockaddr_rounding() {
    let sockaddrs = [
        raw_sockaddr(4, 2),      // RTAX_DST: stride max(4,8) -> 8
        raw_sockaddr(9, 2),      // RTAX_GATEWAY: stride 9 -> 16
        sdl_sockaddr(1, b"en0"), // RTAX_IFP
    ]
    .concat();
    let dump = ifinfo2_record(1, 0x01 | 0x02 | RTA_IFP, 111, 222, &sockaddrs);
    let mut probe = FakeNetworkProbe::ok(dump);
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    collect_network_from_probe(&mut probe, &mut out, &mut keys).expect("network collect");
    assert_eq!(out.if_count, 1);
    assert_eq!(out.truncated, 0);
    assert_eq!(out.interfaces[0].name.as_str(), "en0");
    assert_eq!(out.interfaces[0].rx_bytes, 111);
    assert_eq!(out.interfaces[0].tx_bytes, 222);
}

#[test]
fn network_rta_ifp_without_af_link_skips_with_truncation() {
    let bad = ifinfo2_record(1, RTA_IFP, 1, 1, &raw_sockaddr(16, 2));
    let good = if_dump_entry(2, b"en1", 30, 40);
    let mut probe = FakeNetworkProbe::ok([bad, good].concat());
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    collect_network_from_probe(&mut probe, &mut out, &mut keys).expect("network collect");
    assert_eq!(out.if_count, 1);
    assert_eq!(out.truncated, 1);
    assert_eq!(out.interfaces[0].name.as_str(), "en1");
}

#[test]
fn network_missing_rta_ifp_skips_with_truncation() {
    let record = ifinfo2_record(1, 0x01, 1, 1, &raw_sockaddr(16, 2));
    let mut probe = FakeNetworkProbe::ok(record);
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    let availability =
        collect_network_from_probe(&mut probe, &mut out, &mut keys).expect("network collect");
    assert_eq!(
        availability,
        NetworkAvailability {
            bytes: true,
            rates: true,
        }
    );
    assert_eq!(out.if_count, 0);
    assert_eq!(out.truncated, 1);
}

#[test]
fn network_malformed_nlen_or_index_skips_with_truncation() {
    let mut zero_nlen_sa = sdl_sockaddr(1, b"en0");
    zero_nlen_sa[5] = 0;
    let zero_nlen = ifinfo2_record(1, RTA_IFP, 1, 1, &zero_nlen_sa);

    let mut big_nlen_sa = sdl_sockaddr(2, b"en1");
    big_nlen_sa[5] = 17;
    let big_nlen = ifinfo2_record(2, RTA_IFP, 2, 2, &big_nlen_sa);

    let mut overrun_sa = sdl_sockaddr(3, b"en2");
    overrun_sa[5] = 12; // 8 + 12 overruns the declared sa_len of 11
    let overrun = ifinfo2_record(3, RTA_IFP, 3, 3, &overrun_sa);

    let mut zero_index_sa = sdl_sockaddr(4, b"en3");
    zero_index_sa[2..4].copy_from_slice(&0u16.to_le_bytes());
    let zero_index = ifinfo2_record(4, RTA_IFP, 4, 4, &zero_index_sa);

    let mut control_sa = sdl_sockaddr(5, b"en4");
    control_sa[8] = 0x1F;
    let control = ifinfo2_record(5, RTA_IFP, 5, 5, &control_sa);

    let good = if_dump_entry(9, b"en9", 30, 40);
    let mut probe =
        FakeNetworkProbe::ok([zero_nlen, big_nlen, overrun, zero_index, control, good].concat());
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    collect_network_from_probe(&mut probe, &mut out, &mut keys).expect("network collect");
    assert_eq!(out.if_count, 1);
    assert_eq!(out.truncated, 1);
    assert_eq!(out.interfaces[0].name.as_str(), "en9");
    assert_eq!(out.interfaces[0].rx_bytes, 30);
}

#[test]
fn network_short_ifinfo2_header_skips_with_truncation() {
    let mut record = if_dump_entry(1, b"en0", 10, 20);
    record.truncate(100);
    record[0..2].copy_from_slice(&100u16.to_le_bytes());
    let mut probe = FakeNetworkProbe::ok(record);
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    collect_network_from_probe(&mut probe, &mut out, &mut keys).expect("network collect");
    assert_eq!(out.if_count, 0);
    assert_eq!(out.truncated, 1);
}

#[test]
fn network_caps_at_16_and_sets_truncation() {
    let mut dump = Vec::new();
    for index in 0..17u16 {
        let name = format!("en{index}");
        dump.extend(if_dump_entry(
            index + 1,
            name.as_bytes(),
            u64::from(index),
            1,
        ));
    }
    let mut probe = FakeNetworkProbe::ok(dump);
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    collect_network_from_probe(&mut probe, &mut out, &mut keys).expect("network collect");
    assert_eq!(out.if_count, MAX_NETIFS as u8);
    assert_eq!(out.truncated, 1);
    assert_eq!(out.interfaces[0].name.as_str(), "en0");
    assert_eq!(out.interfaces[15].name.as_str(), "en15");
    assert_eq!(out.interfaces[15].rx_bytes, 15);
}

#[test]
fn network_duplicate_key_keeps_first_record() {
    let dump = [
        if_dump_entry(1, b"en0", 10, 20),
        if_dump_entry(1, b"en0", 99, 99),
    ]
    .concat();
    let mut probe = FakeNetworkProbe::ok(dump);
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    collect_network_from_probe(&mut probe, &mut out, &mut keys).expect("network collect");
    assert_eq!(out.if_count, 1);
    assert_eq!(out.truncated, 0);
    assert_eq!(out.interfaces[0].rx_bytes, 10);
}

#[test]
fn network_identity_key_is_sdl_index_plus_name() {
    let dump = [
        if_dump_entry(1, b"en0", 10, 20),
        if_dump_entry(7, b"en0", 30, 40),
    ]
    .concat();
    let mut probe = FakeNetworkProbe::ok(dump);
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    collect_network_from_probe(&mut probe, &mut out, &mut keys).expect("network collect");
    assert_eq!(out.if_count, 2);
    assert!(!keys[0].matches(&keys[1]));
    assert!(keys[0].matches(&NetIfKey::from_macos(1, b"en0")));
    assert!(keys[1].matches(&NetIfKey::from_macos(7, b"en0")));
}

#[test]
fn network_key_identity_is_stable_across_message_reorder() {
    let first = [
        if_dump_entry(1, b"en0", 10, 20),
        if_dump_entry(2, b"en1", 30, 40),
    ]
    .concat();
    let second = [
        if_dump_entry(2, b"en1", 30, 40),
        if_dump_entry(1, b"en0", 10, 20),
    ]
    .concat();
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys_a = [NetIfKey::empty(); MAX_NETIFS];
    let mut keys_b = [NetIfKey::empty(); MAX_NETIFS];
    collect_network_from_probe(&mut FakeNetworkProbe::ok(first), &mut out, &mut keys_a)
        .expect("first dump");
    collect_network_from_probe(&mut FakeNetworkProbe::ok(second), &mut out, &mut keys_b)
        .expect("second dump");
    assert!(keys_a[0].matches(&keys_b[1]));
    assert!(keys_a[1].matches(&keys_b[0]));
    assert!(keys_a[0].matches(&NetIfKey::from_macos(1, b"en0")));
    assert!(keys_a[1].matches(&NetIfKey::from_macos(2, b"en1")));
}

#[test]
fn network_enomen_clears_capability_without_truncation() {
    let mut probe = FakeNetworkProbe::err(MACOS_ENOMEM);
    let mut out = TelemetryArchive::zeroed().network;
    out.if_count = 3;
    out.truncated = 1;
    out.interfaces[0].rx_bytes = 77;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    keys[0] = NetIfKey::from_macos(1, b"en0");
    let availability =
        collect_network_from_probe(&mut probe, &mut out, &mut keys).expect("network collect");
    assert_eq!(
        availability,
        NetworkAvailability {
            bytes: false,
            rates: false,
        }
    );
    assert_eq!(out.if_count, 0);
    assert_eq!(out.truncated, 0);
    assert!(out
        .interfaces
        .iter()
        .all(|interface| interface.rx_bytes == 0 && interface.name.bytes[0] == 0));
    assert!(keys.iter().all(NetIfKey::is_empty));
}

#[test]
fn network_required_beyond_fixed_capacity_clears_capability() {
    let mut probe = FakeNetworkProbe::ok(if_dump_entry(1, b"en0", 10, 20));
    probe.capacity = 64; // the kernel dump needs more than the fixed buffer
    let mut out = TelemetryArchive::zeroed().network;
    out.if_count = 2;
    out.truncated = 1;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    keys[0] = NetIfKey::from_macos(1, b"en0");
    let availability =
        collect_network_from_probe(&mut probe, &mut out, &mut keys).expect("network collect");
    assert_eq!(
        availability,
        NetworkAvailability {
            bytes: false,
            rates: false,
        }
    );
    assert_eq!(out.if_count, 0);
    assert_eq!(out.truncated, 0);
    assert!(keys.iter().all(NetIfKey::is_empty));
}

#[test]
fn network_other_dump_error_is_fatal() {
    let mut probe = FakeNetworkProbe::err(MACOS_EPERM);
    let mut out = TelemetryArchive::zeroed().network;
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    assert_fatal(collect_network_from_probe(&mut probe, &mut out, &mut keys));
}

// ---------------------------------------------------------------------
// Capability honesty through the production SystemCollector orchestration
// ---------------------------------------------------------------------

struct MacosShapedSources {
    cpu: FakeCpuProbe,
    memory: FakeMemoryProbe,
    network: FakeNetworkProbe,
}

impl CollectorSources for MacosShapedSources {
    fn collect_cpu(
        &mut self,
        state: &mut FixedCollectorState,
        _scratch: &mut CollectorScratch,
    ) -> AuraResult<CpuAvailability> {
        collect_cpu_from_probe(&mut self.cpu, &mut state.archive.cpu)
    }

    fn collect_memory(
        &mut self,
        state: &mut FixedCollectorState,
        _scratch: &mut CollectorScratch,
    ) -> AuraResult<MemoryAvailability> {
        collect_memory_from_probe(&mut self.memory, &mut state.archive.memory)
    }

    fn collect_network(
        &mut self,
        state: &mut FixedCollectorState,
        _scratch: &mut CollectorScratch,
    ) -> AuraResult<NetworkAvailability> {
        collect_network_from_probe(
            &mut self.network,
            &mut state.archive.network,
            &mut state.net_keys,
        )
    }

    fn collect_storage(
        &mut self,
        _state: &mut FixedCollectorState,
        _scratch: &mut CollectorScratch,
    ) -> AuraResult<StorageAvailability> {
        Ok(StorageAvailability {
            disk_metrics: false,
            mounts: false,
        })
    }

    fn collect_process(
        &mut self,
        state: &mut FixedCollectorState,
        _scratch: &mut CollectorScratch,
    ) -> AuraResult<ProcessAvailability> {
        mark_unavailable(&mut state.archive.process);
        Ok(ProcessAvailability::unavailable())
    }

    fn collect_meta_and_gpu(
        &mut self,
        state: &mut FixedCollectorState,
    ) -> AuraResult<MetaGpuAvailability> {
        state.archive.meta.uptime_secs = 42;
        Ok(MetaGpuAvailability {
            uptime: true,
            load_average: false,
            timezone: false,
            os_identity: false,
            os_version_id: false,
            gpu_enumeration: false,
        })
    }
}

fn shaped_sources() -> MacosShapedSources {
    MacosShapedSources {
        cpu: cpu_probe(&[[10, 20, 30, 5], [40, 50, 60, 7]]),
        memory: memory_probe(1_000, Ok(swap_bytes(8_000, 5_000, 3_000))),
        network: single_if_probe(1, b"en0", 100, 200),
    }
}

#[test]
fn macos_capability_bits_are_honest() {
    let mut collector = SystemCollector::with_sources(shaped_sources());
    let mut state = FixedCollectorState::default();
    let mut scratch = CollectorScratch::default();
    let outcome = collector.collect(&mut state, &mut scratch);
    assert!(matches!(outcome, ProviderOutcome::Available(())));
    let caps = state.archive.capabilities;
    assert!(caps & CAP_CPU_GLOBAL != 0);
    assert!(caps & CAP_CPU_PER_CORE != 0);
    assert_eq!(caps & CAP_CPU_CONTEXT_SWITCHES, 0);
    assert!(caps & CAP_MEMORY_RAM_TOTAL != 0);
    assert!(caps & CAP_MEMORY_RAM_FREE != 0);
    assert!(caps & CAP_MEMORY_RAM_USED != 0);
    assert_eq!(caps & CAP_MEMORY_BUFFERS, 0);
    assert!(caps & CAP_MEMORY_CACHED != 0);
    assert!(caps & CAP_MEMORY_SWAP != 0);
    assert!(caps & CAP_MEMORY_PAGE_FAULTS != 0);
    assert!(caps & CAP_NETWORK_BYTES != 0);
    assert!(caps & CAP_NETWORK_RATES != 0);
    assert_eq!(
        caps & (CAP_PROCESS_TOTAL
            | CAP_PROCESS_RUNNING
            | CAP_PROCESS_BLOCKED
            | CAP_PROCESS_SLEEPING
            | CAP_PROCESS_TOP_CPU
            | CAP_PROCESS_TOP_MEMORY),
        0
    );
    assert_eq!(caps & CAP_GPU_ENUMERATION, 0);

    let mut finalize = finalizer(1_000_000_000);
    finalize.finalize(&mut state).expect("finalize");
    validate_archive(&state.archive).expect("archive must pass ABI validation");
    assert!(state.archive.derived.ram_used_percent > 0.0);
    assert!(state.archive.derived.swap_used_percent > 0.0);
}

#[test]
fn swap_and_network_unavailable_clear_their_caps() {
    let mut sources = shaped_sources();
    sources.memory = memory_probe(1_000, Err(MACOS_ENOTSUP));
    sources.network = FakeNetworkProbe::err(MACOS_ENOMEM);
    let mut collector = SystemCollector::with_sources(sources);
    let mut state = FixedCollectorState::default();
    let mut scratch = CollectorScratch::default();
    let outcome = collector.collect(&mut state, &mut scratch);
    assert!(matches!(outcome, ProviderOutcome::Available(())));
    let caps = state.archive.capabilities;
    assert_eq!(caps & CAP_MEMORY_SWAP, 0);
    assert!(caps & CAP_MEMORY_RAM_TOTAL != 0);
    assert_eq!(caps & (CAP_NETWORK_BYTES | CAP_NETWORK_RATES), 0);

    let mut finalize = finalizer(1_000_000_000);
    finalize.finalize(&mut state).expect("finalize");
    validate_archive(&state.archive).expect("archive must pass ABI validation");
    assert_eq!(state.archive.memory.swap_total, 0);
    assert_eq!(state.archive.network.if_count, 0);
    assert_eq!(state.archive.derived.swap_used_percent, 0.0);
    assert_eq!(state.archive.derived.aggregate_rx_bytes_per_sec, 0.0);
}

// ---------------------------------------------------------------------
// Finalize interactions: over-cap, hotplug, counter reset, page faults
// ---------------------------------------------------------------------

#[test]
fn over_capacity_finalize_clears_per_core_and_top_cpu() {
    let mut cores = [[0i32; 4]; 129];
    for (index, core) in cores.iter_mut().enumerate() {
        *core = [index as i32, 2, 3, 1];
    }
    let mut probe = cpu_probe(&cores);
    let mut state = FixedCollectorState::default();
    let availability = collect_cpu_from_probe(&mut probe, &mut state.archive.cpu).expect("cpu");
    assert!(availability.over_capacity);
    state.cpu_over_capacity = availability.over_capacity;
    state.archive.capabilities = availability.capability_mask();
    state.archive.process.top_cpu_count = 1;
    state.archive.process.top_cpu[0].pid = 9;
    state.archive.capabilities |= CAP_PROCESS_TOTAL | CAP_PROCESS_TOP_CPU;
    state.baselines.prev_timestamp_ns = 1_000_000_000;

    let mut finalize = finalizer(2_000_000_000);
    finalize.finalize(&mut state).expect("finalize");
    assert!(state.archive.capabilities & CAP_CPU_GLOBAL != 0);
    assert_eq!(state.archive.capabilities & CAP_CPU_PER_CORE, 0);
    assert_eq!(state.archive.capabilities & CAP_PROCESS_TOP_CPU, 0);
    assert_eq!(state.archive.process.top_cpu_count, 0);
    assert!(state
        .archive
        .cpu
        .cores
        .iter()
        .all(|core| core.total_ticks == 0));
    let expected_user: u64 = (0..129u64).sum::<u64>() + 129;
    assert_eq!(state.archive.cpu.user_ticks, expected_user);
    assert!(state.archive.cpu.total_ticks > 0);
    validate_archive(&state.archive).expect("archive must pass ABI validation");
}

#[test]
fn cpu_hotplug_zeroes_usage_delta() {
    let mut probe = cpu_probe(&[[10, 20, 30, 5]]);
    let mut state = FixedCollectorState::default();
    let availability = collect_cpu_from_probe(&mut probe, &mut state.archive.cpu).expect("cpu");
    state.archive.capabilities = availability.capability_mask();
    state.baselines.prev_timestamp_ns = 1_000_000_000;
    state.baselines.core_count = 2;
    state.baselines.cores[0].total = 100;
    state.baselines.cores[1].total = 100;
    state.baselines.cpu_ticks.total = 200;

    let mut finalize = finalizer(2_000_000_000);
    finalize.finalize(&mut state).expect("finalize");
    assert_eq!(state.archive.cpu.usage_percent, 0.0);
    assert_eq!(state.archive.cpu.cores[0].usage_percent, 0.0);
}

#[test]
fn network_counter_reset_yields_zero_rates() {
    let mut state = FixedCollectorState::default();
    state.archive.capabilities = CAP_NETWORK_BYTES | CAP_NETWORK_RATES;
    state.baselines.prev_timestamp_ns = 1_000_000_000;

    let mut first = single_if_probe(1, b"en0", 1_000, 2_000);
    collect_network_from_probe(&mut first, &mut state.archive.network, &mut state.net_keys)
        .expect("cycle 1");
    let mut finalize = finalizer(2_000_000_000);
    finalize.finalize(&mut state).expect("finalize 1");
    assert_eq!(state.archive.network.interfaces[0].rx_bytes_per_sec, 0.0);

    let mut second = single_if_probe(1, b"en0", 1_500, 2_400);
    collect_network_from_probe(&mut second, &mut state.archive.network, &mut state.net_keys)
        .expect("cycle 2");
    finalize.finalize(&mut state).expect("finalize 2");
    assert_eq!(state.archive.network.interfaces[0].rx_bytes_per_sec, 500.0);
    assert_eq!(state.archive.network.interfaces[0].tx_bytes_per_sec, 400.0);
    assert_eq!(state.archive.derived.aggregate_rx_bytes_per_sec, 500.0);

    let mut reset = single_if_probe(1, b"en0", 100, 50);
    collect_network_from_probe(&mut reset, &mut state.archive.network, &mut state.net_keys)
        .expect("cycle 3");
    finalize.finalize(&mut state).expect("finalize 3");
    assert_eq!(state.archive.network.interfaces[0].rx_bytes_per_sec, 0.0);
    assert_eq!(state.archive.network.interfaces[0].tx_bytes_per_sec, 0.0);
    assert_eq!(state.archive.derived.aggregate_rx_bytes_per_sec, 0.0);
}

#[test]
fn page_fault_delta_is_computed_by_finalize() {
    let mut probe = memory_probe(160, Ok(swap_bytes(8_000, 5_000, 3_000)));
    let mut state = FixedCollectorState::default();
    let availability =
        collect_memory_from_probe(&mut probe, &mut state.archive.memory).expect("memory collect");
    assert!(availability.page_faults);
    state.archive.capabilities = CAP_MEMORY_RAM_TOTAL
        | CAP_MEMORY_RAM_FREE
        | CAP_MEMORY_RAM_USED
        | CAP_MEMORY_CACHED
        | CAP_MEMORY_PAGE_FAULTS;
    state.baselines.prev_timestamp_ns = 1_000_000_000;
    state.baselines.prev_page_faults = 100;

    let mut finalize = finalizer(2_000_000_000);
    finalize.finalize(&mut state).expect("finalize");
    assert_eq!(state.archive.memory.page_faults_per_sec, 60.0);
    assert_eq!(state.baselines.prev_page_faults, 160);
}

// ---------------------------------------------------------------------
// Probe trait shape
// ---------------------------------------------------------------------

#[test]
fn probe_traits_are_object_safe() {
    fn _cpu(_probe: &dyn MacosCpuProbe) {}
    fn _memory(_probe: &dyn MacosMemoryProbe) {}
    fn _network(_probe: &dyn MacosNetworkProbe) {}
}

// ---------------------------------------------------------------------
// Forbidden private-API symbol absence in production sources
// ---------------------------------------------------------------------

#[test]
fn production_sources_have_no_private_apple_symbols() {
    let denylist = [
        "proc_pidinfo",
        "proc_listpids",
        "proc_listallpids",
        "proc_name",
        "proc_pid",
        "libproc",
        "IOKit",
        "IOSurface",
        "IOServiceMatching",
        "CoreFoundation",
        "system_profiler",
        "IOAccelerator",
        "KERN_PROC",
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);
    assert!(!files.is_empty(), "no production sources found");
    for file in files {
        let text = std::fs::read_to_string(&file).expect("read production source");
        for needle in denylist {
            assert!(
                !text.contains(needle),
                "{} contains forbidden private symbol {needle}",
                file.display()
            );
        }
    }
}

fn collect_rs_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

// ---------------------------------------------------------------------
// Zero allocation after warm-up across all three macOS collector paths
// ---------------------------------------------------------------------

mod alloc_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    pub struct CountingAllocator;

    static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
    static ACTIVE: AtomicBool = AtomicBool::new(false);

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if ACTIVE.load(Ordering::Relaxed) {
                ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            }
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    pub fn start() {
        ALLOCATIONS.store(0, Ordering::Relaxed);
        ACTIVE.store(true, Ordering::Relaxed);
    }

    pub fn finish() -> usize {
        ACTIVE.store(false, Ordering::Relaxed);
        ALLOCATIONS.load(Ordering::Relaxed)
    }
}

#[global_allocator]
static MACOS_TEST_ALLOCATOR: alloc_probe::CountingAllocator = alloc_probe::CountingAllocator;

#[test]
fn warmed_collect_cycles_allocate_zero() {
    const CHILD_MARKER: &str = "AURA_MACOS_ALLOC_PROBE_CHILD";
    if std::env::var_os(CHILD_MARKER).is_none() {
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("warmed_collect_cycles_allocate_zero")
            .arg("--test-threads=1")
            .env(CHILD_MARKER, "1")
            .output()
            .expect("run isolated allocation probe");
        assert!(
            output.status.success(),
            "isolated allocation probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let mut cpu = cpu_probe(&[[10, 20, 30, 5], [40, 50, 60, 7]]);
    let mut memory = memory_probe(1_000, Ok(swap_bytes(8_000, 5_000, 3_000)));
    let mut network = single_if_probe(1, b"en0", 100, 200);
    let mut out = TelemetryArchive::zeroed();
    let mut keys = [NetIfKey::empty(); MAX_NETIFS];

    for _ in 0..2 {
        collect_cpu_from_probe(&mut cpu, &mut out.cpu).expect("warm-up cpu");
        collect_memory_from_probe(&mut memory, &mut out.memory).expect("warm-up memory");
        collect_network_from_probe(&mut network, &mut out.network, &mut keys)
            .expect("warm-up network");
    }

    alloc_probe::start();
    for _ in 0..50 {
        collect_cpu_from_probe(&mut cpu, &mut out.cpu).expect("measured cpu");
        collect_memory_from_probe(&mut memory, &mut out.memory).expect("measured memory");
        collect_network_from_probe(&mut network, &mut out.network, &mut keys)
            .expect("measured network");
    }
    assert_eq!(alloc_probe::finish(), 0, "warm macOS collect allocated");
}

// ---------------------------------------------------------------------
// Real-host smoke test (macOS CI lane only)
// ---------------------------------------------------------------------

#[cfg(target_os = "macos")]
#[test]
fn real_host_collects_live_telemetry() {
    let mut state = aura_daemon::collectors::CollectorState::new();
    aura_daemon::collectors::init(&mut state).expect("platform init");

    let mut out = TelemetryArchive::zeroed();
    let mut buf = Vec::new();
    let mut aux = Vec::new();
    let cpu = aura_daemon::collectors::cpu::macos::collect(&mut buf, &mut out.cpu).expect("cpu");
    assert!(out.cpu.core_count >= 1);
    assert!(out.cpu.total_ticks >= out.cpu.idle_ticks);
    assert!(!cpu.context_switches);

    let memory =
        aura_daemon::collectors::memory::macos::collect(&mut buf, &mut aux, &mut out.memory)
            .expect("memory");
    assert!(out.memory.ram_total > 0);
    assert!(out.memory.ram_free <= out.memory.ram_total);
    assert!(memory.page_faults);

    let mut keys = [NetIfKey::empty(); MAX_NETIFS];
    let network =
        aura_daemon::collectors::network::macos::collect_with_keys(&mut out.network, &mut keys)
            .expect("network");
    assert!(network.bytes && network.rates);
    assert!(out.network.if_count >= 1);
    for index in 0..out.network.if_count as usize {
        assert!(!keys[index].is_empty());
    }
}
