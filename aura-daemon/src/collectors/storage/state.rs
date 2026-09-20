//! Fixed-capacity disk baseline state keyed by `(major, minor)` device
//! numbers. Mirrors the network identity map: open addressing over a fixed
//! slot array with monotonic generation markers; devices that disappear are
//! swept after each finalized cycle.

pub const DISK_MAP_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiskKey {
    pub major: u32,
    pub minor: u32,
}

impl DiskKey {
    pub const fn empty() -> Self {
        Self { major: 0, minor: 0 }
    }
}

/// Raw cumulative counters that never appear in the archive; the finalize
/// rate pass diffs them against the previous snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiskRawSnapshot {
    pub sectors_read: u64,
    pub sectors_written: u64,
    pub reads_completed: u64,
    pub read_ms: u64,
    pub writes_completed: u64,
    pub write_ms: u64,
}

impl DiskRawSnapshot {
    pub const fn zero() -> Self {
        Self {
            sectors_read: 0,
            sectors_written: 0,
            reads_completed: 0,
            read_ms: 0,
            writes_completed: 0,
            write_ms: 0,
        }
    }

    /// True when every current counter is at least the baseline counterpart;
    /// any decrease is a counter reset.
    pub fn covers(&self, previous: &Self) -> bool {
        self.sectors_read >= previous.sectors_read
            && self.sectors_written >= previous.sectors_written
            && self.reads_completed >= previous.reads_completed
            && self.read_ms >= previous.read_ms
            && self.writes_completed >= previous.writes_completed
            && self.write_ms >= previous.write_ms
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskSlot {
    pub key: DiskKey,
    pub generation: u32,
    pub counters: DiskRawSnapshot,
}

impl DiskSlot {
    pub const fn empty() -> Self {
        Self {
            key: DiskKey::empty(),
            generation: 0,
            counters: DiskRawSnapshot::zero(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DiskBaselineMap {
    pub slots: [DiskSlot; DISK_MAP_CAPACITY],
    pub generation: u32,
}

impl Default for DiskBaselineMap {
    fn default() -> Self {
        Self::zero()
    }
}

impl DiskBaselineMap {
    pub const fn zero() -> Self {
        Self {
            slots: [DiskSlot::empty(); DISK_MAP_CAPACITY],
            generation: 0,
        }
    }

    fn probe(&self, key: &DiskKey) -> (usize, bool) {
        let mut index = Self::hash(key) % DISK_MAP_CAPACITY;
        let mut first_empty = DISK_MAP_CAPACITY;
        let mut probed = 0;
        while probed < DISK_MAP_CAPACITY {
            let slot = &self.slots[index];
            if slot.generation == 0 {
                if first_empty == DISK_MAP_CAPACITY {
                    first_empty = index;
                }
                return (first_empty, false);
            }
            if slot.key == *key {
                return (index, true);
            }
            index = (index + 1) % DISK_MAP_CAPACITY;
            probed += 1;
        }
        (first_empty, false)
    }

    pub fn get(&self, key: &DiskKey) -> Option<&DiskSlot> {
        let (index, found) = self.probe(key);
        if found {
            Some(&self.slots[index])
        } else {
            None
        }
    }

    /// Inserts or replaces a snapshot; returns false when the map is full.
    pub fn insert(&mut self, key: DiskKey, generation: u32, counters: DiskRawSnapshot) -> bool {
        let (index, found) = self.probe(&key);
        if !found {
            if index >= DISK_MAP_CAPACITY {
                return false;
            }
            self.slots[index] = DiskSlot {
                key,
                generation,
                counters,
            };
            return true;
        }
        self.slots[index].counters = counters;
        self.slots[index].generation = generation;
        false
    }

    /// Drops every slot not marked with the current generation.
    pub fn sweep(&mut self, current_generation: u32) {
        for slot in &mut self.slots {
            if slot.generation != 0 && slot.generation != current_generation {
                *slot = DiskSlot::empty();
            }
        }
    }

    fn hash(key: &DiskKey) -> usize {
        let mut h: u32 = 0x811c9dc5;
        for byte in key
            .major
            .to_le_bytes()
            .into_iter()
            .chain(key.minor.to_le_bytes())
        {
            h ^= u32::from(byte);
            h = h.wrapping_mul(0x0100_0193);
        }
        h as usize
    }
}
