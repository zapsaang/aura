use std::collections::{HashMap, HashSet};
use std::fs::{read_dir, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use aura_common::{
    system_page_size, AuraError, AuraResult, FixedString16, ProcessStat, ProcessStats,
};

use super::heap::{HeapEntry, MinHeap5};
use super::parsing::{parse_u64, split_whitespace, trim_ascii};

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
    cache: HashMap<u32, File>,
    proc_root: PathBuf,
}

impl ProcFdCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            proc_root: PathBuf::from("/proc"),
        }
    }

    fn get_or_open(&mut self, pid: u32) -> Option<&mut File> {
        if self.cache.contains_key(&pid) {
            return self.cache.get_mut(&pid);
        }

        let stat_path = self.proc_root.join(pid.to_string()).join("stat");
        match File::open(stat_path) {
            Ok(file) => {
                self.cache.insert(pid, file);
                self.cache.get_mut(&pid)
            }
            Err(_) => None,
        }
    }

    fn prune(&mut self, active_pids: &HashSet<u32>) {
        self.cache.retain(|pid, _| active_pids.contains(pid));
    }
}

impl Default for ProcFdCache {
    fn default() -> Self {
        Self::new()
    }
}

pub fn collect_top_n(
    buf: &mut [u8; aura_common::PROC_BUFFER_SIZE],
    out: &mut ProcessStats,
    prev_proc_ticks: &mut HashMap<u32, u64>,
    prev_total_ticks: &mut u64,
    current_total_ticks: u64,
    core_count: u8,
    proc_cache: &mut ProcFdCache,
) -> AuraResult<()> {
    let mut total = 0u32;
    let mut running = 0u32;
    let mut blocked = 0u32;
    let mut sleeping = 0u32;
    let mut cpu_heap = MinHeap5::new();
    let mut mem_heap = MinHeap5::new();

    let mut current_proc_ticks = HashMap::with_capacity(prev_proc_ticks.len());
    let mut active_pids = HashSet::with_capacity(prev_proc_ticks.len());

    let global_delta = current_total_ticks.saturating_sub(*prev_total_ticks);
    let ncores = core_count.max(1) as f32;

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

        active_pids.insert(pid);

        let file = match proc_cache.get_or_open(pid) {
            Some(f) => f,
            None => continue,
        };

        if file.seek(SeekFrom::Start(0)).is_err() {
            continue;
        }

        let n = match file.read(buf) {
            Ok(n) => n,
            Err(_) => continue,
        };

        let (proc_pid, comm, utime, stime, rss, state) = match parse_proc_stat(&buf[..n]) {
            Ok(v) => v,
            Err(_) => continue,
        };

        total = total.saturating_add(1);
        match state {
            b'R' => running = running.saturating_add(1),
            b'D' => blocked = blocked.saturating_add(1),
            b'S' | b'I' => sleeping = sleeping.saturating_add(1),
            _ => {}
        }

        let curr_ticks = utime.saturating_add(stime);
        let prev_ticks = prev_proc_ticks.get(&pid).copied().unwrap_or(0);
        let delta_proc = curr_ticks.saturating_sub(prev_ticks);
        current_proc_ticks.insert(pid, curr_ticks);

        let cpu_percent = if global_delta > 0 {
            (delta_proc as f32 * ncores / global_delta as f32) * 100.0
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

    proc_cache.prune(&active_pids);

    *prev_total_ticks = current_total_ticks;
    *prev_proc_ticks = current_proc_ticks;

    out.total = total;
    out.running = running;
    out.blocked = blocked;
    out.sleeping = sleeping;
    out.top_cpu = cpu_heap.as_desc_array();
    out.top_mem = mem_heap.as_desc_array();

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
