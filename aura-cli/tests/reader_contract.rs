use std::fs::OpenOptions;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aura_cli::reader::{timestamp_is_fresh, TelemetryReader};
use aura_common::{
    validate_archive, write_double_buffer, AuraError, CpuCoreStat, CpuGlobalStat, DerivedStats,
    DiskStat, FixedString16, GpuStat, GpuStats, MemoryStats, NetIfStat, NetworkStats,
    OsFingerprint, ProcessStat, ProcessStats, StorageStats, TelemetryArchive, ARCHIVE_VERSION,
    CAP_CPU_GLOBAL, CAP_GPU_ENUMERATION, KNOWN_CAPABILITIES_MASK, MAX_CORES, MAX_DISKS, MAX_GPUS,
    MAX_MOUNTS, MAX_NETIFS, MAX_TOP_N, SHM_SIZE, TONE_GREEN,
};
use memmap2::MmapOptions;

fn temp_shm_path(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "aura-reader-contract-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    dir.join("state.dat")
}

fn cleanup_shm(path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn write_shm(path: &Path, archive: &TelemetryArchive) {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.set_len(SHM_SIZE as u64).unwrap();
    // SAFETY: the temp file was just sized to `SHM_SIZE`, matching the mapping length.
    let mut mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() };
    let mut snapshot = *archive;
    snapshot.checksum = snapshot.calculate_checksum();
    // SAFETY: the mapping is a full writable SHM region and the snapshot is initialized.
    unsafe {
        write_double_buffer(mmap.as_mut_ptr(), &snapshot).expect("publish snapshot");
    }
    mmap.flush().unwrap();
}

fn minimal_valid() -> TelemetryArchive {
    // SAFETY: `TelemetryArchive` is `Zeroable`; the all-zero bit pattern is valid.
    let mut a = unsafe { std::mem::zeroed::<TelemetryArchive>() };
    a.version = ARCHIVE_VERSION;
    a.meta.timestamp_ns = 1;
    a
}

