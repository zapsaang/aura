use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use aura_common::{
    read_double_buffer, AuraError, AuraResult, DoubleBufferHeader, FixedString16, TelemetryArchive,
    BUFFER_0_OFFSET, BUFFER_1_OFFSET, SHM_SIZE,
};
use aura_daemon::state::ShmHandle;
use memmap2::{Mmap, MmapOptions};
use tempfile::TempDir;

struct TelemetryReader {
    mmap: Mmap,
    #[allow(dead_code)]
    path: PathBuf,
}

impl TelemetryReader {
    fn new(path: &Path) -> AuraResult<Self> {
        let file = OpenOptions::new().read(true).open(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                AuraError::MmapFailed(e.to_string())
            } else {
                AuraError::SharedMemory(e)
            }
        })?;

        let mmap = unsafe {
            MmapOptions::new()
                .len(SHM_SIZE)
                .map(&file)
                .map_err(|e| AuraError::MmapFailed(e.to_string()))?
        };

        Ok(Self {
            mmap,
            path: path.to_path_buf(),
        })
    }

    fn read(&self) -> AuraResult<TelemetryArchive> {
        unsafe {
            read_double_buffer(self.mmap.as_ptr() as *mut u8)
                .map_err(|()| AuraError::SeqLockInvalid)
        }
    }
}

#[test]
fn ipc_roundtrip_write_with_daemon_read_with_cli_reader() {
    let tmp = TempDir::new().expect("create temp dir");
    let path = tmp.path().join("aura-ipc-roundtrip.dat");

    let mut expected = sample_archive();

    let mut shm = ShmHandle::new(&path).expect("create shm handle");
    shm.write(&mut expected).expect("write telemetry snapshot");

    let reader = TelemetryReader::new(&path).expect("open telemetry reader");
    let actual = reader.read().expect("read telemetry snapshot");

    assert_archive_fields_equal(&expected, &actual);

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("open shm file for mmap validation");
    let mmap = unsafe {
        MmapOptions::new()
            .len(SHM_SIZE)
            .map(&file)
            .expect("map shm file")
    };

    let header = unsafe { &*(mmap.as_ptr() as *const DoubleBufferHeader) };
    let final_seq = header.seq[1].load(Ordering::Acquire);
    let active = header.active_index.load(Ordering::Acquire);
    assert_eq!(
        final_seq, 2,
        "expected seq[1]=2 after one write (0->1->2), got {final_seq}"
    );
    assert_eq!(
        active, 1,
        "first write should publish buffer index 1 from zeroed initial state"
    );

    let active_offset = if active == 0 {
        BUFFER_0_OFFSET
    } else {
        BUFFER_1_OFFSET
    };
    let checksum_offset = active_offset + std::mem::offset_of!(TelemetryArchive, checksum);
    let stored_checksum = u32::from_le_bytes([
        mmap[checksum_offset],
        mmap[checksum_offset + 1],
        mmap[checksum_offset + 2],
        mmap[checksum_offset + 3],
    ]);
    let expected_checksum = {
        let mut t = expected;
        t.checksum = 0;
        t.calculate_checksum()
    };
    assert_eq!(
        stored_checksum, expected_checksum,
        "checksum persisted in shared memory"
    );
}

/// Regression test: reader reading buffer 0 must NOT be blocked by writer
/// actively writing to buffer 1 (false contention bug).
#[test]
fn reader_not_blocked_by_writer_on_other_buffer() {
    let tmp = TempDir::new().expect("create temp dir");
    let path = tmp.path().join("aura-false-contention.dat");

    let mut shm = ShmHandle::new(&path).expect("create shm handle");

    // Write twice to cycle: first write->buffer1(active=1), second write->buffer0(active=0)
    let mut archive = sample_archive();
    archive.version = 42;
    shm.write(&mut archive).expect("first write to buffer 1");
    archive.version = 43;
    shm.write(&mut archive).expect("second write to buffer 0");

    // Manipulate header to simulate false contention:
    // active=0 (reader reads buffer 0), seq[0]=2 (valid), seq[1]=1 (writer on buffer 1)
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("open shm for header manipulation");
    let mut mmap = unsafe {
        MmapOptions::new()
            .len(SHM_SIZE)
            .map_mut(&file)
            .expect("mmap for header manipulation")
    };

    let header = unsafe { &mut *(mmap.as_mut_ptr() as *mut DoubleBufferHeader) };

    // After two writes: active=0, seq[0]=2, seq[1]=2
    assert_eq!(header.active_index.load(Ordering::Acquire), 0);

    // Simulate writer mid-write to buffer 1: seq[1]=1 (odd)
    // With NEW per-buffer seq: reader reading buffer 0 checks seq[0]=2 (even) -> succeeds
    // With OLD single write_seq: writer writing to buffer 1 increments write_seq to odd -> reader fails
    header.seq[1].store(1, Ordering::Release);

    let reader = TelemetryReader::new(&path).expect("open reader");
    let result = reader.read();

    // NEW code succeeds: reader sees seq[0]=2 (even), writer is on buffer 1 not 0
    // OLD code fails: write_seq is... wait, the old code doesn't have seq array
    // The test using seq[1] will FAIL TO COMPILE until we implement the fix
    assert!(
        result.is_ok(),
        "reader reading buffer 0 should succeed even when writer is mid-write to buffer 1"
    );
    assert_eq!(result.unwrap().version, 43);
}

