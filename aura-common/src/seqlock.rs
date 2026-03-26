use crate::{AuraError, AuraResult};

#[inline]
pub fn validate_freshness(timestamp_ns: u64, threshold_ns: u64) -> AuraResult<()> {
    let now = crate::time::monotonic_ns();
    let age_ns = now.saturating_sub(timestamp_ns);

    if age_ns > threshold_ns {
        Err(AuraError::StaleData {
            age_ms: age_ns / 1_000_000,
            threshold_ms: threshold_ns / 1_000_000,
        })
    } else {
        Ok(())
    }
}
