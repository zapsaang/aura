//! Fixed process baseline state: open-addressed table keyed by
//! `(pid, starttime)` with generation markers, tombstones, and a per-cycle
//! rehash into a zeroed spare table (no allocation, no unbounded probing).

use aura_common::FixedString16;

pub const PROCESS_BASELINE_CAPACITY: usize = 4096;

pub type ProcessKey = (u32, u64);

#[derive(Clone, Copy, Debug)]
pub struct ProcessProcStat {
    pub pid: u32,
    pub comm: FixedString16,
    pub state: u8,
    pub utime: u64,
    pub stime: u64,
    pub starttime: u64,
    pub rss_pages: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct ProcessBaseSnapshot {
    pub utime: u64,
    pub stime: u64,
}

impl ProcessBaseSnapshot {
    pub const fn zero() -> Self {
        Self { utime: 0, stime: 0 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProcessBaseStat {
    pub pid: u32,
    pub generation: u32,
    pub starttime: u64,
    pub stat: ProcessBaseSnapshot,
}

impl ProcessBaseStat {
    pub const fn empty() -> Self {
        Self {
            pid: 0,
            generation: 0,
            starttime: 0,
            stat: ProcessBaseSnapshot::zero(),
        }
    }
}

const _: () = assert!(
    std::mem::size_of::<ProcessBaseStat>() == 32,
    "baseline entries stay at 32 bytes so both fixed tables fit 256 KiB"
);

#[derive(Clone, Copy, Debug)]
pub struct ProcessBaseline {
    pub slots: [ProcessBaseStat; PROCESS_BASELINE_CAPACITY],
    rehash_target: [ProcessBaseStat; PROCESS_BASELINE_CAPACITY],
    pub current_generation: u32,
}

impl Default for ProcessBaseline {
    fn default() -> Self {
        Self {
            slots: [ProcessBaseStat::empty(); PROCESS_BASELINE_CAPACITY],
            rehash_target: [ProcessBaseStat::empty(); PROCESS_BASELINE_CAPACITY],
            current_generation: 0,
        }
    }
}

impl ProcessBaseline {
    pub fn probe(&self, key: &ProcessKey) -> (usize, bool) {
        let mut index = Self::hash(key) % PROCESS_BASELINE_CAPACITY;
        let mut probed = 0;
        while probed < PROCESS_BASELINE_CAPACITY {
            let slot = self.slots[index];
            if slot.generation == 0 {
                return (index, false);
            }
            if slot.pid == key.0 && slot.starttime == key.1 {
                return (index, true);
            }
            index = (index + 1) % PROCESS_BASELINE_CAPACITY;
            probed += 1;
        }
        (PROCESS_BASELINE_CAPACITY, false)
    }

    pub fn get(&self, key: &ProcessKey) -> Option<&ProcessBaseStat> {
        let (index, found) = self.probe(key);
        if found {
            Some(&self.slots[index])
        } else {
            None
        }
    }

    pub fn insert(&mut self, key: ProcessKey, stat: ProcessBaseSnapshot) -> u32 {
        let (index, found) = self.probe(&key);
        self.current_generation = self.current_generation.wrapping_add(1);
        if self.current_generation == 0 {
            self.current_generation = 1;
        }
        let generation = self.current_generation;
        if !found {
            if index >= PROCESS_BASELINE_CAPACITY {
                return generation;
            }
            self.slots[index] = ProcessBaseStat {
                pid: key.0,
                generation,
                starttime: key.1,
                stat,
            };
            return generation;
        }
        self.slots[index].stat = stat;
        self.slots[index].generation = generation;
        generation
    }

    pub fn generation(&self) -> u32 {
        self.current_generation
    }

    /// Tombstones every occupied slot that was not re-stamped after
    /// `cycle_start_generation`. Wrap-safe: a slot survives only when its
    /// generation falls inside the half-open range stamped this cycle.
    pub fn sweep_unmarked(&mut self, cycle_start_generation: u32) {
        let span = self.current_generation.wrapping_sub(cycle_start_generation);
        for slot in self.slots.iter_mut() {
            if slot.generation == 0 {
                continue;
            }
            let age = slot.generation.wrapping_sub(cycle_start_generation);
            if age == 0 || age > span {
                *slot = ProcessBaseStat::empty();
            }
        }
    }

    /// Rebuilds the table into the zeroed spare (dropping tombstones), then
    /// swaps; runs after every complete non-Fatal scan. Live entries never
    /// exceed capacity, so reinsertion probing always terminates.
    pub fn rehash_after_cycle(&mut self) {
        for slot in self.rehash_target.iter_mut() {
            *slot = ProcessBaseStat::empty();
        }
        for slot in self.slots.iter() {
            if slot.generation != 0 {
                let key = (slot.pid, slot.starttime);
                let mut index = Self::hash(&key) % PROCESS_BASELINE_CAPACITY;
                while self.rehash_target[index].generation != 0 {
                    index = (index + 1) % PROCESS_BASELINE_CAPACITY;
                }
                self.rehash_target[index] = *slot;
            }
        }
        std::mem::swap(&mut self.slots, &mut self.rehash_target);
    }

    fn hash(key: &ProcessKey) -> usize {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= u64::from(key.0);
        h = h.wrapping_mul(0x100000001b3);
        h ^= key.1;
        h = h.wrapping_mul(0x100000001b3);
        h as usize
    }
}
