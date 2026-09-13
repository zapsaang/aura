use std::fs::OpenOptions;
use std::mem::MaybeUninit;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aura_common::{
    write_double_buffer, AuraError, CpuCoreStat, CpuGlobalStat, DoubleBufferHeader, FixedString16,
    GpuStat, GpuStats, NetIfStat, NetworkStats, ProcessStat, ProcessStats, StorageStats,
    TelemetryArchive, BUFFER_0_OFFSET, BUFFER_1_OFFSET, MAX_CORES, MAX_DISKS, MAX_MOUNTS,
    MAX_NETIFS, MAX_TOP_N, SHM_SIZE,
};
use memmap2::MmapOptions;

use super::TelemetryReader;

fn checksum_offset() -> usize {
    let uninit = MaybeUninit::<TelemetryArchive>::uninit();
    let base = uninit.as_ptr();
    // SAFETY: `addr_of!` forms a raw field pointer without reading the
    // uninitialized archive, and both pointers share one allocation.
    let field = unsafe { std::ptr::addr_of!((*base).checksum) };
    field as usize - base as usize
}

#[test]
fn read_returns_snapshot_from_active_buffer() {
    let dir = test_dir("stable");
    let path = dir.join("state.dat");
    let mut mmap = init_shm_file(&path);
    let telemetry = sample_telemetry(44.5);
    write_snapshot(&mut mmap, &telemetry);

    let reader = TelemetryReader::new(&path).unwrap();
    let out = reader.read().unwrap();

    assert_eq!(out.cpu.usage_percent, 44.5);
    cleanup(&dir);
}

#[test]
fn read_returns_checksum_mismatch_for_corrupt_active_buffer() {
    let dir = test_dir("checksum");
    let path = dir.join("state.dat");
    let mut mmap = init_shm_file(&path);
    write_snapshot(&mut mmap, &sample_telemetry(10.0));

    let base = mmap.as_mut_ptr();
    // SAFETY: `base` is from a writable `SHM_SIZE` mapping whose first bytes are an aligned `DoubleBufferHeader`.
    let header = unsafe { &*(base as *const DoubleBufferHeader) };
    let active_offset = if header
        .active_index
        .load(std::sync::atomic::Ordering::Relaxed)
        == 0
    {
        BUFFER_0_OFFSET
    } else {
        BUFFER_1_OFFSET
    };
    // SAFETY: `active_offset` selects an in-bounds archive buffer and the checksum field offset is aligned for `u32`.
    unsafe {
        let checksum_ptr = base.add(active_offset + checksum_offset()).cast::<u32>();
        *checksum_ptr = 0;
    }
    mmap.flush().unwrap();

    let reader = TelemetryReader::new(&path).unwrap();
    match reader.read() {
        Ok(_) => panic!("expected checksum mismatch"),
        Err(err) => assert!(matches!(err, AuraError::ChecksumMismatch { .. })),
    }
    cleanup(&dir);
}

#[test]
fn fresh_file_mtime_does_not_override_zero_timestamp() {
    let dir = test_dir("fresh");
    let path = dir.join("state.dat");
    let mut mmap = init_shm_file(&path);
    write_snapshot(&mut mmap, &sample_telemetry(20.0));

    let reader = TelemetryReader::new(&path).unwrap();
    let stale = sample_telemetry(20.0);

    assert!(!reader.is_fresh(&stale, Duration::from_secs(2)));
    cleanup(&dir);
}

fn test_dir(tag: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("aura-cli-reader-{tag}-{}-{ts}", std::process::id()));
    std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    // macOS temp dirs live under /var, a symlink the SHM security layer rejects.
    std::fs::canonicalize(&dir).unwrap()
}

fn init_shm_file(path: &Path) -> memmap2::MmapMut {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.set_len(SHM_SIZE as u64).unwrap();
    // SAFETY: the temp file was just sized to `SHM_SIZE`, matching the writable mapping length.
    unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() }
}

fn write_snapshot(mmap: &mut memmap2::MmapMut, telemetry: &TelemetryArchive) {
    let mut t = *telemetry;
    t.checksum = t.calculate_checksum();
    // SAFETY: `mmap` is a writable full-size test SHM mapping and `t` is a fully initialized snapshot.
    unsafe {
        write_double_buffer(mmap.as_mut_ptr(), &t).expect("publish snapshot");
    }
    mmap.flush().unwrap();
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

fn sample_telemetry(cpu_usage: f32) -> TelemetryArchive {
    // SAFETY: `TelemetryArchive` derives `bytemuck::Zeroable`, so the all-zero bit pattern is valid for every field.
    let mut t = unsafe { std::mem::zeroed::<TelemetryArchive>() };
    t.version = aura_common::ARCHIVE_VERSION;
    t.capabilities = aura_common::CAP_CPU_GLOBAL;
    t.meta.timestamp_ns = 1;
    t.cpu = CpuGlobalStat {
        user_ticks: 100,
        system_ticks: 50,
        idle_ticks: 100,
        total_ticks: 250,
        context_switches: 0,
        context_switches_per_sec: 0.0,
        usage_percent: cpu_usage,
        cores: [CpuCoreStat {
            core_index: 0,
            _pad0: [0; 7],
            user_ticks: 0,
            system_ticks: 0,
            idle_ticks: 0,
            total_ticks: 0,
            usage_percent: 0.0,
            _pad1: [0; 4],
        }; MAX_CORES],
        core_count: 0,
        _pad0: [0; 7],
    };
    t.process = ProcessStats {
        total: 0,
        running: 0,
        blocked: 0,
        sleeping: 0,
        top_cpu: [ProcessStat {
            pid: 0,
            cpu_usage: 0.0,
            memory_bytes: 0,
            comm: FixedString16::new(),
        }; MAX_TOP_N],
        top_mem: [ProcessStat {
            pid: 0,
            cpu_usage: 0.0,
            memory_bytes: 0,
            comm: FixedString16::new(),
        }; MAX_TOP_N],
        top_cpu_count: 0,
        top_mem_count: 0,
        flags: 0,
        _pad0: [0; 5],
    };
    t.storage = StorageStats {
        disks: [
            // SAFETY: `DiskStat` derives `bytemuck::Zeroable`, so an all-zero disk entry is valid.
            unsafe { std::mem::zeroed() };
            MAX_DISKS
        ],
        disk_count: 0,
        disk_truncated: 0,
        _pad0: [0; 6],
        mounts: [
            // SAFETY: `MountStat` derives `bytemuck::Zeroable`, so an all-zero mount entry is valid.
            unsafe { std::mem::zeroed() };
            MAX_MOUNTS
        ],
        mount_count: 0,
        mount_truncated: 0,
        _pad1: [0; 5],
    };
    t.network = NetworkStats {
        interfaces: [NetIfStat {
            name: FixedString16::new(),
            rx_bytes: 0,
            tx_bytes: 0,
            rx_bytes_per_sec: 0.0,
            tx_bytes_per_sec: 0.0,
        }; MAX_NETIFS],
        if_count: 0,
        truncated: 0,
        _pad0: [0; 6],
    };
    t.gpu = GpuStats {
        gpus: [GpuStat {
            name: FixedString16::new(),
            memory_total: 0,
            memory_used: 0,
            utilization_percent: 0.0,
            power_watts: 0.0,
            temperature_celsius: 0,
            available: 0,
            tone: 0,
            _pad0: [0; 4],
            capabilities: 0,
        }; 8],
        gpu_count: 0,
        nvml_available: 0,
        truncated: 0,
        _pad0: [0; 5],
    };
    t
}
