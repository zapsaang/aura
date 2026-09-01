use super::{MAX_CORES, MAX_DISKS, MAX_GPUS, MAX_MOUNTS, MAX_TOP_N};

pub const CPU_OFFSET: usize = 16;
pub const PROCESS_OFFSET: usize = 6216;
pub const MEMORY_OFFSET: usize = 6560;
pub const STORAGE_OFFSET: usize = 6640;
pub const NETWORK_OFFSET: usize = 17536;
pub const GPU_OFFSET: usize = 18488;
pub const DERIVED_OFFSET: usize = 18944;
pub const RESERVED_OFFSET: usize = 18_972;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HiddenKind {
    ArchiveReserved,
    CpuCoreLeadingPadding,
    CpuCoreTrailingPadding,
    CpuPadding,
    DerivedReserved,
    DerivedPadding,
    ProcessPadding,
    ProcessTopMemoryUnownedCpu,
    MemoryPadding,
    StorageDiskPadding,
    StorageDiskHeaderPadding,
    StorageMountPadding,
    StorageMountTailPadding,
    NetworkPadding,
    GpuRecordPadding,
    GpuPadding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HiddenInterval {
    pub offset: usize,
    pub len: usize,
    pub kind: HiddenKind,
    pub index: usize,
}

pub const HIDDEN_INTERVAL_COUNT: usize = 2 * MAX_CORES
    + 1
    + 2
    + 1
    + MAX_TOP_N
    + 1
    + MAX_DISKS
    + 1
    + MAX_MOUNTS
    + 1
    + 1
    + MAX_GPUS
    + 1
    + 1;

const EMPTY: HiddenInterval = HiddenInterval {
    offset: 0,
    len: 0,
    kind: HiddenKind::ArchiveReserved,
    index: 0,
};

const fn build() -> [HiddenInterval; HIDDEN_INTERVAL_COUNT] {
    let mut out = [EMPTY; HIDDEN_INTERVAL_COUNT];
    let mut at = 0;
    let mut i = 0;
    while i < MAX_CORES {
        let base = CPU_OFFSET + 48 + i * 48;
        out[at] = HiddenInterval {
            offset: base + 1,
            len: 7,
            kind: HiddenKind::CpuCoreLeadingPadding,
            index: i,
        };
        at += 1;
        out[at] = HiddenInterval {
            offset: base + 44,
            len: 4,
            kind: HiddenKind::CpuCoreTrailingPadding,
            index: i,
        };
        at += 1;
        i += 1;
    }
    out[at] = HiddenInterval {
        offset: CPU_OFFSET + 6193,
        len: 7,
        kind: HiddenKind::CpuPadding,
        index: 0,
    };
    at += 1;
    i = 0;
    while i < MAX_TOP_N {
        out[at] = HiddenInterval {
            offset: PROCESS_OFFSET + 176 + i * 32 + 4,
            len: 4,
            kind: HiddenKind::ProcessTopMemoryUnownedCpu,
            index: i,
        };
        at += 1;
        i += 1;
    }
    out[at] = HiddenInterval {
        offset: PROCESS_OFFSET + 339,
        len: 5,
        kind: HiddenKind::ProcessPadding,
        index: 0,
    };
    at += 1;
    out[at] = HiddenInterval {
        offset: MEMORY_OFFSET + 76,
        len: 4,
        kind: HiddenKind::MemoryPadding,
        index: 0,
    };
    at += 1;
    i = 0;
    while i < MAX_DISKS {
        out[at] = HiddenInterval {
            offset: STORAGE_OFFSET + i * 72 + 68,
            len: 4,
            kind: HiddenKind::StorageDiskPadding,
            index: i,
        };
        at += 1;
        i += 1;
    }
    out[at] = HiddenInterval {
        offset: STORAGE_OFFSET + 1154,
        len: 6,
        kind: HiddenKind::StorageDiskHeaderPadding,
        index: 0,
    };
    at += 1;
    i = 0;
    while i < MAX_MOUNTS {
        out[at] = HiddenInterval {
            offset: STORAGE_OFFSET + 1160 + i * 304 + 300,
            len: 4,
            kind: HiddenKind::StorageMountPadding,
            index: i,
        };
        at += 1;
        i += 1;
    }
    out[at] = HiddenInterval {
        offset: STORAGE_OFFSET + 10891,
        len: 5,
        kind: HiddenKind::StorageMountTailPadding,
        index: 0,
    };
    at += 1;
    out[at] = HiddenInterval {
        offset: NETWORK_OFFSET + 642,
        len: 6,
        kind: HiddenKind::NetworkPadding,
        index: 0,
    };
    at += 1;
    i = 0;
    while i < MAX_GPUS {
        out[at] = HiddenInterval {
            offset: GPU_OFFSET + i * 56 + 44,
            len: 4,
            kind: HiddenKind::GpuRecordPadding,
            index: i,
        };
        at += 1;
        i += 1;
    }
    out[at] = HiddenInterval {
        offset: GPU_OFFSET + 451,
        len: 5,
        kind: HiddenKind::GpuPadding,
        index: 0,
    };
    at += 1;
    out[at] = HiddenInterval {
        offset: DERIVED_OFFSET + 19,
        len: 1,
        kind: HiddenKind::DerivedReserved,
        index: 0,
    };
    at += 1;
    out[at] = HiddenInterval {
        offset: DERIVED_OFFSET + 20,
        len: 4,
        kind: HiddenKind::DerivedPadding,
        index: 0,
    };
    at += 1;
    out[at] = HiddenInterval {
        offset: RESERVED_OFFSET,
        len: 46_564,
        kind: HiddenKind::ArchiveReserved,
        index: 0,
    };
    out
}

static HIDDEN_INTERVALS: [HiddenInterval; HIDDEN_INTERVAL_COUNT] = build();

pub fn hidden_intervals() -> &'static [HiddenInterval] {
    &HIDDEN_INTERVALS
}

pub fn hidden_interval_path(interval: &HiddenInterval) -> String {
    let i = interval.index;
    match interval.kind {
        HiddenKind::ArchiveReserved => "archive.reserved".to_string(),
        HiddenKind::CpuCoreLeadingPadding => format!("cpu.cores[{i}].leading_padding"),
        HiddenKind::CpuCoreTrailingPadding => format!("cpu.cores[{i}].trailing_padding"),
        HiddenKind::CpuPadding => "cpu.padding".to_string(),
        HiddenKind::DerivedReserved => "derived.reserved".to_string(),
        HiddenKind::DerivedPadding => "derived.padding".to_string(),
        HiddenKind::ProcessPadding => "process.padding".to_string(),
        HiddenKind::ProcessTopMemoryUnownedCpu => {
            format!("process.top_memory[{i}].unowned_cpu_usage")
        }
        HiddenKind::MemoryPadding => "memory.padding".to_string(),
        HiddenKind::StorageDiskPadding => format!("storage.disks[{i}].padding"),
        HiddenKind::StorageDiskHeaderPadding => "storage.disk_header_padding".to_string(),
        HiddenKind::StorageMountPadding => format!("storage.mounts[{i}].padding"),
        HiddenKind::StorageMountTailPadding => "storage.mount_tail_padding".to_string(),
        HiddenKind::NetworkPadding => "network.padding".to_string(),
        HiddenKind::GpuRecordPadding => format!("gpu.gpus[{i}].padding"),
        HiddenKind::GpuPadding => "gpu.padding".to_string(),
    }
}

pub fn hidden_interval_paths() -> Vec<String> {
    hidden_intervals()
        .iter()
        .map(hidden_interval_path)
        .collect()
}
