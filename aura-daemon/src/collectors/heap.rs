use std::cmp::Ordering;

use aura_common::{FixedString16, ProcessStat, MAX_TOP_N};

#[derive(Clone, Copy)]
pub struct HeapEntry {
    key: u64,
    pub stat: ProcessStat,
}

impl HeapEntry {
    pub fn new(key: u64, stat: ProcessStat) -> Self {
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

pub struct MinHeap5 {
    heap: [Option<HeapEntry>; MAX_TOP_N],
    count: usize,
}

impl Default for MinHeap5 {
    fn default() -> Self {
        Self::new()
    }
}

impl MinHeap5 {
    pub fn new() -> Self {
        Self {
            heap: [None, None, None, None, None],
            count: 0,
        }
    }

    pub fn push(&mut self, val: HeapEntry) {
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

    pub fn as_desc_array(&self) -> [ProcessStat; MAX_TOP_N] {
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

pub const fn zero_process() -> ProcessStat {
    ProcessStat {
        pid: 0,
        cpu_usage: 0.0,
        memory_bytes: 0,
        comm: FixedString16 { bytes: [0; 16] },
    }
}