/// Verifies that with per-buffer seq, reader IS blocked when writer
/// is actively writing to the SAME buffer the reader is reading.
#[test]
fn reader_blocked_by_writer_on_same_buffer() {
    let tmp = TempDir::new().expect("create temp dir");
    let path = tmp.path().join("aura-same-buffer-contention.dat");

    let mut shm = ShmHandle::new(&path).expect("create shm handle");

    let mut archive = sample_archive();
    archive.version = 77;
    shm.write(&mut archive).expect("seed archive");

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("open shm for same-buffer test");
    let mut mmap = unsafe {
        MmapOptions::new()
            .len(SHM_SIZE)
            .map_mut(&file)
            .expect("mmap for same-buffer test")
    };

    let header = unsafe { &mut *(mmap.as_mut_ptr() as *mut DoubleBufferHeader) };

    // After first write: active=1, seq[1]=2
    assert_eq!(header.active_index.load(Ordering::Acquire), 1);

    // Simulate writer MID-WRITE to buffer 1: seq[1]=1 (odd)
    header.seq[1].store(1, Ordering::Release);

    let reader = TelemetryReader::new(&path).expect("open reader for same-buffer test");
    let result = reader.read();

    // Reader sees seq[1]=1 (odd), retries, fails -> correct behavior
    assert!(
        result.is_err(),
        "reader reading buffer 1 should fail when writer is mid-write to buffer 1"
    );
}

#[test]
fn double_buffer_writer_advances_header_state() {
    let tmp = TempDir::new().expect("create temp dir");
    let path = tmp.path().join("aura-ipc-header-state.dat");
    let mut expected = sample_archive();

    let mut shm = ShmHandle::new(&path).expect("create shm handle");
    shm.write(&mut expected).expect("write snapshot");
    shm.write(&mut expected).expect("write second snapshot");

    let file = OpenOptions::new()
        .read(true)
        .open(&path)
        .expect("open shm file for header validation");
    let mmap = unsafe {
        MmapOptions::new()
            .len(SHM_SIZE)
            .map(&file)
            .expect("map shm file")
    };

    let header = unsafe { &*(mmap.as_ptr() as *const DoubleBufferHeader) };
    assert_eq!(
        header.seq[0].load(Ordering::Acquire),
        2,
        "buffer 0: 0->1->2 after second write"
    );
    assert_eq!(
        header.seq[1].load(Ordering::Acquire),
        2,
        "buffer 1: 0->1->2 after first write"
    );
    assert_eq!(header.active_index.load(Ordering::Acquire), 0);
}

