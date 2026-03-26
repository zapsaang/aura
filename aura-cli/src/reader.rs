use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use aura_common::{
    monotonic_ns, read_double_buffer, AuraError, AuraResult, TelemetryArchive, SHM_FILE_MODE,
    SHM_SIZE,
};
use memmap2::{Mmap, MmapOptions};

pub struct TelemetryReader {
    mmap: Mmap,
    path: PathBuf,
}

impl TelemetryReader {
    pub fn new(path: &Path) -> AuraResult<Self> {
        let file = OpenOptions::new().read(true).open(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                AuraError::MmapFailed(format!(
                    "Permission denied reading {}: check file permissions (expected 0o{SHM_FILE_MODE:o})",
                    path.display()
                ))
            } else {
                AuraError::SharedMemory(e)
            }
        })?;

        let meta = file.metadata()?;
        if meta.len() != SHM_SIZE as u64 {
            return Err(AuraError::MmapFailed(format!(
                "incompatible shm size {} (expected {})",
                meta.len(),
                SHM_SIZE
            )));
        }

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

    pub fn read(&self) -> AuraResult<TelemetryArchive> {
        let mut snapshot = unsafe {
            read_double_buffer(self.mmap.as_ptr()).map_err(|()| AuraError::SeqLockInvalid)?
        };

        let expected = snapshot.checksum;
        snapshot.checksum = 0;
        let actual = snapshot.calculate_checksum();
        snapshot.checksum = expected;

        if expected != actual {
            return Err(AuraError::ChecksumMismatch { expected, actual });
        }

        Ok(snapshot)
    }

    pub fn is_fresh(&self, telemetry: &TelemetryArchive, threshold: Duration) -> bool {
        if telemetry.meta.timestamp_ns > 0 {
            let now = monotonic_ns();
            let age_ns = now.saturating_sub(telemetry.meta.timestamp_ns);
            let threshold_ns = threshold.as_nanos() as u64;
            if age_ns <= threshold_ns {
                return true;
            }
        }

        self.file_is_fresh(threshold)
    }

    fn file_is_fresh(&self, threshold: Duration) -> bool {
        let Ok(metadata) = std::fs::metadata(&self.path) else {
            return false;
        };
        let Ok(modified) = metadata.modified() else {
            return false;
        };
        let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
            return true;
        };
        elapsed <= threshold
    }
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use aura_common::{
        write_double_buffer, AuraError, CpuCoreStat, CpuGlobalStat, DoubleBufferHeader,
        FixedString16, GpuStat, GpuStats, MemoryStats, MetaStats, NetIfStat, NetworkStats,
        OsFingerprint, ProcessStat, ProcessStats, StorageStats, TelemetryArchive, BUFFER_0_OFFSET,
        BUFFER_1_OFFSET, MAX_CORES, MAX_DISKS, MAX_MOUNTS, MAX_NETIFS, MAX_TOP_N, SHM_SIZE,
    };
    use memmap2::MmapOptions;

    use super::TelemetryReader;

    #[test]
    fn read_returns_snapshot_from_active_buffer() {
        let path = temp_shm_path("stable");
        let mut mmap = init_shm_file(&path);
        let telemetry = sample_telemetry(44.5);
        write_snapshot(&mut mmap, &telemetry);

        let reader = TelemetryReader::new(&path).unwrap();
        let out = reader.read().unwrap();

        assert_eq!(out.cpu.usage_percent, 44.5);
        cleanup(&path);
    }

    #[test]
    fn read_returns_checksum_mismatch_for_corrupt_active_buffer() {
        let path = temp_shm_path("checksum");
        let mut mmap = init_shm_file(&path);
        write_snapshot(&mut mmap, &sample_telemetry(10.0));

        let base = mmap.as_mut_ptr();
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
        unsafe {
            let checksum_ptr = base
                .add(active_offset + std::mem::offset_of!(TelemetryArchive, checksum))
                .cast::<u32>();
            *checksum_ptr = 0;
        }
        mmap.flush().unwrap();

        let reader = TelemetryReader::new(&path).unwrap();
        match reader.read() {
            Ok(_) => panic!("expected checksum mismatch"),
            Err(err) => assert!(matches!(err, AuraError::ChecksumMismatch { .. })),
        }
        cleanup(&path);
    }

    #[test]
    fn freshness_uses_file_mtime_fallback() {
        let path = temp_shm_path("fresh");
        let mut mmap = init_shm_file(&path);
        write_snapshot(&mut mmap, &sample_telemetry(20.0));

        let reader = TelemetryReader::new(&path).unwrap();
        let stale = sample_telemetry(20.0);

        assert!(reader.is_fresh(&stale, Duration::from_secs(2)));
        cleanup(&path);
    }

    fn temp_shm_path(suffix: &str) -> std::path::PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("aura-cli-{suffix}-{ts}.dat"))
    }

    fn init_shm_file(path: &std::path::Path) -> memmap2::MmapMut {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap();
        file.set_len(SHM_SIZE as u64).unwrap();
        unsafe { MmapOptions::new().len(SHM_SIZE).map_mut(&file).unwrap() }
    }

    fn write_snapshot(mmap: &mut memmap2::MmapMut, telemetry: &TelemetryArchive) {
        let mut t = *telemetry;
        t.checksum = t.calculate_checksum();
        unsafe {
            write_double_buffer(mmap.as_mut_ptr(), &t);
        }
        mmap.flush().unwrap();
    }

    fn cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
    }

    fn sample_telemetry(cpu_usage: f32) -> TelemetryArchive {
        let mut t = unsafe { std::mem::zeroed::<TelemetryArchive>() };
        t.version = 1;
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
                usage_percent: cpu_usage,
                _pad1: [0; 4],
            }; MAX_CORES],
            core_count: 1,
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
        };
        t.memory = MemoryStats {
            ram_total: 1,
            ram_free: 1,
            ram_used: 0,
            buffers: 0,
            cached: 0,
            swap_total: 0,
            swap_free: 0,
            swap_used: 0,
            page_faults: 0,
            page_faults_per_sec: 0.0,
            _pad0: [0; 4],
        };
        t.storage = StorageStats {
            disks: [unsafe { std::mem::zeroed() }; MAX_DISKS],
            disk_count: 0,
            _pad0: [0; 7],
            mounts: [unsafe { std::mem::zeroed() }; MAX_MOUNTS],
            mount_count: 0,
            _pad1: [0; 6],
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
            _pad0: [0; 7],
        };
        t.meta = MetaStats {
            timestamp_ns: 0,
            uptime_secs: 0,
            load_avg_1m: 0.0,
            load_avg_5m: 0.0,
            load_avg_15m: 0.0,
            timezone_name: [0; 8],
            timezone_offset_secs: 0,
            os: OsFingerprint {
                os_type: FixedString16::new(),
                os_id: FixedString16::new(),
                os_version_id: FixedString16::new(),
                os_pretty_name: [0; 128],
            },
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
                _pad0: [0; 5],
            }; 8],
            gpu_count: 0,
            nvml_available: 0,
            _pad0: [0; 6],
        };
        t
    }
}
