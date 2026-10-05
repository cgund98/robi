//! The compaction seam: where a transcript may cut, and the token estimate the
//! meter and the auto trigger share.
//!
//! The cut and the estimate are pure functions. The summary call, the rewrite,
//! and the route live in `crates/robi`; the loop only decides *when* to ask.
//! See `docs/src/design/workspace/context-management.md`.

use std::collections::HashSet;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::ids::{MessageId, SessionId};
use crate::message::{Message, Role};

/// The share of the window above which a user turn auto-compacts.
pub const AUTO_THRESHOLD_PERCENT: u64 = 80;
/// The share of the window the kept tail aims to stay under.
pub const TAIL_TARGET_PERCENT: u64 = 50;

/// Where a compact may cut. `prefix` is summarized. `tail` is kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Cut {
    pub prefix: Vec<Message>,
    pub tail: Vec<Message>,
}

/// What called a compact. The trigger only changes the threshold, not the cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactTrigger {
    /// At the start of a user turn, when the estimate reaches the threshold.
    Auto,
    /// From the context-meter popover, regardless of the threshold.
    Manual,
}

/// What a compact did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactOutcome {
    /// The transcript was rewritten; the summary carries this id.
    Compacted { message: MessageId },
    /// Auto only: the estimate is below the threshold.
    BelowThreshold,
    /// The prefix would be empty: there is nothing older than the current turn.
    Nothing,
}

/// A compaction failure. The transcript is unchanged whenever this is returned.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CompactError(pub String);

/// Summarizes an older prefix and rewrites the transcript.
#[async_trait]
pub trait Compactor: Send + Sync {
    /// Summarize and rewrite, according to `trigger`.
    ///
    /// `window` is the model's advertised context window, when it has one. A
    /// missing window never auto-compacts; a manual compact still runs.
    async fn compact(
        &self,
        session: SessionId,
        window: Option<u64>,
        trigger: CompactTrigger,
        cancel: &CancellationToken,
    ) -> Result<CompactOutcome, CompactError>;
}

/// Same figure the meter uses, without the composer draft.
///
/// The latest assistant `usage.input` is the prompt size through the request
/// that reported it, so earlier messages are not counted again. Every message
/// from that one on, including it, is estimated as characters over four. With
/// no report at all, the whole transcript is.
pub fn estimate_tokens(messages: &[Message]) -> u64 {
    let report = messages
        .iter()
        .rposition(|message| message.role == Role::Assistant && reports_input(message));
    match report {
        Some(index) => {
            let usage = messages[index]
                .usage
                .expect("the reporting message carries usage");
            usage.input + transcript_chars(&messages[index..]) / 4
        }
        None => transcript_chars(messages) / 4,
    }
}

/// Whether a user turn should auto-compact. A missing window is never over.
pub fn over_auto_threshold(messages: &[Message], window: Option<u64>) -> bool {
    let Some(window) = window else {
        return false;
    };
    // `used >= 0.80 * window`, in integer math without overflow.
    u128::from(estimate_tokens(messages)) * 100
        >= u128::from(window) * u128::from(AUTO_THRESHOLD_PERCENT)
}

/// Where to cut so the kept tail is at most [`TAIL_TARGET_PERCENT`] of the
/// window.
///
/// `None` when the prefix would be empty. The current turn is never in the
/// prefix: the boundary is at or before the last user message.
pub fn plan_cut(messages: &[Message], context_window: u64) -> Option<Cut> {
    let mut chosen: Option<usize> = None;
    let mut first_legal: Option<usize> = None;
    for boundary in user_boundaries(messages) {
        if !is_legal_boundary(messages, boundary) {
            continue;
        }
        if first_legal.is_none() {
            first_legal = Some(boundary);
        }
        if tail_within(messages, boundary, context_window) {
            chosen = Some(boundary);
        } else {
            // Older tails only grow, so this and every older boundary fail.
            break;
        }
    }

    // No boundary was under the target. Keep the newest legal tail whole.
    let boundary = chosen.or(first_legal)?;
    cut_at(messages, boundary)
}

