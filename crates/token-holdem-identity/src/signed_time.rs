pub const MAX_SIGNED_MESSAGE_CLOCK_SKEW_MS: u64 = 5 * 60 * 1_000;

#[must_use]
pub const fn is_signed_time_not_future(timestamp_unix_ms: u64, now_unix_ms: u64) -> bool {
    timestamp_unix_ms <= now_unix_ms.saturating_add(MAX_SIGNED_MESSAGE_CLOCK_SKEW_MS)
}

#[must_use]
pub const fn is_signed_time_before_expiry(timestamp_unix_ms: u64, expires_at_unix_ms: u64) -> bool {
    timestamp_unix_ms < expires_at_unix_ms.saturating_add(MAX_SIGNED_MESSAGE_CLOCK_SKEW_MS)
}

#[must_use]
pub const fn is_signed_time_window_active(
    created_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    now_unix_ms: u64,
) -> bool {
    is_signed_time_not_future(created_at_unix_ms, now_unix_ms)
        && is_signed_time_before_expiry(now_unix_ms, expires_at_unix_ms)
}

#[must_use]
pub const fn is_signed_time_window_expired(expires_at_unix_ms: u64, now_unix_ms: u64) -> bool {
    !is_signed_time_before_expiry(now_unix_ms, expires_at_unix_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 签名时间窗口允许有限双向时钟偏差() {
        let created_at = 1_000_000;
        let expires_at = 1_060_000;

        assert!(is_signed_time_window_active(
            created_at,
            expires_at,
            created_at - MAX_SIGNED_MESSAGE_CLOCK_SKEW_MS
        ));
        assert!(is_signed_time_window_active(
            created_at,
            expires_at,
            expires_at + MAX_SIGNED_MESSAGE_CLOCK_SKEW_MS - 1
        ));
        assert!(!is_signed_time_window_active(
            created_at,
            expires_at,
            created_at - MAX_SIGNED_MESSAGE_CLOCK_SKEW_MS - 1
        ));
        assert!(!is_signed_time_window_active(
            created_at,
            expires_at,
            expires_at + MAX_SIGNED_MESSAGE_CLOCK_SKEW_MS
        ));
    }
}
