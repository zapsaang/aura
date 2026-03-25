use std::cmp::Ordering;
use std::fs::{read_dir, File};
use std::io::Read;

use aura_common::{
    AuraError, AuraResult, FixedString16, ProcessStat, ProcessStats, MAX_PID, MAX_TOP_N,
};

#[derive(Clone, Copy)]
struct HeapEntry {
    key: u64,
    stat: ProcessStat,
}

impl HeapEntry {
    fn new(key: u64, stat: ProcessStat) -> Self {
        Self { key, stat }
    }
}

impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for HeapEntry {}
impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key.cmp(&other.key)
    }
}

struct MinHeap5 {
    heap: [Option<HeapEntry>; MAX_TOP_N],
    count: usize,
}

impl MinHeap5 {
    fn new() -> Self {
        Self {
            heap: [None, None, None, None, None],
            count: 0,
        }
    }

    fn push(&mut self, val: HeapEntry) {
        if self.count < MAX_TOP_N {
            self.heap[self.count] = Some(val);
            self.bubble_up(self.count);
            self.count += 1;
            return;
        }

        if let Some(root) = self.heap[0] {
            if val > root {
                self.heap[0] = Some(val);
                self.bubble_down(0);
            }
        }
    }

    fn bubble_up(&mut self, mut i: usize) {
        while i > 0 {
            let p = (i - 1) / 2;
            if self.heap[i] < self.heap[p] {
                self.heap.swap(i, p);
                i = p;
            } else {
                break;
            }
        }
    }

    fn bubble_down(&mut self, mut i: usize) {
        loop {
            let l = i * 2 + 1;
            let r = i * 2 + 2;
            let mut s = i;

            if l < self.count && self.heap[l] < self.heap[s] {
                s = l;
            }
            if r < self.count && self.heap[r] < self.heap[s] {
                s = r;
            }
            if s == i {
                break;
            }
            self.heap.swap(i, s);
            i = s;
        }
    }

    fn as_desc_array(&self) -> [ProcessStat; MAX_TOP_N] {
        let mut tmp = self.heap;
        let mut n = self.count;
        let mut out = [zero_process(); MAX_TOP_N];
        let mut idx = 0usize;

        while n > 0 && idx < MAX_TOP_N {
            let max_i = max_entry_index(&tmp, n);
            if let Some(v) = tmp[max_i] {
                out[idx] = v.stat;
                idx += 1;
            }
            tmp[max_i] = tmp[n - 1];
            tmp[n - 1] = None;
            n -= 1;
        }

        out
    }
}

fn max_entry_index(arr: &[Option<HeapEntry>; MAX_TOP_N], n: usize) -> usize {
    let mut max_i = 0usize;
    let mut i = 1usize;
    while i < n {
        if arr[i] > arr[max_i] {
            max_i = i;
        }
        i += 1;
    }
    max_i
}

const fn zero_process() -> ProcessStat {
    ProcessStat {
        pid: 0,
        cpu_usage: 0.0,
        memory_bytes: 0,
        comm: FixedString16 { bytes: [0; 16] },
    }
}

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
        rss_pages.saturating_mul(4096),
        state,
    ))
}

pub fn collect_top_n(
    buf: &mut [u8; 4096],
    out: &mut ProcessStats,
    prev_proc_ticks: &mut [u64; (MAX_PID as usize) + 1],
    prev_total_ticks: &mut u64,
    current_total_ticks: u64,
) -> AuraResult<()> {
    let mut total = 0u32;
    let mut running = 0u32;
    let mut blocked = 0u32;
    let mut sleeping = 0u32;
    let mut cpu_heap = MinHeap5::new();
    let mut mem_heap = MinHeap5::new();

    let global_delta = current_total_ticks.saturating_sub(*prev_total_ticks);

    for entry in read_dir("/proc")? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let name = entry.file_name();
        let pid = match parse_u64(name.as_encoded_bytes()) {
            Ok(v) if v > 0 && v <= MAX_PID as u64 => v as usize,
            _ => continue,
        };

        let stat_path = entry.path().join("stat");
        let mut file = match File::open(stat_path) {
            Ok(f) => f,
            Err(_) => continue,
        };

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
        let prev_ticks = prev_proc_ticks[pid];
        let delta_proc = curr_ticks.saturating_sub(prev_ticks);
        prev_proc_ticks[pid] = curr_ticks;

        let cpu_percent = if global_delta > 0 {
            (delta_proc as f32 / global_delta as f32) * 100.0
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

    *prev_total_ticks = current_total_ticks;

    out.total = total;
    out.running = running;
    out.blocked = blocked;
    out.sleeping = sleeping;
    out.top_cpu = cpu_heap.as_desc_array();
    out.top_mem = mem_heap.as_desc_array();

    Ok(())
}

fn parse_u64(b: &[u8]) -> AuraResult<u64> {
    let mut out = 0u64;
    let mut seen = false;
    for &c in b {
        if c.is_ascii_digit() {
            out = out.saturating_mul(10).saturating_add((c - b'0') as u64);
            seen = true;
        } else if seen {
            break;
        } else {
            return Err(AuraError::ParseError("u64 parse failed".to_string()));
        }
    }
    if seen {
        Ok(out)
    } else {
        Err(AuraError::ParseError("u64 parse failed".to_string()))
    }
}

fn trim_ascii(mut b: &[u8]) -> &[u8] {
    while !b.is_empty() && b[0].is_ascii_whitespace() {
        b = &b[1..];
    }
    while !b.is_empty() && b[b.len() - 1].is_ascii_whitespace() {
        b = &b[..b.len() - 1];
    }
    b
}

fn split_whitespace(mut b: &[u8]) -> impl Iterator<Item = &[u8]> {
    std::iter::from_fn(move || {
        while !b.is_empty() && b[0].is_ascii_whitespace() {
            b = &b[1..];
        }
        if b.is_empty() {
            return None;
        }
        let mut end = 0usize;
        while end < b.len() && !b[end].is_ascii_whitespace() {
            end += 1;
        }
        let token = &b[..end];
        b = &b[end..];
        Some(token)
    })
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
        assert_eq!(rss, 4096 * 200);
    }
}