fn assert_archive_fields_equal(expected: &TelemetryArchive, actual: &TelemetryArchive) {
    assert_eq!(actual.version, expected.version);
    assert_eq!(actual.cpu.user_ticks, expected.cpu.user_ticks);
    assert_eq!(actual.cpu.system_ticks, expected.cpu.system_ticks);
    assert_eq!(actual.cpu.idle_ticks, expected.cpu.idle_ticks);
    assert_eq!(actual.cpu.total_ticks, expected.cpu.total_ticks);
    assert_eq!(actual.cpu.context_switches, expected.cpu.context_switches);
    assert_eq!(
        actual.cpu.context_switches_per_sec,
        expected.cpu.context_switches_per_sec
    );
    assert_eq!(actual.cpu.usage_percent, expected.cpu.usage_percent);
    assert_eq!(actual.cpu.core_count, expected.cpu.core_count);
    assert_eq!(
        actual.cpu.cores[0].core_index,
        expected.cpu.cores[0].core_index
    );
    assert_eq!(
        actual.cpu.cores[0].usage_percent,
        expected.cpu.cores[0].usage_percent
    );

    assert_eq!(actual.process.total, expected.process.total);
    assert_eq!(actual.process.running, expected.process.running);
    assert_eq!(actual.process.blocked, expected.process.blocked);
    assert_eq!(actual.process.sleeping, expected.process.sleeping);
    assert_eq!(
        actual.process.top_cpu[0].pid,
        expected.process.top_cpu[0].pid
    );
    assert_eq!(
        actual.process.top_cpu[0].memory_bytes,
        expected.process.top_cpu[0].memory_bytes
    );
    assert_eq!(
        actual.process.top_cpu[0].comm.bytes,
        expected.process.top_cpu[0].comm.bytes
    );

    assert_eq!(actual.memory.ram_total, expected.memory.ram_total);
    assert_eq!(actual.memory.ram_free, expected.memory.ram_free);
    assert_eq!(actual.memory.ram_used, expected.memory.ram_used);
    assert_eq!(actual.memory.page_faults, expected.memory.page_faults);
    assert_eq!(
        actual.memory.page_faults_per_sec,
        expected.memory.page_faults_per_sec
    );

    assert_eq!(actual.storage.disk_count, expected.storage.disk_count);
    assert_eq!(actual.storage.mount_count, expected.storage.mount_count);
    assert_eq!(
        actual.storage.disks[0].rx_bytes,
        expected.storage.disks[0].rx_bytes
    );
    assert_eq!(
        actual.storage.disks[0].wx_bytes,
        expected.storage.disks[0].wx_bytes
    );

    assert_eq!(actual.network.if_count, expected.network.if_count);
    assert_eq!(
        actual.network.interfaces[0].rx_bytes,
        expected.network.interfaces[0].rx_bytes
    );
    assert_eq!(
        actual.network.interfaces[0].tx_bytes,
        expected.network.interfaces[0].tx_bytes
    );

    assert_eq!(actual.meta.timestamp_ns, expected.meta.timestamp_ns);
    assert_eq!(actual.meta.uptime_secs, expected.meta.uptime_secs);
    assert_eq!(actual.meta.load_avg_1m, expected.meta.load_avg_1m);
    assert_eq!(actual.meta.load_avg_5m, expected.meta.load_avg_5m);
    assert_eq!(actual.meta.load_avg_15m, expected.meta.load_avg_15m);
    assert_eq!(actual.meta.timezone_name, expected.meta.timezone_name);
    assert_eq!(
        actual.meta.timezone_offset_secs,
        expected.meta.timezone_offset_secs
    );
    assert_eq!(actual.meta.os.os_type.bytes, expected.meta.os.os_type.bytes);
    assert_eq!(actual.meta.os.os_id.bytes, expected.meta.os.os_id.bytes);
    assert_eq!(
        actual.meta.os.os_version_id.bytes,
        expected.meta.os.os_version_id.bytes
    );
    assert_eq!(
        actual.meta.os.os_pretty_name,
        expected.meta.os.os_pretty_name
    );

    assert_eq!(actual.gpu.gpu_count, expected.gpu.gpu_count);
    assert_eq!(actual.gpu.nvml_available, expected.gpu.nvml_available);
    assert_eq!(
        actual.gpu.gpus[0].name.bytes,
        expected.gpu.gpus[0].name.bytes
    );
    assert_eq!(
        actual.gpu.gpus[0].memory_total,
        expected.gpu.gpus[0].memory_total
    );
    assert_eq!(
        actual.gpu.gpus[0].memory_used,
        expected.gpu.gpus[0].memory_used
    );
    assert_eq!(
        actual.gpu.gpus[0].utilization_percent,
        expected.gpu.gpus[0].utilization_percent
    );
    assert_eq!(
        actual.gpu.gpus[0].power_watts,
        expected.gpu.gpus[0].power_watts
    );
    assert_eq!(
        actual.gpu.gpus[0].temperature_celsius,
        expected.gpu.gpus[0].temperature_celsius
    );
    assert_eq!(actual.gpu.gpus[0].available, expected.gpu.gpus[0].available);

    assert_eq!(actual.checksum, expected.checksum);
}

