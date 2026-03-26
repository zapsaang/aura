use aura_common::{AuraResult, NetworkStats};

use crate::collectors::NetByteSnapshot;

pub fn collect(
    _buf: &mut Vec<u8>,
    out: &mut NetworkStats,
    prev: &mut NetByteSnapshot,
    _delta_secs: f64,
) -> AuraResult<()> {
    out.if_count = 0;
    prev.count = 0;
    Ok(())
}
