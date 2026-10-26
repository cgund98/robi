//! The pause: which calls are waiting, and what the user has said about them.
//!
//! The GUI disables a composer while a turn is paused; a terminal has no such
//! affordance, so the modal owns the keyboard instead. That is the whole
//! invariant of this module: as long as a call is pending, nothing typed reaches
//! the composer.
//!
//! # One decision at a time
//!
//! The runtime settles exactly one call and then resumes the turn, which pauses
//! again on the next pending call. So "approve all" is a queue here rather than a
//! burst of requests: the modal records the ids that were pending when the key
//! was pressed, and issues the next one when the next `awaiting_approval` frame
//! names it. A call from a later iteration is not in the set and gets its own
//! modal.

use std::collections::BTreeSet;

use ratatui_textarea::TextArea;
use robi_core::ids::ToolCallId;

use crate::tui::state::tool_card::{self, Summary};

/// What the user decided about one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Run it.
    Approve,
    /// Refuse it. `reason` reaches the model; `None` lets the service supply its
    /// default, which is what an empty reason field means.
    Reject { reason: Option<String> },
}

impl Decision {
    /// The word the service takes, which is also what the wire carries.
    pub fn word(&self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Reject { .. } => "reject",
        }
    }

    pub fn reason(&self) -> Option<String> {
        match self {
            Self::Approve => None,
            Self::Reject { reason } => reason.clone(),
        }
    }

    pub fn is_approve(&self) -> bool {
        matches!(self, Self::Approve)
    }
}

/// The decisions still owed after the current one: approve-all, or decline-all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    pub decision: Decision,
    pub remaining: BTreeSet<ToolCallId>,
}

/// The paused turn.
#[derive(Debug)]
pub struct Approval {
    /// Every call still waiting, in the order the frames named them.
    pending: Vec<ToolCallId>,
    /// Which of them the keyboard is on.
    selected: usize,
    /// A decision is in flight. The modal refuses a second one until it lands.
    busy: bool,
    /// Decisions still owed, from `A` or `Ctrl-C`.
    queue: Option<Queued>,
    /// The reason buffer.
    ///
    /// It lives here rather than on the prompt because it has to outlive it:
    /// closing the field and reopening it must not lose what was typed. It is
    /// per pause, not per call — one objection usually covers the batch.
    pub reason: TextArea<'static>,
}

impl Default for Approval {
    fn default() -> Self {
        Self::new()
    }
}

impl Approval {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            selected: 0,
            busy: false,
            queue: None,
            reason: text_field(),
        }
    }

    /// Add a waiting call. A repeat is not a second call.
    pub fn push(&mut self, call: ToolCallId) {
        if !self.pending.contains(&call) {
            self.pending.push(call);
        }
    }

    /// Settle one call locally, keeping the selection on something real.
    pub fn remove(&mut self, call: ToolCallId) {
        if let Some(index) = self.pending.iter().position(|pending| *pending == call) {
            self.pending.remove(index);
            self.selected = self.selected.min(self.pending.len().saturating_sub(1));
        }
        if let Some(queue) = &mut self.queue {
            queue.remaining.remove(&call);
        }
    }

    pub fn pending(&self) -> &[ToolCallId] {
        &self.pending
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_call(&self) -> Option<ToolCallId> {
        self.pending.get(self.selected).copied()
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
    }

    pub fn next(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.pending.len() - 1);
    }

    pub fn previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Arm a run of decisions over the calls currently listed.
    ///
    /// The set is frozen now on purpose: a call the next iteration asks for is
    /// not one of these, and silently approving it would defeat the pause.
    pub fn queue_for_all(&mut self, decision: Decision) {
        let remaining = self
            .pending
            .iter()
            .copied()
            .filter(|call| Some(*call) != self.selected_call())
            .collect();
        self.queue = Some(Queued {
            decision,
            remaining,
        });
    }

    /// The next decision this queue owes for `call`, if any.
    ///
    /// `awaiting_approval` calls this: it is the only place a queued decision is
    /// released, because it is the only frame that means the runtime is idle.
    pub fn take_queued(&mut self, call: ToolCallId) -> Option<Decision> {
        let queue = self.queue.as_mut()?;
        if !queue.remaining.remove(&call) {
            return None;
        }
        let decision = queue.decision.clone();
        if queue.remaining.is_empty() {
            self.queue = None;
        }
        Some(decision)
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.selected = 0;
        self.busy = false;
        self.queue = None;
        self.reason = text_field();
    }

    /// The reason as it would be sent: empty means "no reason", not a blank one.
    pub fn reason_text(&self) -> Option<String> {
        let text = self.reason.lines().join("\n");
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    }

    /// The label over the reason field: what is about to be refused.
    ///
    /// A call the transcript has not loaded yet still gets a label, because the
    /// field opens the moment the pause arrives and the reload lands a frame
    /// later.
    pub fn reason_label(&self, call: Option<&robi_core::message::ToolCall>) -> String {
        let Some(call) = call else {
            return "Reject the call".to_owned();
        };
        let Summary { verb, target, .. } = tool_card::summary(call);
        if target.is_empty() {
            format!("Reject {verb}")
        } else {
            format!("Reject {verb} {target}")
        }
    }
}

