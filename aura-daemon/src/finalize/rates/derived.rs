use aura_common::{
    CAP_MEMORY_RAM_TOTAL, CAP_MEMORY_RAM_USED, CAP_MEMORY_SWAP, TONE_GREEN, TONE_MAGENTA, TONE_RED,
    TONE_YELLOW,
};

use crate::collectors::FixedCollectorState;

pub(super) fn finalize(state: &mut FixedCollectorState) {
    let derived = &mut state.archive.derived;
    let caps = state.archive.capabilities;
    let ram_prereq = caps & CAP_MEMORY_RAM_TOTAL != 0 && caps & CAP_MEMORY_RAM_USED != 0;
    let swap_prereq = caps & CAP_MEMORY_SWAP != 0;

    derived.cpu_tone = tone_from_percent(state.archive.cpu.usage_percent);
    derived.ram_tone = if ram_prereq {
        tone_from_percent(derived.ram_used_percent)
    } else {
        TONE_GREEN
    };
    derived.swap_tone = if swap_prereq {
        tone_from_percent(derived.swap_used_percent)
    } else {
        TONE_GREEN
    };
}

pub(super) fn tone_from_percent(value: f32) -> u8 {
    if value.is_nan() {
        return TONE_GREEN;
    }
    if value >= 80.0 {
        TONE_RED
    } else if value >= 70.0 {
        TONE_YELLOW
    } else if value >= 60.0 {
        TONE_MAGENTA
    } else {
        TONE_GREEN
    }
}
