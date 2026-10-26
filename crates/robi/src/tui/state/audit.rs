//! The audit log: one row per effect the app asked for.
//!
//! The peer of the webview's request log, at a different boundary. `src/` makes
//! HTTP, so `src/state/requestLog.ts` records fetches through an
//! `openapi-fetch` middleware; the terminal makes no HTTP at all, so its rows are
//! service calls, recorded where every call already passes: `effects::dispatch`.
//! Same cap, same newest-first order, same memory-only lifetime — this is what the
//! app has done since it started, not a file.
//!
//! Three calls are left out by `Action::skips_audit` for the reason the webview
//! leaves out `/api/v1/health`: a heartbeat, a completion that runs on every
//! keystroke, and a write that happens while the process is leaving are noise
//! around the calls a reader opened the page to see.
//!
//! **The table has no selection of its own.** The settings view already has one
//! row cursor and one page size; a second cursor inside one section would be a
//! mode within a mode. The row under the cursor is the row `Enter` expands, which
//! is what `j`/`k` moving the cursor already means everywhere else.

use chrono::{DateTime, Utc};

/// How many rows are kept, as the webview's `MAX_ENTRIES`.
pub const MAX_ENTRIES: usize = 250;

/// One call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Assigned when the row is recorded, so an insert at the front cannot move
    /// an expanded row onto its neighbour.
    pub seq: u64,
    pub call: &'static str,
    pub ok: bool,
    pub duration_ms: u128,
    /// The outcome's own one-line summary, expanded with `Enter`.
    pub body: String,
    pub at: DateTime<Utc>,
}

/// The table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditLog {
    entries: Vec<Entry>,
    seq: u64,
    /// The row whose body is expanded, by `seq`.
    expanded: Option<u64>,
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditLog {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            seq: 0,
            expanded: None,
        }
    }

    /// Record one call at the front, and drop anything past the cap.
    pub fn record(&mut self, mut entry: Entry) {
        self.seq += 1;
        entry.seq = self.seq;
        self.entries.insert(0, entry);
        self.entries.truncate(MAX_ENTRIES);
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether the row at `index` shows its body.
    pub fn is_expanded(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|entry| self.expanded == Some(entry.seq))
    }

    /// Expand the row at `index`, or fold it when it is already open.
    pub fn toggle(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        let seq = entry.seq;
        self.expanded = if self.expanded == Some(seq) {
            None
        } else {
            Some(seq)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(call: &'static str) -> Entry {
        Entry {
            seq: 0,
            call,
            ok: true,
            duration_ms: 3,
            body: String::new(),
            at: Utc::now(),
        }
    }

    #[test]
    fn the_newest_call_is_the_first_row_and_each_row_has_its_own_number() {
        let mut log = AuditLog::new();
        log.record(entry("workspaces"));
        log.record(entry("sessions"));
        let calls: Vec<_> = log.entries().iter().map(|entry| entry.call).collect();
        assert_eq!(calls, ["sessions", "workspaces"]);
        assert_ne!(log.entries()[0].seq, log.entries()[1].seq);
    }

    #[test]
    fn the_log_stops_at_the_cap() {
        let mut log = AuditLog::new();
        for _ in 0..(MAX_ENTRIES + 5) {
            log.record(entry("probe"));
        }
        assert_eq!(log.len(), MAX_ENTRIES);
    }

    #[test]
    fn an_expansion_follows_its_row_as_new_calls_arrive() {
        let mut log = AuditLog::new();
        log.record(entry("workspaces"));
        log.toggle(0);
        assert!(log.is_expanded(0));
        log.record(entry("sessions"));
        assert!(!log.is_expanded(0), "the new row took the front slot");
        assert!(
            log.is_expanded(1),
            "the expanded row moved down with its call"
        );
        log.toggle(1);
        assert!(!log.is_expanded(1), "toggling again folds it");
    }

    #[test]
    fn toggling_past_the_end_does_nothing() {
        let mut log = AuditLog::new();
        log.toggle(0);
        assert!(!log.is_expanded(0));
        log.record(entry("workspaces"));
        log.toggle(4);
        assert!(!log.is_expanded(0), "an index with no row expands nothing");
    }
}
