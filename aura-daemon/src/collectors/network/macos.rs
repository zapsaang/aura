use aura_common::{AuraResult, NetworkStats};

use crate::collectors::NetByteSnapshot;

pub struct MacosNetworkCollector;

impl super::NetworkCollector for MacosNetworkCollector {
    fn collect(
        &self,
        _buf: &mut [u8; 4096],
        out: &mut NetworkStats,
        prev: &mut NetByteSnapshot,
        _delta_secs: f32,
    ) -> AuraResult<()> {
        out.if_count = 0;
        prev.count = 0;
        Ok(())
    }
}
