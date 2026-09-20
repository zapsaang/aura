use aura_common::{
    bytes_to_string, TelemetryArchive, CAP_GPU_ENUMERATION, CAP_META_LOAD_AVERAGE,
    CAP_META_OS_CODENAME, CAP_META_OS_IDENTITY, CAP_META_OS_VERSION, CAP_META_OS_VERSION_ID,
    CAP_META_TIMEZONE, CAP_META_UPTIME, CAP_META_WALLCLOCK, CAP_NETWORK_BYTES, CAP_NETWORK_RATES,
    CAP_STORAGE_MOUNTS, GPU_CAP_MEMORY_TOTAL, GPU_CAP_MEMORY_USED, GPU_CAP_NAME, GPU_CAP_POWER,
    GPU_CAP_TEMPERATURE, GPU_CAP_UTILIZATION,
};

use super::convert::{opt, DISK_CAPS};
use super::schema::*;
use crate::output::color::tone_str;

pub(super) fn storage_json(t: &TelemetryArchive) -> StorageJson {
    let caps = t.capabilities;
    let s = &t.storage;
    let disks_owned = caps & DISK_CAPS != 0;
    StorageJson {
        disk_truncated: opt(disks_owned, s.disk_truncated != 0),
        mount_truncated: opt(caps & CAP_STORAGE_MOUNTS != 0, s.mount_truncated != 0),
        disks: opt(
            disks_owned,
            (0..s.disk_count as usize)
                .map(|i| {
                    let d = &s.disks[i];
                    DiskJson {
                        name: opt(
                            caps & aura_common::CAP_STORAGE_DISK_BYTES != 0,
                            d.name.as_str().to_string(),
                        ),
                        major: opt(caps & aura_common::CAP_STORAGE_DISK_BYTES != 0, d.major),
                        minor: opt(caps & aura_common::CAP_STORAGE_DISK_BYTES != 0, d.minor),
                        read_bytes: opt(
                            caps & aura_common::CAP_STORAGE_DISK_BYTES != 0,
                            d.read_bytes,
                        ),
                        write_bytes: opt(
                            caps & aura_common::CAP_STORAGE_DISK_BYTES != 0,
                            d.write_bytes,
                        ),
                        read_bytes_per_sec: opt(
                            caps & aura_common::CAP_STORAGE_DISK_RATES != 0,
                            d.read_bytes_per_sec,
                        ),
                        write_bytes_per_sec: opt(
                            caps & aura_common::CAP_STORAGE_DISK_RATES != 0,
                            d.write_bytes_per_sec,
                        ),
                        read_iops: opt(caps & aura_common::CAP_STORAGE_DISK_IOPS != 0, d.read_iops),
                        write_iops: opt(
                            caps & aura_common::CAP_STORAGE_DISK_IOPS != 0,
                            d.write_iops,
                        ),
                        queue_depth: opt(
                            caps & aura_common::CAP_STORAGE_DISK_QUEUE_DEPTH != 0,
                            d.queue_depth,
                        ),
                        read_latency_ms: opt(
                            caps & aura_common::CAP_STORAGE_DISK_LATENCY != 0,
                            d.read_latency_ms,
                        ),
                        write_latency_ms: opt(
                            caps & aura_common::CAP_STORAGE_DISK_LATENCY != 0,
                            d.write_latency_ms,
                        ),
                    }
                })
                .collect(),
        ),
        mounts: opt(
            caps & CAP_STORAGE_MOUNTS != 0,
            (0..s.mount_count as usize)
                .map(|i| {
                    let m = &s.mounts[i];
                    MountJson {
                        mountpoint: bytes_to_string(&m.mountpoint),
                        fstype: m.fstype.as_str().to_string(),
                        total: m.total,
                        available: m.available,
                        used: m.used,
                        percent: m.percent,
                    }
                })
                .collect(),
        ),
    }
}

