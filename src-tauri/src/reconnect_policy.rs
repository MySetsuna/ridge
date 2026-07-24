//! R17-RECONN: pure reconnect backoff schedule (no I/O).
//! Used by cloud/LAN reconnect paths as a shared policy.

/// Default base delay (ms) and max delay (ms). Cloud TS uses 1000/15000.
pub const DEFAULT_BASE_MS: u64 = 1_000;
pub const DEFAULT_MAX_MS: u64 = 15_000;
/// Teammate shim HTTP retry (tmux bin) uses a tighter window.
pub const SHIM_BASE_MS: u64 = 150;
pub const SHIM_MAX_MS: u64 = 600;

/// Exponential backoff: `min(max_ms, base_ms * 2^attempt)` with attempt starting at 0.
pub fn backoff_ms(attempt: u32, base_ms: u64, max_ms: u64) -> u64 {
    let base = base_ms.max(1);
    let max = max_ms.max(base);
    let shift = attempt.min(16); // prevent overflow
    let exp = base.saturating_mul(1u64 << shift);
    exp.min(max)
}

/// Whether another reconnect attempt is allowed.
pub fn should_retry(attempt: u32, max_attempts: u32) -> bool {
    attempt < max_attempts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff_ms(0, 500, 30_000), 500);
        assert_eq!(backoff_ms(1, 500, 30_000), 1000);
        assert_eq!(backoff_ms(2, 500, 30_000), 2000);
        assert_eq!(backoff_ms(10, 500, 30_000), 30_000);
        assert_eq!(backoff_ms(20, 500, 30_000), 30_000);
    }

    #[test]
    fn retry_gate() {
        assert!(should_retry(0, 5));
        assert!(should_retry(4, 5));
        assert!(!should_retry(5, 5));
    }
}
