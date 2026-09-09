use aura_common::{AuraResult, NetworkStats};

pub fn collect(_buf: &mut Vec<u8>, out: &mut NetworkStats) -> AuraResult<()> {
    out.if_count = 0;
    Ok(())
}
