//! The idle sweep: removes a container nothing has run in for a while.
//!
//! One container serves one session and is kept between commands, so its
//! writable layer survives a `pip install`. The cost of that is a container that
//! outlives the conversation; this task is the other half of the trade. Removing
//! it loses the layer and keeps the workspace mount and the cache volumes, so
//! the next command recreates the container from warm caches.
//!
//! The interval and the timeout are fixed for now. `Registry::sweep_idle` takes
//! the timeout as an argument, so turning it into a setting later changes the
//! call site here and nothing else.
//!
//! What this cannot see: a container left by a **crashed** process. It is in no
//! registry, so no in-process sweep will find it. That needs a signal that
//! outlives the process and is a separate decision.

use std::time::Duration;

use tokio::time::{interval_at, Instant, MissedTickBehavior};

use super::Registry;

/// How often the sweep runs.
pub const REAP_INTERVAL: Duration = Duration::from_secs(120);

/// How long a container may go without a command before it is removed.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(600);

/// Starts the sweep on the current runtime. Returns immediately.
///
/// Called once from the composition root, which is the one place `robi-api`,
/// `robi`, and the Tauri process all pass through, so every surface gets it
/// without its own copy.
pub fn spawn() {
    tokio::spawn(async {
        // The first tick is one interval away: startup does no Docker I/O, and a
        // test that calls `build_app_state` never reaches a daemon while it runs.
        let start = Instant::now() + REAP_INTERVAL;
        let mut ticker = interval_at(start, REAP_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            sweep_once().await;
        }
    });
}

/// One pass. Kept separate from the loop so the sweep can be called directly in
/// a test without waiting on a tick.
async fn sweep_once() {
    let reaped = Registry::global().sweep_idle(IDLE_TIMEOUT).await;
    if reaped.is_empty() {
        tracing::debug!("idle sandbox sweep found nothing to remove");
    }
}

#[cfg(test)]
mod tests {
    use super::super::is_reapable;
    use super::*;

    fn at(start: Instant, after: Duration) -> Instant {
        start + after
    }

    #[tokio::test]
    async fn nothing_is_reapable_before_the_timeout() {
        let start = Instant::now();
        let almost = at(start, IDLE_TIMEOUT - Duration::from_secs(1));
        assert!(!is_reapable(start, almost, IDLE_TIMEOUT));
    }

    #[tokio::test]
    async fn exactly_the_timeout_counts_as_idle() {
        let start = Instant::now();
        assert!(is_reapable(start, at(start, IDLE_TIMEOUT), IDLE_TIMEOUT));
    }

    #[tokio::test]
    async fn well_past_the_timeout_is_reapable() {
        let start = Instant::now();
        let later = at(start, IDLE_TIMEOUT * 3);
        assert!(is_reapable(start, later, IDLE_TIMEOUT));
    }

    #[test]
    fn the_interval_is_shorter_than_the_timeout() {
        // Otherwise a container would sit idle for nearly two timeouts before it
        // went, and the timeout would not mean what it says.
        const _: () = assert!(REAP_INTERVAL.as_secs() < IDLE_TIMEOUT.as_secs());
    }
}