/// Cut at the newest user-message boundary, keeping only the current turn.
///
/// Used for a manual compact when the model advertises no window, so there is
/// no figure to target.
pub fn plan_last_turn(messages: &[Message]) -> Option<Cut> {
    let boundary = user_boundaries(messages)
        .into_iter()
        .find(|boundary| is_legal_boundary(messages, *boundary))?;
    cut_at(messages, boundary)
}

/// Boundaries newest-first: every user index that is not the transcript start.
fn user_boundaries(messages: &[Message]) -> Vec<usize> {
    let mut boundaries: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.role == Role::User)
        .map(|(index, _)| index)
        .collect();
    boundaries.reverse();
    boundaries
}

/// A boundary is illegal when it would separate an assistant message that has
/// tool calls from the tool messages that answer them.
fn is_legal_boundary(messages: &[Message], boundary: usize) -> bool {
    let answered: HashSet<_> = messages[boundary..]
        .iter()
        .filter(|message| message.role == Role::Tool)
        .filter_map(|message| message.tool_call_id)
        .collect();
    if answered.is_empty() {
        return true;
    }
    !messages[..boundary]
        .iter()
        .flat_map(|message| message.tool_calls.iter())
        .any(|call| answered.contains(&call.id))
}

/// The tail at `boundary` is at most half the window.
fn tail_within(messages: &[Message], boundary: usize, context_window: u64) -> bool {
    let tail_tokens = transcript_chars(&messages[boundary..]) / 4;
    u128::from(tail_tokens) * 100 <= u128::from(context_window) * u128::from(TAIL_TARGET_PERCENT)
}

/// Split at `boundary`, or `None` when that leaves an empty prefix.
fn cut_at(messages: &[Message], boundary: usize) -> Option<Cut> {
    if boundary == 0 {
        return None;
    }
    Some(Cut {
        prefix: messages[..boundary].to_vec(),
        tail: messages[boundary..].to_vec(),
    })
}

fn reports_input(message: &Message) -> bool {
    message.usage.is_some_and(|usage| usage.input > 0)
}

/// Characters over every message, the way the meter counts them.
fn transcript_chars(messages: &[Message]) -> u64 {
    messages.iter().map(message_chars).sum()
}

