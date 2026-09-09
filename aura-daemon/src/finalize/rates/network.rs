use aura_common::{CAP_NETWORK_RATES, MAX_NETIFS};

use crate::collectors::{FixedCollectorState, NetIfKey};

pub(super) fn finalize(state: &mut FixedCollectorState, elapsed: f64, warmed: bool) {
    let caps = state.archive.capabilities;
    let network = &mut state.archive.network;
    let represented = (network.if_count as usize).min(MAX_NETIFS);

    let mut aggregate_rx = 0.0f32;
    let mut aggregate_tx = 0.0f32;
    let next_generation = state.baselines.net_bytes.generation.wrapping_add(1);
    let allow_rates = caps & CAP_NETWORK_RATES != 0;

    for index in 0..represented {
        let interface = &mut network.interfaces[index];
        let key = NetIfKey::from_name_bytes(&interface.name);
        let previous = if warmed && allow_rates {
            state.baselines.net_bytes.get(&key).copied()
        } else {
            None
        };
        match previous {
            Some(slot)
                if slot.rx_bytes <= interface.rx_bytes && slot.tx_bytes <= interface.tx_bytes =>
            {
                interface.rx_bytes_per_sec =
                    (interface.rx_bytes - slot.rx_bytes) as f32 / elapsed as f32;
                interface.tx_bytes_per_sec =
                    (interface.tx_bytes - slot.tx_bytes) as f32 / elapsed as f32;
                aggregate_rx += interface.rx_bytes_per_sec;
                aggregate_tx += interface.tx_bytes_per_sec;
            }
            _ => {
                interface.rx_bytes_per_sec = 0.0;
                interface.tx_bytes_per_sec = 0.0;
            }
        }
        state.baselines.net_bytes.insert(
            key,
            next_generation,
            interface.rx_bytes,
            interface.tx_bytes,
        );
    }

    if allow_rates && represented >= 1 {
        state.archive.derived.aggregate_rx_bytes_per_sec = aggregate_rx;
        state.archive.derived.aggregate_tx_bytes_per_sec = aggregate_tx;
    } else {
        state.archive.derived.aggregate_rx_bytes_per_sec = 0.0;
        state.archive.derived.aggregate_tx_bytes_per_sec = 0.0;
    }

    state.baselines.net_bytes.sweep(next_generation);
    state.baselines.net_bytes.generation = next_generation;
    state.baselines.net_bytes.represented = represented;
}
