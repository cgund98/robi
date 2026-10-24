//! How far the open turn has got, and when to stop trusting the frames.
//!
//! The peer of the GUI's `phaseBySession` and `ActivityLine`: a phase, the
//! elapsed time, the animated dots, and the quiet-tick counter behind the
//! catch-up reload.
//!
//! **Everything is counted in ticks, not wall clock.** `update` has to stay a
//! pure function of state and a message, so `App` holds no `Instant`: one
//! [`TICK`] is 250 ms, a second is four ticks, and the catch-up is
//! [`CATCH_UP_TICKS`]. A test then asserts exact numbers instead of sleeping.

use std::time::Duration;

use crate::tui::state::Phase;

/// The pump's tick. The same value `tui::TICK` gives the `select!`.
pub const TICK: Duration = Duration::from_millis(250);

/// Ticks in a second, so the elapsed label does not drift from the real clock.
const TICKS_PER_SECOND: u64 = 1_000 / 250;

/// How long a busy phase may sit without a frame before the view reloads.
///
/// The GUI's `TRANSCRIPT_CATCH_UP_MS` is 2000; in ticks it is eight. The bus
/// drops a queued frame under backpressure, so a row can be stored while the
/// event that would paint it was thrown away.
pub const CATCH_UP_TICKS: u64 = 2_000 / 250;

/// How often the dots advance, in ticks. The GUI uses 400 ms.
const DOTS_EVERY: u64 = 2;

/// How many dots the animation cycles through.
const DOTS_COUNT: u64 = 3;

/// Cells in the footer's progress widget.
///
/// Eight is enough to read as a moving head at a glance and short enough to sit
/// at the right edge of a narrow terminal next to the hint.
pub const PROGRESS_CELLS: usize = 8;

/// The kind of delta a frame carried, as far as the phase is concerned.
///
/// `delta_json` in `domain/events` writes these names. Anything else
/// (tool-call arguments, usage, a finished body) leaves the phase alone: those
/// are stored rows, and their text comes from a refetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaKind {
    /// Model text: the reply is being written.
    Text,
    /// A reasoning trace: still thinking.
    Reasoning,
    /// Any other delta.
    Other,
}

impl DeltaKind {
    /// The kind named by a `message_delta` frame's `data.delta.kind`.
    pub fn from_name(name: Option<&str>) -> Self {
        match name {
            Some("text") => Self::Text,
            Some("reasoning") => Self::Reasoning,
            _ => Self::Other,
        }
    }
}

/// The open turn's phase, elapsed time, and quiet ticks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Activity {
    phase: Phase,
    /// Ticks since the turn started, used for elapsed time and the dots.
    ticks: u64,
    /// Ticks since the last frame for the open session.
    quiet: u64,
}

impl Activity {
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Whether a turn is in flight.
    pub fn is_busy(&self) -> bool {
        self.phase != Phase::Idle
    }

    /// Whether the turn is paused on a tool decision. T2 opens the modal; T1
    /// only reports it.
    pub fn start(&mut self, phase: Phase) {
        if self.phase == Phase::Idle && phase != Phase::Idle {
            self.ticks = 0;
        }
        self.phase = phase;
        self.quiet = 0;
    }

    /// Back to idle, for `turn_finished`, a stop, or a session switch.
    pub fn idle(&mut self) {
        self.phase = Phase::Idle;
        self.ticks = 0;
        self.quiet = 0;
    }

    /// A delta moved the phase. Text means the reply is arriving; reasoning
    /// means it is still being thought about.
    pub fn note(&mut self, kind: DeltaKind) {
        let phase = match kind {
            DeltaKind::Text => Phase::Responding,
            DeltaKind::Reasoning => Phase::Thinking,
            DeltaKind::Other => return,
        };
        if !self.is_busy() {
            self.ticks = 0;
        }
        self.phase = phase;
        self.frame();
    }

    /// A frame for the open session landed, so the catch-up is not due.
    pub fn frame(&mut self) {
        self.quiet = 0;
    }