/// Content plus each tool call's name and serialized arguments.
fn message_chars(message: &Message) -> u64 {
    let mut chars = message.content.len() as u64;
    for call in &message.tool_calls {
        chars += call.name.len() as u64;
        chars += serde_json::to_string(&call.args)
            .map(|args| args.len() as u64)
            .unwrap_or(0);
    }
    chars
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{ToolCall, Usage};

    fn user(text: &str) -> Message {
        Message::user(text)
    }

    fn assistant(text: &str) -> Message {
        Message::assistant(text)
    }

    /// ~`chars` characters of content, so the estimate is predictable.
    fn sized(chars: usize) -> String {
        "x".repeat(chars)
    }

    fn reports(message: Message, input: u64) -> Message {
        message.with_usage(Usage {
            input,
            output: 0,
            cached: 0,
        })
    }

    #[test]
    fn with_no_usage_the_estimate_is_the_whole_transcript_over_four() {
        let messages = vec![user(&sized(400)), assistant(&sized(400))];
        assert_eq!(estimate_tokens(&messages), 200);
    }

    #[test]
    fn the_estimate_includes_the_reporting_message() {
        // 400 chars before the report are already in `input`; the reporting
        // message and everything after it are counted again.
        let messages = vec![
            user(&sized(400)),
            reports(assistant(&sized(8)), 1_000),
            user(&sized(8)),
        ];
        // input 1000 + (8 + 8) / 4 = 1004.
        assert_eq!(estimate_tokens(&messages), 1_004);
    }

    #[test]
    fn a_zero_input_report_is_ignored() {
        let messages = vec![reports(assistant(&sized(40)), 0), user(&sized(40))];
        assert_eq!(estimate_tokens(&messages), 20);
    }

    #[test]
    fn tool_call_arguments_count_toward_the_estimate() {
        let call = ToolCall::new("read_file", serde_json::json!({"path": "a"}));
        let mut message = assistant("");
        message.tool_calls = vec![call];
        // name "read_file" (9) + args `{"path":"a"}` (12) = 21 chars.
        assert_eq!(estimate_tokens(&[message]), 21 / 4);
    }

    #[test]
    fn a_boundary_that_splits_a_tool_round_is_illegal() {
        // A user message between an assistant call and its tool result would
        // let the boundary fall inside the round.
        let call = ToolCall::new("read_file", serde_json::json!({}));
        let messages = vec![
            user("u0"),
            Message::assistant_with_tool_calls("", vec![call.clone()]),
            user("u1"),
            Message::tool_result(call.id, "result"),
        ];
        // Boundary 2 is the user index inside the round: the tail's tool result
        // answers a call in the prefix, so it is illegal.
        assert!(!is_legal_boundary(&messages, 2));
        assert!(is_legal_boundary(&messages, 0));
    }

    #[test]
    fn plan_cut_keeps_the_oldest_boundary_under_half_the_window() {
        // u0 is 800 chars; every other turn is 400. Window 600 -> half is 300
        // tokens. A tail from boundary 2 is 200 tokens (under); from boundary 0
        // it is the whole 400-token transcript (over).
        let messages = vec![
            user(&sized(800)),
            assistant(&sized(0)),
            user(&sized(400)),
            assistant(&sized(0)),
            user(&sized(400)),
            assistant(&sized(0)),
        ];
        let cut = plan_cut(&messages, 600).expect("older turns are compactable");
        // Oldest boundary whose tail is under 300 tokens is index 2.
        assert_eq!(cut.prefix.len(), 2);
        assert_eq!(cut.tail.len(), 4);
        assert_eq!(cut.tail[0], messages[2]);
    }

    #[test]
    fn plan_cut_keeps_the_last_turn_whole_when_it_is_over_half() {
        let messages = vec![user(&sized(10)), assistant(&sized(10)), user(&sized(4_000))];
        let cut = plan_cut(&messages, 100).expect("the first turns are still compactable");
        // The last turn alone is over half, so the newest legal tail is kept.
        assert_eq!(cut.prefix.len(), 2);
        assert_eq!(cut.tail.len(), 1);
        assert_eq!(cut.tail[0], messages[2]);
    }

    #[test]
    fn plan_cut_is_none_when_the_only_turn_starts_the_transcript() {
        let messages = vec![user(&sized(4_000))];
        assert!(plan_cut(&messages, 100).is_none());
    }

    #[test]
    fn plan_cut_is_none_without_a_user_message() {
        let messages = vec![assistant(&sized(4_000))];
        assert!(plan_cut(&messages, 100).is_none());
    }

    #[test]
    fn plan_last_turn_cuts_at_the_newest_user_boundary() {
        let messages = vec![user("a"), assistant("b"), user("c"), assistant("d")];
        let cut = plan_last_turn(&messages).expect("there is an older turn");
        assert_eq!(cut.prefix.len(), 2);
        assert_eq!(cut.tail.len(), 2);
        assert_eq!(cut.tail[0], messages[2]);
    }

    #[test]
    fn plan_last_turn_is_none_for_a_single_turn() {
        let messages = vec![user("a"), assistant("b")];
        assert!(plan_last_turn(&messages).is_none());
    }

    #[test]
    fn the_auto_threshold_is_reached_at_eighty_percent() {
        // 100 tokens used. Window 125 -> 80%. Window 126 -> under.
        let messages = vec![reports(assistant(&sized(0)), 100)];
        assert!(over_auto_threshold(&messages, Some(125)));
        assert!(!over_auto_threshold(&messages, Some(126)));
        assert!(!over_auto_threshold(&messages, None));
    }
}