fn fs16(text: &str) -> FixedString16 {
    FixedString16::from_bytes(text.as_bytes())
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
    for core in a.cpu.cores[2..].iter_mut() {
        // SAFETY: `CpuCoreStat` is `Zeroable`; the all-zero bit pattern is valid.
        *core = unsafe { std::mem::zeroed() };
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
        flags: 0,
        _pad0: [0; 5],
    };
    a.process.top_cpu[1] = unsafe { std::mem::zeroed() };
    a.process.top_cpu[2] = unsafe { std::mem::zeroed() };
    a.process.top_cpu[3] = unsafe { std::mem::zeroed() };
    a.process.top_cpu[4] = unsafe { std::mem::zeroed() };
    a.process.top_mem[1] = unsafe { std::mem::zeroed() };
    a.process.top_mem[2] = unsafe { std::mem::zeroed() };
    a.process.top_mem[3] = unsafe { std::mem::zeroed() };
    a.process.top_mem[4] = unsafe { std::mem::zeroed() };
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
        disks: [unsafe { std::mem::zeroed() }; MAX_DISKS],
        disk_count: 1,
        disk_truncated: 0,
        _pad0: [0; 6],
        mounts: [unsafe { std::mem::zeroed() }; MAX_MOUNTS],
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
    storage.mounts[0].total = 100;
    storage.mounts[0].used = 50;
    storage.mounts[0].available = 40;
    storage.mounts[0].percent = 50.0;
    storage.mounts[0].mountpoint[0] = b'/';
    storage.mounts[0].fstype = fs16("ext4");
    a.storage = storage;
    let mut network = NetworkStats {
        interfaces: [unsafe { std::mem::zeroed() }; MAX_NETIFS],
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
    a.meta.wallclock_ns = 456;
    a.meta.uptime_secs = 1000;
    a.meta.load_avg_1m = 0.5;
    a.meta.load_avg_5m = 1.0;
    a.meta.load_avg_15m = 1.5;
    a.meta.timezone_name = [b'U', b'T', b'C', 0, 0, 0, 0, 0];
    a.meta.os = OsFingerprint {
        os_type: fs16("linux"),
        os_id: fs16("debian"),
        os_version_id: fs16("12"),
        version_codename: fs16("bookworm"),
        version: {
            let mut v = [0u8; 64];
            v[..5].copy_from_slice(b"6.1.0");
            v
        },
        os_pretty_name: {
            let mut p = [0u8; 128];
            p[..19].copy_from_slice(b"Debian GNU/Linux 12");
            p
        },
    };
    let mut gpu = GpuStats {
        gpus: [unsafe { std::mem::zeroed() }; MAX_GPUS],
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
        tone: TONE_GREEN,
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
        ram_tone: TONE_GREEN,
        swap_tone: TONE_GREEN,
        _reserved0: 0,
        _pad0: [0; 4],
    };
    a
}

fn read_error(archive: &TelemetryArchive, tag: &str) -> AuraError {
    let path = temp_shm_path(tag);
    write_shm(&path, archive);
    let reader = TelemetryReader::new(&path).unwrap();
    let result = reader.read();
    cleanup_shm(&path);
    result.expect_err("read must fail")
}

fn expect_invalid_archive(err: AuraError, expected_reason: &str) {
    match err {
        AuraError::InvalidArchive { reason } => assert_eq!(reason, expected_reason),
        other => panic!("expected InvalidArchive, got {other:?}"),
    }
}

#[test]
fn valid_minimal_archive_reads() {
    let path = temp_shm_path("minimal");
    write_shm(&path, &minimal_valid());
    let reader = TelemetryReader::new(&path).unwrap();
    let out = reader.read().unwrap();
    assert_eq!(out.version, ARCHIVE_VERSION);
    cleanup_shm(&path);
}

#[test]
fn valid_maximal_archive_reads_and_validates() {
    let archive = maximal_valid();
    validate_archive(&archive).unwrap();
    let path = temp_shm_path("maximal");
    write_shm(&path, &archive);
    let reader = TelemetryReader::new(&path).unwrap();
    let out = reader.read().unwrap();
    assert_eq!(out.capabilities, KNOWN_CAPABILITIES_MASK);
    assert_eq!(out.derived.ram_used_percent, 80.0);
    assert_eq!(out.gpu.gpus[0].temperature_celsius, 70);
    assert_eq!(out.storage.disks[0].major, 8);
    cleanup_shm(&path);
}

#[test]
fn version_zero_is_rejected() {
    let mut a = minimal_valid();
    a.version = 0;
    match read_error(&a, "v0") {
        AuraError::UnsupportedVersion { found } => assert_eq!(found, 0),
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn version_one_is_rejected() {
    let mut a = minimal_valid();
    a.version = 1;
    match read_error(&a, "v1") {
        AuraError::UnsupportedVersion { found } => assert_eq!(found, 1),
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn version_three_is_rejected() {
    let mut a = minimal_valid();
    a.version = 3;
    match read_error(&a, "v3") {
        AuraError::UnsupportedVersion { found } => assert_eq!(found, 3),
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[test]
fn version_fault_precedes_crc_check() {
    let path = temp_shm_path("precedence-vc");
    let mut a = minimal_valid();
    a.version = 1;
    write_shm(&path, &a);
    // Corrupt one payload byte without fixing the checksum.
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    // SAFETY: the temp file is exactly SHM_SIZE and mapped writable.
    let mut mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() };
    let header = mmap.as_ptr() as *const u64;
    // SAFETY: header points at the active_index of a live SHM-sized mapping.
    let active = unsafe { std::ptr::read_volatile(header) } & 1;
    let base_offset = 24 + (active as usize) * 65536;
    mmap[base_offset + 100] ^= 0xFF;
    mmap.flush().unwrap();
    let reader = TelemetryReader::new(&path).unwrap();
    match reader.read() {
        Err(AuraError::UnsupportedVersion { found }) => assert_eq!(found, 1),
        other => panic!("version fault must precede CRC, got {other:?}"),
    }
    cleanup_shm(&path);
}

#[test]
fn crc_fault_precedes_validation() {
    let mut a = minimal_valid();
    a.capabilities = 1 << 40;
    let path = temp_shm_path("precedence-cv");
    write_shm(&path, &a);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    // SAFETY: the temp file is exactly SHM_SIZE and mapped writable.
    let mut mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() };
    let header = mmap.as_ptr() as *const u64;
    // SAFETY: header points at the active_index of a live SHM-sized mapping.
    let active = unsafe { std::ptr::read_volatile(header) } & 1;
    let base_offset = 24 + (active as usize) * 65536;
    mmap[base_offset + 100] ^= 0xFF;
    mmap.flush().unwrap();
    let reader = TelemetryReader::new(&path).unwrap();
    match reader.read() {
        Err(AuraError::ChecksumMismatch { .. }) => {}
        other => panic!("CRC fault must precede validation, got {other:?}"),
    }
    cleanup_shm(&path);
}

#[test]
fn validation_fault_follows_crc_success() {
    let mut a = minimal_valid();
    a.capabilities = 1 << 40;
    let err = read_error(&a, "unknown-caps");
    expect_invalid_archive(err, "field capabilities: unknown bits 0x0000010000000000");
}

#[test]
fn owner_clear_field_fault_reaches_reader() {
    let mut a = minimal_valid();
    a.cpu.user_ticks = 7;
    let err = read_error(&a, "owner-clear");
    expect_invalid_archive(err, "field cpu.user_ticks: expected zero");
}

#[test]
fn invalid_tone_fault_reaches_reader() {
    let mut a = minimal_valid();
    a.capabilities = CAP_CPU_GLOBAL;
    a.cpu.user_ticks = 1;
    a.cpu.system_ticks = 1;
    a.cpu.idle_ticks = 1;
    a.cpu.total_ticks = 3;
    a.derived.cpu_tone = 9;
    let err = read_error(&a, "tone");
    expect_invalid_archive(err, "field derived.cpu_tone: outside 0..=3");
}

#[test]
fn zero_timestamp_is_offline_at_reader_boundary() {
    let mut a = minimal_valid();
    a.meta.timestamp_ns = 0;
    let err = read_error(&a, "ts-zero");
    assert!(
        matches!(err, AuraError::Offline(_)),
        "zero producer timestamp must report offline, got {err:?}"
    );
}

#[test]
fn invalid_utf8_fault_reaches_reader() {
    let mut a = minimal_valid();
    a.capabilities = CAP_GPU_ENUMERATION;
    a.gpu.gpu_count = 1;
    a.gpu.gpus[0].available = 1;
    a.gpu.gpus[0].capabilities = 1;
    a.gpu.gpus[0].name.bytes[0] = 0xFF;
    let err = read_error(&a, "utf8");
    expect_invalid_archive(err, "field gpu.gpus[0].name: invalid UTF-8");
}

#[test]
fn zeroed_shm_reports_unsupported_version() {
    let path = temp_shm_path("zeroed-shm");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    file.set_len(SHM_SIZE as u64).unwrap();
    let reader = TelemetryReader::new(&path).unwrap();
    match reader.read() {
        Err(AuraError::Offline(reason)) => assert_eq!(reason, "telemetry has not been published"),
        other => panic!("zeroed SHM must surface not-published offline, got {other:?}"),
    }
    cleanup_shm(&path);
}

#[test]
fn zero_monotonic_timestamp_is_offline() {
    // Given
    let threshold = Duration::from_secs(2);

    // When
    let fresh = timestamp_is_fresh(0, 10_000_000_000, threshold);

    // Then
    assert!(!fresh);
}

#[test]
fn future_monotonic_timestamp_is_offline() {
    // Given
    let threshold = Duration::from_secs(2);

    // When
    let fresh = timestamp_is_fresh(10_000_000_001, 10_000_000_000, threshold);

    // Then
    assert!(!fresh);
}

#[test]
fn stale_monotonic_timestamp_is_offline() {
    // Given
    let threshold = Duration::from_secs(2);

    // When
    let fresh = timestamp_is_fresh(7_999_999_999, 10_000_000_000, threshold);

    // Then
    assert!(!fresh);
}

#[test]
fn monotonic_timestamp_at_threshold_is_fresh() {
    // Given
    let threshold = Duration::from_secs(2);

    // When
    let fresh = timestamp_is_fresh(8_000_000_000, 10_000_000_000, threshold);

    // Then
    assert!(fresh);
}

#[test]
fn wrong_shm_size_is_rejected() {
    let path = temp_shm_path("wrong-size");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    file.set_len(1024).unwrap();
    let reader = TelemetryReader::new(&path);
    assert!(reader.is_err());
    cleanup_shm(&path);
}

#[test]
fn unsupported_version_display_is_exact() {
    let err = AuraError::UnsupportedVersion { found: 1 };
    assert_eq!(
        err.to_string(),
        "unsupported archive version 1 (expected 2)"
    );
}

#[test]
fn invalid_archive_display_is_exact() {
    let err = AuraError::InvalidArchive {
        reason: "field capabilities: unknown bits 0x0000000000000001".to_string(),
    };
    assert_eq!(
        err.to_string(),
        "invalid archive: field capabilities: unknown bits 0x0000000000000001"
    );
}

#[test]
fn seqlock_torn_write_is_retried_or_rejected() {
    let path = temp_shm_path("torn");
    write_shm(&path, &minimal_valid());
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    // SAFETY: the temp file is exactly SHM_SIZE and mapped writable.
    let mmap = unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() };
    let base = mmap.as_ptr() as *mut u8;
    // SAFETY: base points at a live SHM header; both sequence words are in-bounds atomics.
    unsafe {
        let seq = base.add(8) as *const AtomicU64;
        (*seq).fetch_add(1, Ordering::SeqCst);
        (*seq).fetch_add(1, Ordering::SeqCst);
    }
    let reader = TelemetryReader::new(&path).unwrap();
    let archive = reader
        .read()
        .expect("inactive sequence does not block read");
    assert_eq!(archive.version, ARCHIVE_VERSION);
    cleanup_shm(&path);
}
