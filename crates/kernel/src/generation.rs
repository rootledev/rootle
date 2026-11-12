//! Monotonic request issuance. Exhaustion refuses instead of reusing an epoch.

/// Issue the next generation, or refuse at the end of the identifier space.
pub fn next_generation(current: u64) -> Option<u64> {
    if current == u64::MAX {
        None
    } else {
        Some(current + 1)
    }
}
