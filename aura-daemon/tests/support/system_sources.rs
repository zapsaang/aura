use aura_common::{
    AuraResult, FixedString16, CAP_CPU_CONTEXT_SWITCHES, CAP_CPU_GLOBAL, CAP_CPU_PER_CORE,
    CAP_GPU_ENUMERATION, CAP_MEMORY_BUFFERS, CAP_MEMORY_CACHED, CAP_MEMORY_PAGE_FAULTS,
    CAP_MEMORY_RAM_FREE, CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP,
    CAP_META_LOAD_AVERAGE, CAP_META_OS_IDENTITY, CAP_META_OS_VERSION_ID, CAP_META_TIMEZONE,
    CAP_META_UPTIME, CAP_NETWORK_BYTES, CAP_NETWORK_RATES, CAP_PROCESS_BLOCKED,
    CAP_PROCESS_RUNNING, CAP_PROCESS_SLEEPING, CAP_PROCESS_TOP_CPU, CAP_PROCESS_TOP_MEMORY,
    CAP_PROCESS_TOTAL, MAX_NETIFS,
};
use aura_daemon::collectors::cpu::CpuAvailability;
use aura_daemon::collectors::memory::MemoryAvailability;
use aura_daemon::collectors::process::ProcessAvailability;
use aura_daemon::collectors::{
    CollectorScratch, CollectorSources, FixedCollectorState, MetaGpuAvailability,
    NetworkAvailability,
};

pub const SOURCE_CAPABILITIES: u64 = CAP_CPU_GLOBAL
    | CAP_CPU_PER_CORE
    | CAP_CPU_CONTEXT_SWITCHES
    | CAP_MEMORY_RAM_TOTAL
    | CAP_MEMORY_RAM_FREE
    | CAP_MEMORY_RAM_USED
    | CAP_MEMORY_BUFFERS
    | CAP_MEMORY_CACHED
    | CAP_MEMORY_SWAP
    | CAP_MEMORY_PAGE_FAULTS
    | CAP_NETWORK_BYTES
    | CAP_NETWORK_RATES
    | CAP_PROCESS_TOTAL
    | CAP_PROCESS_RUNNING
    | CAP_PROCESS_BLOCKED
    | CAP_PROCESS_SLEEPING
    | CAP_PROCESS_TOP_CPU
    | CAP_PROCESS_TOP_MEMORY
    | CAP_META_UPTIME
    | CAP_META_LOAD_AVERAGE
    | CAP_META_TIMEZONE
    | CAP_META_OS_IDENTITY
    | CAP_META_OS_VERSION_ID
    | CAP_GPU_ENUMERATION;

#[derive(Default)]
pub struct DeterministicSources {
    pub calls: [usize; 5],
    pub over_capacity_cpu_fixture: bool,
    sample: u64,
}

impl CollectorSources for DeterministicSources {
    fn collect_cpu(
        &mut self,
        state: &mut FixedCollectorState,
        _scratch: &mut CollectorScratch,
    ) -> AuraResult<CpuAvailability> {
        self.calls[0] += 1;
        if self.over_capacity_cpu_fixture {
            return aura_daemon::collectors::cpu::linux::collect_from_bytes(
                include_bytes!("../fixtures/proc_stat_129_cores.txt"),
                &mut state.archive.cpu,
            );
        }
        let step = self.sample * 10;
        state.archive.cpu.user_ticks = 100 + step;
        state.archive.cpu.system_ticks = 50 + step;
        state.archive.cpu.idle_ticks = 150 + step;
        state.archive.cpu.total_ticks = 300 + step * 3;
        state.archive.cpu.context_switches = 1_000 + step;
        state.archive.cpu.core_count = 1;
        state.archive.cpu.cores[0].core_index = 0;
        state.archive.cpu.cores[0].user_ticks = 100 + step;
        state.archive.cpu.cores[0].system_ticks = 50 + step;
        state.archive.cpu.cores[0].idle_ticks = 150 + step;
        state.archive.cpu.cores[0].total_ticks = 300 + step * 3;
        Ok(CpuAvailability {
            context_switches: true,
            over_capacity: false,
        })
    }

    fn collect_memory(
        &mut self,
        state: &mut FixedCollectorState,
        _scratch: &mut CollectorScratch,
    ) -> AuraResult<MemoryAvailability> {
        self.calls[1] += 1;
        state.archive.memory.ram_total = 16_000;
        state.archive.memory.ram_free = 6_000;
        state.archive.memory.ram_used = 10_000;
        state.archive.memory.buffers = 500;
        state.archive.memory.cached = 2_000;
        state.archive.memory.swap_total = 4_000;
        state.archive.memory.swap_free = 3_000;
        state.archive.memory.swap_used = 1_000;
        state.archive.memory.page_faults = 2_000 + self.sample * 10;
        Ok(MemoryAvailability {
            buffers: true,
            cached: true,
            swap: true,
            page_faults: true,
        })
    }

    fn collect_network(
        &mut self,
        state: &mut FixedCollectorState,
        _scratch: &mut CollectorScratch,
    ) -> AuraResult<NetworkAvailability> {
        self.calls[2] += 1;
        state.archive.network.if_count = MAX_NETIFS as u8;
        for (index, interface) in state.archive.network.interfaces.iter_mut().enumerate() {
            interface.name = FixedString16::from_bytes(b"ethernet");
            interface.rx_bytes = 10_000 + self.sample * 100 + index as u64;
            interface.tx_bytes = 20_000 + self.sample * 100 + index as u64;
        }
        Ok(NetworkAvailability {
            bytes: true,
            rates: true,
        })
    }

    fn collect_process(
        &mut self,
        state: &mut FixedCollectorState,
        _scratch: &mut CollectorScratch,
    ) -> AuraResult<ProcessAvailability> {
        self.calls[4] += 1;
        state.archive.process.total = 42;
        state.archive.process.running = 40;
        state.archive.process.blocked = 1;
        state.archive.process.sleeping = 1;
        if self.over_capacity_cpu_fixture {
            state.archive.process.top_cpu_count = 1;
            state.archive.process.top_cpu[0].pid = 7;
            state.archive.process.top_cpu[0].cpu_usage = 50.0;
            state.archive.process.top_cpu[0].comm = FixedString16::from_bytes(b"hot");
        }
        Ok(ProcessAvailability {
            running: true,
            total: true,
        })
    }

    fn collect_meta_and_gpu(
        &mut self,
        state: &mut FixedCollectorState,
    ) -> AuraResult<MetaGpuAvailability> {
        self.calls[3] += 1;
        state.archive.meta.uptime_secs = 100 + self.sample;
        state.archive.meta.load_avg_1m = 1.0;
        state.archive.meta.timezone_name = *b"UTC\0\0\0\0\0";
        state.archive.meta.os.os_type = FixedString16::from_bytes(b"linux");
        state.archive.meta.os.os_id = FixedString16::from_bytes(b"test");
        state.archive.meta.os.os_pretty_name[..10].copy_from_slice(b"Test Linux");
        state.archive.meta.os.os_version_id = FixedString16::from_bytes(b"1");
        state.archive.gpu.nvml_available = 1;
        state.archive.gpu.gpu_count = 1;
        state.archive.gpu.gpus[0].available = 1;
        self.sample += 1;
        Ok(MetaGpuAvailability {
            uptime: true,
            load_average: true,
            timezone: true,
            os_identity: true,
            os_version_id: true,
            gpu_enumeration: true,
        })
    }
}