/// The reason field: one line, no wrapping, no underline.
///
/// A one-line field because a rejection reason is a sentence. Growing it would
/// steal rows from the modal's list for no gain.
fn text_field() -> TextArea<'static> {
    let mut field = TextArea::default();
    field.set_cursor_line_style(ratatui::style::Style::default());
    field
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_frame_does_not_add_a_second_call() {
        let mut approval = Approval::new();
        let call = ToolCallId::new();
        approval.push(call);
        approval.push(call);
        assert_eq!(approval.len(), 1);
    }

    #[test]
    fn removing_the_last_selected_call_moves_the_selection_back() {
        let mut approval = Approval::new();
        let first = ToolCallId::new();
        let second = ToolCallId::new();
        approval.push(first);
        approval.push(second);
        approval.next();
        assert_eq!(approval.selected_call(), Some(second));

        approval.remove(second);
        assert_eq!(approval.selected_call(), Some(first));
        approval.remove(first);
        assert_eq!(approval.selected_call(), None);
        assert!(approval.is_empty());
    }

    #[test]
    fn approving_all_leaves_the_selected_call_out_of_the_queue() {
        let mut approval = Approval::new();
        let calls: Vec<ToolCallId> = (0..3).map(|_| ToolCallId::new()).collect();
        for call in &calls {
            approval.push(*call);
        }
        approval.next();
        approval.queue_for_all(Decision::Approve);

        // The selected call is approved directly, so the queue must not also
        // owe a decision for it.
        assert_eq!(approval.take_queued(calls[1]), None);
        assert_eq!(approval.take_queued(calls[0]), Some(Decision::Approve));
        assert_eq!(approval.take_queued(calls[2]), Some(Decision::Approve));
        assert_eq!(approval.take_queued(calls[2]), None, "the queue is spent");
    }

    #[test]
    fn a_call_from_a_later_iteration_is_not_swept_into_the_queue() {
        let mut approval = Approval::new();
        let first = ToolCallId::new();
        approval.push(first);
        approval.queue_for_all(Decision::Approve);

        let later = ToolCallId::new();
        approval.push(later);
        assert_eq!(
            approval.take_queued(later),
            None,
            "a call the user never saw must get its own modal"
        );
    }

    #[test]
    fn an_empty_reason_is_no_reason_and_whitespace_does_not_count() {
        let mut approval = Approval::new();
        assert_eq!(approval.reason_text(), None);
        approval.reason.insert_str("   ");
        assert_eq!(approval.reason_text(), None);
        approval.reason.insert_str("because");
        assert_eq!(approval.reason_text(), Some("because".to_owned()));
    }
}
