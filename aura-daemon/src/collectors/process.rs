use std::collections::HashMap;
use std::fs::{read_dir, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use aura_common::{system_page_size, AuraError, AuraResult, FixedString16, ProcessStat};

use super::heap::{HeapEntry, MinHeap5};
use super::parsing::{parse_u64, split_whitespace, trim_ascii};
use super::CollectorState;

const MAX_FD_CACHE_SIZE: usize = 256;

fn parse_proc_stat(buf: &[u8]) -> AuraResult<(u32, FixedString16, u64, u64, u64, u8)> {
    let open = buf
        .iter()
        .position(|&c| c == b'(')
        .ok_or_else(|| AuraError::ParseError("proc stat missing (".to_string()))?;
    let close = buf
        .iter()
        .rposition(|&c| c == b')')
        .ok_or_else(|| AuraError::ParseError("proc stat missing )".to_string()))?;

    let pid = parse_u64(trim_ascii(&buf[..open]))? as u32;
    let comm = FixedString16::from_bytes(&buf[open + 1..close]);

    if close + 2 >= buf.len() {
        return Err(AuraError::ParseError("proc stat too short".to_string()));
    }
    let state = buf[close + 2];

    let mut fields = [0u64; 32];
    let mut count = 0usize;
    for tok in split_whitespace(&buf[close + 3..]) {
        if count >= fields.len() {
            break;
        }
        fields[count] = parse_u64(tok).unwrap_or(0);
        count += 1;
    }

    let utime = if count > 10 { fields[10] } else { 0 };
    let stime = if count > 11 { fields[11] } else { 0 };
    let rss_pages = if count > 20 { fields[20] } else { 0 };

    Ok((
        pid,
        comm,
        utime,
        stime,
        rss_pages.saturating_mul(system_page_size() as u64),
        state,
    ))
}

pub struct ProcFdCache {
    cache: HashMap<u32, (File, u64)>,
    proc_root: PathBuf,
    access_counter: u64,
}

impl ProcFdCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            proc_root: PathBuf::from("/proc"),
            access_counter: 0,
        }
    }

    fn get_or_open(&mut self, pid: u32) -> Option<&mut File> {
        if self.cache.contains_key(&pid) {
            self.cache.get_mut(&pid).map(|e| {
                e.1 = self.access_counter;
                self.access_counter += 1;
                &mut e.0
            })
        } else {
            if self.cache.len() >= MAX_FD_CACHE_SIZE {
                self.evict_lru();
            }
            let stat_path = self.proc_root.join(pid.to_string()).join("stat");
            let file = match File::open(stat_path) {
                Ok(f) => f,
                Err(_) => return None,
            };
            let counter = self.access_counter;
            self.access_counter += 1;
            self.cache.insert(pid, (file, counter));
            self.cache.get_mut(&pid).map(|e| &mut e.0)
        }
    }

    fn evict_lru(&mut self) {
        if let Some((&pid, _)) = self.cache.iter().min_by_key(|(_, (_, c))| *c) {
            self.cache.remove(&pid);
        }
    }

    fn prune<F>(&mut self, is_active: F)
    where
        F: Fn(&u32) -> bool,
    {
        self.cache.retain(|pid, _| is_active(pid));
    }
}

impl Default for ProcFdCache {
    fn default() -> Self {
        Self::new()
    }
}

pub fn collect_top_n(
    state: &mut CollectorState,
    current_total_ticks: u64,
    core_count: u8,
) -> AuraResult<()> {
    let mut total = 0u32;
    let mut running = 0u32;
    let mut blocked = 0u32;
    let mut sleeping = 0u32;
    let mut cpu_heap = MinHeap5::new();
    let mut mem_heap = MinHeap5::new();

    state.current_proc_ticks.clear();
    state.active_pids.clear();

    let global_delta = current_total_ticks.saturating_sub(state.prev_proc_total_ticks);
    let ncores = core_count.max(1) as f64;

    for entry in read_dir("/proc")? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let name = entry.file_name();
        let pid = match parse_u64(name.as_encoded_bytes()) {
            Ok(v) if v > 0 => match u32::try_from(v) {
                Ok(pid) => pid,
                Err(_) => continue,
            },
            _ => continue,
        };

        state.active_pids.insert(pid, ());

        let file = match state.proc_fd_cache.get_or_open(pid) {
            Some(f) => f,
            None => continue,
        };

        if file.seek(SeekFrom::Start(0)).is_err() {
            continue;
        }

        state.proc_buffer.clear();
        if file.read_to_end(&mut state.proc_buffer).is_err() {
            continue;
        }

        let (proc_pid, comm, utime, stime, rss, proc_state) =
            match parse_proc_stat(&state.proc_buffer) {
                Ok(v) => v,
                Err(_) => continue,
            };

        total = total.saturating_add(1);
        match proc_state {
            b'R' => running = running.saturating_add(1),
            b'D' => blocked = blocked.saturating_add(1),
            b'S' | b'I' => sleeping = sleeping.saturating_add(1),
            _ => {}
        }

        let curr_ticks = utime.saturating_add(stime);
        let prev_ticks = state.prev_proc_ticks.get(&pid).copied().unwrap_or(0);
        let delta_proc = curr_ticks.saturating_sub(prev_ticks);
        state.current_proc_ticks.insert(pid, curr_ticks);

        let cpu_percent = if global_delta > 0 {
            ((delta_proc as f64 * ncores / global_delta as f64) * 100.0) as f32
        } else {
            0.0
        };

        let cpu_stat = ProcessStat {
            pid: proc_pid,
            cpu_usage: cpu_percent,
            memory_bytes: rss,
            comm,
        };

        cpu_heap.push(HeapEntry::new(delta_proc, cpu_stat));

        let mem_stat = ProcessStat {
            pid: proc_pid,
            cpu_usage: cpu_percent,
            memory_bytes: rss,
            comm,
        };
        mem_heap.push(HeapEntry::new(rss, mem_stat));
    }

    state
        .proc_fd_cache
        .prune(|pid| state.active_pids.contains_key(pid));

    std::mem::swap(&mut state.prev_proc_ticks, &mut state.current_proc_ticks);
    state.prev_proc_total_ticks = current_total_ticks;
    state.maybe_shrink_maps();

    state.telemetry.process.total = total;
    state.telemetry.process.running = running;
    state.telemetry.process.blocked = blocked;
    state.telemetry.process.sleeping = sleeping;
    state.telemetry.process.top_cpu = cpu_heap.as_desc_array();
    state.telemetry.process.top_mem = mem_heap.as_desc_array();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_proc_stat;

    #[test]
    fn parse_proc_stat_sample() {
        let fixture = include_bytes!("../../tests/fixtures/proc_pid_stat_sample.txt");
        let (pid, comm, utime, stime, rss, state) = parse_proc_stat(fixture).expect("parse");
        assert_eq!(pid, 12345);
        assert_eq!(comm.as_str(), "my process");
        assert_eq!(state, b'R');
        assert_eq!(utime, 100);
        assert_eq!(stime, 20);
        assert_eq!(rss, aura_common::system_page_size() as u64 * 200);
    }
}