fn sample_archive() -> TelemetryArchive {
    let mut t = TelemetryArchive::zeroed();

    t.version = 42;

    t.cpu.user_ticks = 101;
    t.cpu.system_ticks = 202;
    t.cpu.idle_ticks = 303;
    t.cpu.total_ticks = 606;
    t.cpu.context_switches = 777;
    t.cpu.context_switches_per_sec = 12.5;
    t.cpu.usage_percent = 66.6;
    t.cpu.core_count = 1;
    t.cpu.cores[0].core_index = 0;
    t.cpu.cores[0].user_ticks = 11;
    t.cpu.cores[0].system_ticks = 22;
    t.cpu.cores[0].idle_ticks = 33;
    t.cpu.cores[0].total_ticks = 66;
    t.cpu.cores[0].usage_percent = 50.5;

    t.process.total = 321;
    t.process.running = 12;
    t.process.blocked = 3;
    t.process.sleeping = 306;
    t.process.top_cpu[0].pid = 4242;
    t.process.top_cpu[0].cpu_usage = 39.9;
    t.process.top_cpu[0].memory_bytes = 9_876_543;
    t.process.top_cpu[0].comm = FixedString16::from_bytes(b"daemon-main");
    t.process.top_mem[0].pid = 4343;
    t.process.top_mem[0].cpu_usage = 10.1;
    t.process.top_mem[0].memory_bytes = 8_888_888;
    t.process.top_mem[0].comm = FixedString16::from_bytes(b"worker");

    t.memory.ram_total = 64 * 1024 * 1024 * 1024;
    t.memory.ram_free = 12 * 1024 * 1024 * 1024;
    t.memory.ram_used = t.memory.ram_total - t.memory.ram_free;
    t.memory.buffers = 1_024_000;
    t.memory.cached = 2_048_000;
    t.memory.swap_total = 8 * 1024 * 1024 * 1024;
    t.memory.swap_free = 7 * 1024 * 1024 * 1024;
    t.memory.swap_used = t.memory.swap_total - t.memory.swap_free;
    t.memory.page_faults = 123_456;
    t.memory.page_faults_per_sec = 9.75;

    t.storage.disk_count = 1;
    t.storage.disks[0].name = FixedString16::from_bytes(b"nvme0n1");
    t.storage.disks[0].rx_bytes = 55_000;
    t.storage.disks[0].wx_bytes = 77_000;
    t.storage.disks[0].rx_per_sec = 512.0;
    t.storage.disks[0].wx_per_sec = 768.0;
    t.storage.mount_count = 1;
    t.storage.mounts[0].mountpoint[0] = b'/';
    t.storage.mounts[0].fstype = FixedString16::from_bytes(b"ext4");
    t.storage.mounts[0].total = 1_000_000;
    t.storage.mounts[0].available = 250_000;
    t.storage.mounts[0].used = 750_000;
    t.storage.mounts[0].percent = 75.0;

    t.network.if_count = 1;
    t.network.interfaces[0].name = FixedString16::from_bytes(b"eth0");
    t.network.interfaces[0].rx_bytes = 1_234_567;
    t.network.interfaces[0].tx_bytes = 7_654_321;
    t.network.interfaces[0].rx_bytes_per_sec = 111.1;
    t.network.interfaces[0].tx_bytes_per_sec = 222.2;

    t.meta.timestamp_ns = 5_000_000_000;
    t.meta.uptime_secs = 17_000;
    t.meta.load_avg_1m = 1.1;
    t.meta.load_avg_5m = 0.9;
    t.meta.load_avg_15m = 0.7;
    t.meta.timezone_name = *b"UTC\0\0\0\0\0";
    t.meta.timezone_offset_secs = 0;
    t.meta.os.os_type = FixedString16::from_bytes(b"linux");
    t.meta.os.os_id = FixedString16::from_bytes(b"ubuntu");
    t.meta.os.os_version_id = FixedString16::from_bytes(b"24.04");
    let pretty = b"Ubuntu 24.04 LTS";
    t.meta.os.os_pretty_name[..pretty.len()].copy_from_slice(pretty);

    t.gpu.gpu_count = 1;
    t.gpu.nvml_available = 1;
    t.gpu.gpus[0].name = FixedString16::from_bytes(b"rtx4090");
    t.gpu.gpus[0].memory_total = 24 * 1024 * 1024 * 1024;
    t.gpu.gpus[0].memory_used = 6 * 1024 * 1024 * 1024;
    t.gpu.gpus[0].utilization_percent = 42.0;
    t.gpu.gpus[0].power_watts = 235.5;
    t.gpu.gpus[0].temperature_celsius = 61;
    t.gpu.gpus[0].available = 1;

    t
}
