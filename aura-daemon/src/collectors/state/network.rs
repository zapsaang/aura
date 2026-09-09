use aura_common::FixedString16;

pub const NET_KEY_LEN: usize = 20;
pub const NET_MAP_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetIfKey {
    pub bytes: [u8; NET_KEY_LEN],
}

impl NetIfKey {
    pub const fn empty() -> Self {
        Self {
            bytes: [0u8; NET_KEY_LEN],
        }
    }

    pub fn from_linux_name(name: &[u8]) -> Self {
        let mut key = Self::empty();
        let len = name.len().min(NET_KEY_LEN);
        let mut i = 0;
        while i < len {
            key.bytes[i] = name[i];
            i += 1;
        }
        key
    }

    pub fn from_macos(index: u32, name: &[u8]) -> Self {
        let mut key = Self::empty();
        key.bytes[0] = (index >> 24) as u8;
        key.bytes[1] = (index >> 16) as u8;
        key.bytes[2] = (index >> 8) as u8;
        key.bytes[3] = index as u8;
        let max_name = NET_KEY_LEN - 4;
        let len = name.len().min(max_name);
        let mut i = 0;
        while i < len {
            key.bytes[4 + i] = name[i];
            i += 1;
        }
        key
    }

    pub fn matches(&self, other: &Self) -> bool {
        let mut i = 0;
        while i < NET_KEY_LEN {
            if self.bytes[i] != other.bytes[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn is_empty(&self) -> bool {
        let mut i = 0;
        while i < NET_KEY_LEN {
            if self.bytes[i] != 0 {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn from_name_bytes(name: &FixedString16) -> Self {
        let mut key = Self::empty();
        let max_len = NET_KEY_LEN - 4;
        let mut i = 0;
        while i < max_len && i < name.bytes.len() {
            key.bytes[i] = name.bytes[i];
            i += 1;
        }
        key
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetIfSlot {
    pub key: NetIfKey,
    pub generation: u32,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

impl NetIfSlot {
    pub const fn empty() -> Self {
        Self {
            key: NetIfKey::empty(),
            generation: 0,
            rx_bytes: 0,
            tx_bytes: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NetByteSnapshot {
    pub slots: [NetIfSlot; NET_MAP_CAPACITY],
    pub generation: u32,
    pub represented: usize,
}

impl Default for NetByteSnapshot {
    fn default() -> Self {
        Self::zero()
    }
}

impl NetByteSnapshot {
    pub const fn zero() -> Self {
        Self {
            slots: [NetIfSlot::empty(); NET_MAP_CAPACITY],
            generation: 0,
            represented: 0,
        }
    }

    pub fn probe(&self, key: &NetIfKey) -> (usize, bool) {
        let mut index = Self::hash(key) % NET_MAP_CAPACITY;
        let mut first_empty = NET_MAP_CAPACITY;
        let mut probed = 0;
        while probed < NET_MAP_CAPACITY {
            let slot = &self.slots[index];
            if slot.generation == 0 {
                if first_empty == NET_MAP_CAPACITY {
                    first_empty = index;
                }
                return (first_empty, false);
            }
            if slot.key.matches(key) {
                return (index, true);
            }
            index = (index + 1) % NET_MAP_CAPACITY;
            probed += 1;
        }
        (first_empty, false)
    }

    pub fn get(&self, key: &NetIfKey) -> Option<&NetIfSlot> {
        let (index, found) = self.probe(key);
        if found {
            Some(&self.slots[index])
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, key: &NetIfKey) -> Option<&mut NetIfSlot> {
        let (index, found) = self.probe(key);
        if found {
            Some(&mut self.slots[index])
        } else {
            None
        }
    }

    pub fn insert(&mut self, key: NetIfKey, generation: u32, rx: u64, tx: u64) -> bool {
        let (index, found) = self.probe(&key);
        if !found {
            if index >= NET_MAP_CAPACITY {
                return false;
            }
            self.slots[index] = NetIfSlot {
                key,
                generation,
                rx_bytes: rx,
                tx_bytes: tx,
            };
            return true;
        }
        self.slots[index].rx_bytes = rx;
        self.slots[index].tx_bytes = tx;
        self.slots[index].generation = generation;
        false
    }

    pub fn sweep(&mut self, current_generation: u32) -> usize {
        let mut survived = 0;
        for slot in &mut self.slots {
            if slot.generation != 0 && slot.generation == current_generation {
                survived += 1;
            } else if slot.generation != 0 {
                *slot = NetIfSlot::empty();
            }
        }
        survived
    }

    fn hash(key: &NetIfKey) -> usize {
        let mut h: u32 = 0x811c9dc5;
        let mut i = 0;
        while i < NET_KEY_LEN {
            h ^= key.bytes[i] as u32;
            h = h.wrapping_mul(0x01000193);
            i += 1;
        }
        h as usize
    }
}