    /// One 250 ms tick. Returns whether the catch-up reload is due.
    pub fn tick(&mut self) -> bool {
        if !self.is_busy() {
            return false;
        }
        self.ticks += 1;
        self.quiet += 1;
        self.catch_up_due()
    }

    /// Whether the quiet stretch has reached the catch-up threshold.
    ///
    /// Separate from [`Self::tick`] so the reload can reset the counter without
    /// waiting for a frame that the bus may have dropped.
    pub fn catch_up_due(&self) -> bool {
        self.is_busy() && self.quiet >= CATCH_UP_TICKS
    }

    /// The catch-up was served: start the quiet stretch again.
    pub fn caught_up(&mut self) {
        self.quiet = 0;
    }

    /// Ticks since the turn started. A test asserts the elapsed label from it.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Whole seconds since the turn started.
    pub fn elapsed(&self) -> u64 {
        self.ticks / TICKS_PER_SECOND
    }

    /// `Thinking`, `Responding`, or nothing when idle.
    pub fn label(&self) -> Option<&'static str> {
        match self.phase {
            Phase::Idle => None,
            Phase::Thinking => Some("Thinking"),
            Phase::Responding => Some("Responding"),
        }
    }

    /// How many dots the animation shows right now: 1, 2, 3, then around.
    pub fn dots(&self) -> usize {
        ((self.ticks / DOTS_EVERY) % DOTS_COUNT + 1) as usize
    }

    /// The header's activity text, or nothing when idle.
    pub fn summary(&self) -> Option<String> {
        let label = self.label()?;
        Some(format!("{label} \u{b7} {}", format_elapsed(self.elapsed())))
    }

    /// Which cell of the footer's progress widget is lit, or nothing when idle.
    ///
    /// The model reports no percentage, so the widget is an indeterminate
    /// marquee: it says "still going" by moving. One cell every two ticks, the
    /// same pace as the header's dots, so a full pass takes four seconds.
    pub fn progress(&self) -> Option<usize> {
        if !self.is_busy() {
            return None;
        }
        Some(((self.ticks / DOTS_EVERY) as usize) % PROGRESS_CELLS)
    }
}

/// Seconds as the GUI's `turnDuration.ts::formatElapsed` writes them.
///
/// `12s`, `2m`, `2m 5s`. One implementation for one fact, so a duration in the
/// terminal header and one in the desktop transcript read the same.
pub fn format_elapsed(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    let rest = seconds % 60;
    if rest == 0 {
        format!("{minutes}m")
    } else {
        format!("{minutes}m {rest}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one test here: the catch-up boundary. Everything else in this type
    /// is exercised through `App`, so a second unit test would only be a
    /// second place to edit when a label moves.
    #[test]
    fn the_catch_up_fires_after_eight_quiet_ticks_and_not_before() {
        let mut activity = Activity::default();
        activity.start(Phase::Thinking);
        for _ in 0..CATCH_UP_TICKS - 1 {
            assert!(!activity.tick(), "the catch-up is not due yet");
        }
        assert!(activity.tick(), "the eighth quiet tick is the boundary");
        assert_eq!(activity.elapsed(), 2, "eight ticks is two seconds");
        assert_eq!(activity.summary().as_deref(), Some("Thinking \u{b7} 2s"));

        // A frame resets the quiet stretch; elapsed time keeps running.
        activity.frame();
        assert!(!activity.catch_up_due());

        // A delta moves the phase and counts as a frame.
        activity.note(DeltaKind::from_name(Some("text")));
        assert_eq!(activity.phase(), Phase::Responding);
        assert!(!activity.catch_up_due());

        // An idle turn never asks for a reload.
        activity.idle();
        for _ in 0..CATCH_UP_TICKS * 2 {
            assert!(!activity.tick());
        }

        // The duration label mirrors `turnDuration.ts`.
        assert_eq!(format_elapsed(12), "12s");
        assert_eq!(format_elapsed(125), "2m 5s");
    }
}