pub(super) fn network_json(t: &TelemetryArchive) -> NetworkJson {
    let caps = t.capabilities;
    let n = &t.network;
    let bytes = caps & CAP_NETWORK_BYTES != 0;
    let rates = caps & CAP_NETWORK_RATES != 0;
    NetworkJson {
        truncated: opt(bytes || rates, n.truncated != 0),
        aggregate_rx_bytes_per_sec: opt(bytes && rates, t.derived.aggregate_rx_bytes_per_sec),
        aggregate_tx_bytes_per_sec: opt(bytes && rates, t.derived.aggregate_tx_bytes_per_sec),
        interfaces: opt(
            bytes || rates,
            (0..n.if_count as usize)
                .map(|i| {
                    let f = &n.interfaces[i];
                    NetIfJson {
                        name: opt(bytes, f.name.as_str().to_string()),
                        rx_bytes: opt(bytes, f.rx_bytes),
                        tx_bytes: opt(bytes, f.tx_bytes),
                        rx_bytes_per_sec: opt(rates, f.rx_bytes_per_sec),
                        tx_bytes_per_sec: opt(rates, f.tx_bytes_per_sec),
                    }
                })
                .collect(),
        ),
    }
}

pub(super) fn meta_json(t: &TelemetryArchive) -> MetaJson {
    let caps = t.capabilities;
    let m = &t.meta;
    let tz = caps & CAP_META_TIMEZONE != 0;
    let id = caps & CAP_META_OS_IDENTITY != 0;
    let load = caps & CAP_META_LOAD_AVERAGE != 0;
    MetaJson {
        timestamp_ns: m.timestamp_ns,
        wallclock_ns: opt(caps & CAP_META_WALLCLOCK != 0, m.wallclock_ns),
        uptime_secs: opt(caps & CAP_META_UPTIME != 0, m.uptime_secs),
        load_avg_1m: opt(load, m.load_avg_1m),
        load_avg_5m: opt(load, m.load_avg_5m),
        load_avg_15m: opt(load, m.load_avg_15m),
        timezone_name: opt(tz, bytes_to_string(&m.timezone_name)),
        timezone_offset_secs: opt(tz, m.timezone_offset_secs),
        os: OsJson {
            os_type: opt(id, m.os.os_type.as_str().to_string()),
            os_id: opt(id, m.os.os_id.as_str().to_string()),
            version: opt(
                caps & CAP_META_OS_VERSION != 0,
                bytes_to_string(&m.os.version),
            ),
            version_id: opt(
                caps & CAP_META_OS_VERSION_ID != 0,
                m.os.os_version_id.as_str().to_string(),
            ),
            version_codename: opt(
                caps & CAP_META_OS_CODENAME != 0,
                m.os.version_codename.as_str().to_string(),
            ),
            pretty_name: opt(id, bytes_to_string(&m.os.os_pretty_name)),
        },
    }
}

pub(super) fn gpu_json(t: &TelemetryArchive) -> GpuJson {
    let g = &t.gpu;
    let owned = t.capabilities & CAP_GPU_ENUMERATION != 0;
    GpuJson {
        nvml_available: opt(owned, g.nvml_available != 0),
        gpu_truncated: opt(owned, g.truncated != 0),
        gpus: opt(
            owned,
            (0..g.gpu_count as usize)
                .map(|i| {
                    let r = &g.gpus[i];
                    let c = r.capabilities;
                    let temp = c & GPU_CAP_TEMPERATURE != 0;
                    GpuRecordJson {
                        available: r.available != 0,
                        name: opt(c & GPU_CAP_NAME != 0, r.name.as_str().to_string()),
                        memory_total: opt(c & GPU_CAP_MEMORY_TOTAL != 0, r.memory_total),
                        memory_used: opt(c & GPU_CAP_MEMORY_USED != 0, r.memory_used),
                        utilization_percent: opt(
                            c & GPU_CAP_UTILIZATION != 0,
                            r.utilization_percent,
                        ),
                        power_watts: opt(c & GPU_CAP_POWER != 0, r.power_watts),
                        temperature_celsius: opt(temp, r.temperature_celsius),
                        tone: opt(temp, tone_str(r.tone)),
                        capabilities: GpuRecordCapsJson {
                            name: c & GPU_CAP_NAME != 0,
                            memory_total: c & GPU_CAP_MEMORY_TOTAL != 0,
                            memory_used: c & GPU_CAP_MEMORY_USED != 0,
                            utilization: c & GPU_CAP_UTILIZATION != 0,
                            power: c & GPU_CAP_POWER != 0,
                            temperature: temp,
                        },
                    }
                })
                .collect(),
        ),
    }
}
