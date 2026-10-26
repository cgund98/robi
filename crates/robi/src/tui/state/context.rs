//! The token meter: how full the context is, estimated the way the webview does.
//!
//! A port of `src/features/chat/contextUsage.ts`. The last message that reported
//! `usage.input` is the anchor; everything since it — and the draft — is estimated
//! at four characters to the token. A model reports tokens per turn and never a
//! running total, so an estimate is the only thing available, and it is the same
//! estimate on both front ends.
//!
//! **`pendingText` has no peer here.** The webview adds the text of a message it
//! has echoed but not yet had confirmed; the terminal's field is cleared on a send
//! and the row arrives on the bus, so there is no window to fill.

use robi_core::message::{Message, Usage};

/// Rough characters per token, for the uncounted tail.
const CHARS_PER_TOKEN: usize = 4;

/// What the meter shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Estimate {
    /// Prompt tokens from the latest report. `None` until a turn reports input.
    pub reported: Option<u64>,
    /// Characters since that report, over four.
    pub uncounted: u64,
    /// Reported input plus the uncounted tail. `None` without a report.
    pub used: Option<u64>,
    /// Share of the window, capped at 100. `None` without a report or a window.
    pub percent: Option<u64>,
    /// The last turn's own accounting.
    pub last_turn: Option<Usage>,
    /// The model's advertised window, when the caller knows it.
    pub window: Option<u64>,
}

/// Estimate the context from the transcript, the draft, and a window.
pub fn estimate(messages: &[Message], draft: &str, window: Option<u64>) -> Estimate {
    let window = window.filter(|window| *window > 0);
    // The newest report wins: messages arrive in order, so walk backwards.
    let anchor = messages
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, message)| {
            message
                .usage
                .filter(|usage| usage.input > 0)
                .map(|usage| (index, usage))
        });
    let Some((start, usage)) = anchor else {
        return Estimate {
            window,
            ..Estimate::default()
        };
    };

    let mut chars = draft.chars().count();
    for message in &messages[start..] {
        chars += message_chars(message);
    }
    let uncounted = (chars / CHARS_PER_TOKEN) as u64;
    let reported = usage.input;
    let used = reported + uncounted;
    Estimate {
        reported: Some(reported),
        uncounted,
        used: Some(used),
        percent: window.map(|window| (used.saturating_mul(100) / window).min(100)),
        last_turn: Some(usage),
        window,
    }
}

/// Tool results live on their own messages, so only this message's own text and
/// its calls count.
fn message_chars(message: &Message) -> usize {
    let mut chars = message.content.chars().count();
    for call in &message.tool_calls {
        chars += call.name.chars().count();
        chars += call.args.to_string().chars().count();
    }
    chars
}

/// A count as the band shows it: `999`, `12.3k`, `1.0M`.
///
/// Compact because the row is shared with the mode, the model and the effort; the
/// webview's `1,000,000` would eat a third of a narrow terminal.
pub fn format_tokens(count: u64) -> String {
    if count < 1_000 {
        return count.to_string();
    }
    if count < 1_000_000 {
        let thousands = count as f64 / 1_000.0;
        return if thousands < 10.0 {
            format!("{thousands:.1}k")
        } else {
            format!("{thousands:.0}k")
        };
    }
    let millions = count as f64 / 1_000_000.0;
    if millions < 10.0 {
        format!("{millions:.1}M")
    } else {
        format!("{millions:.0}M")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reported(input: u64, output: u64, cached: u64) -> Usage {
        Usage {
            input,
            output,
            cached,
        }
    }

    #[test]
    fn no_report_means_no_estimate() {
        let messages = vec![Message::user("hello"), Message::assistant("hi")];
        let estimate = estimate(&messages, "", Some(1_000_000));
        assert_eq!(estimate.used, None);
        assert_eq!(estimate.percent, None);
        assert_eq!(estimate.last_turn, None);
        assert_eq!(estimate.window, Some(1_000_000));
    }

    #[test]
    fn a_report_with_nothing_since_is_the_reported_number() {
        let mut message = Message::assistant("done");
        message.usage = Some(reported(1_000, 20, 0));
        let messages = vec![Message::user("go"), message];
        let estimate = estimate(&messages, "", Some(10_000));
        assert_eq!(estimate.reported, Some(1_000));
        // The two messages' own characters, over four.
        let chars = "go".len() + "done".len();
        assert_eq!(estimate.uncounted, (chars / 4) as u64);
        assert_eq!(estimate.used, Some(1_000 + (chars / 4) as u64));
        assert_eq!(estimate.percent, Some(10));
        assert_eq!(estimate.last_turn.map(|usage| usage.output), Some(20));
    }

    #[test]
    fn only_the_tail_after_the_report_is_counted() {
        let mut anchor = Message::assistant("anchored");
        anchor.usage = Some(reported(100, 0, 0));
        let mut messages = vec![
            Message::user("a very long message that precedes it"),
            anchor,
        ];
        messages.push(Message::assistant("tail"));
        let with_tail = estimate(&messages, "", None);
        let without_tail = estimate(&messages[..2], "", None);
        assert!(with_tail.uncounted > without_tail.uncounted);
        assert_eq!(
            with_tail.used.unwrap() - without_tail.used.unwrap(),
            ("tail".len() / 4) as u64
        );
    }

    #[test]
    fn the_draft_counts_and_a_zero_window_is_unknown() {
        let mut message = Message::assistant("done");
        message.usage = Some(reported(100, 0, 0));
        let messages = vec![message];
        let with_draft = estimate(&messages, "12345678", Some(0));
        assert_eq!(with_draft.window, None, "a zero window is no window");
        assert_eq!(with_draft.percent, None);
        assert_eq!(with_draft.uncounted, ("done".len() / 4 + 2) as u64);

        // And it is capped, so a runaway estimate cannot print 250%.
        let full = estimate(&messages, &"x".repeat(10_000), Some(1_000));
        assert_eq!(full.percent, Some(100));
    }

    #[test]
    fn tokens_are_compact() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_000), "1.0k");
        assert_eq!(format_tokens(12_345), "12k");
        assert_eq!(format_tokens(214_000), "214k");
        assert_eq!(format_tokens(1_000_000), "1.0M");
        assert_eq!(format_tokens(1_048_576), "1.0M");
        assert_eq!(format_tokens(12_000_000), "12M");
    }
}
