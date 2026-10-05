//! When to try again, and how long to wait.
//!
//! A retry is only safe while the adapter has emitted nothing. That boundary is
//! enforced by the caller, which wraps the initial request and never a stream in
//! progress: see `openai::OpenAiCompatibleModel::send_with_retry`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use http::{HeaderMap, HeaderValue, StatusCode};

/// How many times to try, and how long to wait between attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts, counting the first. `1` disables retrying.
    pub max_attempts: u32,
    pub base: Duration,
    pub cap: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base: Duration::from_millis(500),
            cap: Duration::from_secs(8),
        }
    }
}

impl RetryPolicy {
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            ..Self::default()
        }
    }

    /// The wait before the next attempt.
    ///
    /// A `Retry-After` delay is used when the provider sent one, and it is
    /// never longer than [`Self::cap`]. Otherwise the wait is [`Self::backoff`].
    pub fn retry_delay(&self, attempt: u32, after: Option<Duration>) -> Duration {
        match after {
            Some(after) => after.min(self.cap),
            None => self.backoff(attempt),
        }
    }

    /// The wait before attempt `attempt + 1`, with full jitter.
    ///
    /// Full jitter — a uniform draw from zero to the exponential ceiling — spreads
    /// a fleet of clients that all hit the same rate limit at once. Without it they
    /// retry in lockstep and re-limit each other.
    pub fn backoff(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(16);
        let ceiling = self
            .base
            .saturating_mul(1u32 << exponent)
            .min(self.cap)
            .max(Duration::from_millis(1));
        jitter(ceiling)
    }
}

/// Whether a response may be retried, and the delay the provider asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    Yes { after: Option<Duration> },
    No,
}

/// Classify a response status.
///
/// A 429 is a rate limit; a 5xx is transient by contract; every other failure is a
/// property of the request and will fail again unchanged.
pub fn classify(status: StatusCode, headers: &HeaderMap) -> Retry {
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Retry::Yes {
            after: retry_after(headers),
        };
    }
    if status.is_server_error() {
        return Retry::Yes { after: None };
    }
    Retry::No
}

/// A `Retry-After` delay in delta-seconds.
///
/// The header also allows an HTTP date. These servers send the seconds form, and
/// parsing a date correctly needs a calendar; an unrecognized value falls back to
/// the policy's backoff, which is a safe answer rather than a wrong one.
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let raw: &HeaderValue = headers.get(http::header::RETRY_AFTER)?;
    let seconds: u64 = raw.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(seconds))
}

/// A uniform draw in `[0, ceiling]`, from the wall clock.
///
/// Jitter does not need cryptographic randomness, and taking a dependency for it
/// would be the largest thing in this module.
fn jitter(ceiling: Duration) -> Duration {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.subsec_nanos() as u64)
        .unwrap_or(0);
    let ceiling_nanos = ceiling.as_nanos() as u64;
    if ceiling_nanos == 0 {
        return Duration::ZERO;
    }
    Duration::from_nanos(nanos % (ceiling_nanos + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_limit_is_retryable_and_a_bad_request_is_not() {
        let empty = HeaderMap::new();
        assert!(matches!(
            classify(StatusCode::TOO_MANY_REQUESTS, &empty),
            Retry::Yes { .. }
        ));
        assert!(matches!(
            classify(StatusCode::INTERNAL_SERVER_ERROR, &empty),
            Retry::Yes { .. }
        ));
        for status in [
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::NOT_FOUND,
            StatusCode::UNPROCESSABLE_ENTITY,
        ] {
            assert_eq!(
                classify(status, &empty),
                Retry::No,
                "{status} is a property of the request and repeats unchanged"
            );
        }
    }

    #[test]
    fn retry_after_seconds_is_honoured() {
        let mut headers = HeaderMap::new();
        headers.insert(http::header::RETRY_AFTER, HeaderValue::from_static("2"));
        assert_eq!(
            classify(StatusCode::TOO_MANY_REQUESTS, &headers),
            Retry::Yes {
                after: Some(Duration::from_secs(2))
            }
        );
    }

    #[test]
    fn an_unreadable_retry_after_falls_back_to_backoff() {
        let mut headers = HeaderMap::new();
        // The HTTP-date form. Falling back to the policy's backoff is safe.
        headers.insert(
            http::header::RETRY_AFTER,
            HeaderValue::from_static("Wed, 21 Oct 2026 07:28:00 GMT"),
        );
        assert_eq!(
            classify(StatusCode::TOO_MANY_REQUESTS, &headers),
            Retry::Yes { after: None }
        );
    }

    #[test]
    fn backoff_grows_then_stops_at_the_cap() {
        let policy = RetryPolicy {
            max_attempts: 10,
            base: Duration::from_millis(500),
            cap: Duration::from_secs(8),
        };
        // Full jitter draws from [0, ceiling], so assert the ceiling, not a point.
        for attempt in 1..=6 {
            let ceiling = policy.base * (1u32 << (attempt - 1));
            let ceiling = ceiling.min(policy.cap);
            let delay = policy.backoff(attempt);
            assert!(
                delay <= ceiling,
                "attempt {attempt} waited {delay:?}, over the {ceiling:?} ceiling"
            );
        }
    }

    #[test]
    fn a_long_retry_after_stops_at_the_cap() {
        let policy = RetryPolicy::default();
        assert_eq!(
            policy.retry_delay(1, Some(Duration::from_secs(120))),
            policy.cap
        );
    }

    #[test]
    fn a_policy_of_one_attempt_never_waits() {
        assert_eq!(RetryPolicy::none().max_attempts, 1);
    }
}
