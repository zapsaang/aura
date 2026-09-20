use aura_common::{
    CAP_CPU_GLOBAL, GPU_CAP_TEMPERATURE, TONE_GREEN, TONE_MAGENTA, TONE_RED, TONE_YELLOW,
};

use crate::collectors::FixedCollectorState;

pub(super) fn finalize(state: &mut FixedCollectorState) {
    finalize_gpu(state);
    finalize_cpu_tone_zero(state);
}

fn finalize_gpu(state: &mut FixedCollectorState) {
    for gpu in &mut state.archive.gpu.gpus {
        if gpu.capabilities & GPU_CAP_TEMPERATURE == 0 {
            gpu.tone = TONE_GREEN;
        } else {
            gpu.tone = temperature_tone(gpu.temperature_celsius);
        }
    }
}

fn finalize_cpu_tone_zero(state: &mut FixedCollectorState) {
    let caps = state.archive.capabilities;
    let derived = &mut state.archive.derived;
    if caps & CAP_CPU_GLOBAL == 0 {
        derived.cpu_tone = TONE_GREEN;
    }
}

fn temperature_tone(celsius: i16) -> u8 {
    if celsius >= 80 {
        TONE_RED
    } else if celsius >= 70 {
        TONE_YELLOW
    } else if celsius >= 60 {
        TONE_MAGENTA
    } else {
        TONE_GREEN
    }
}
