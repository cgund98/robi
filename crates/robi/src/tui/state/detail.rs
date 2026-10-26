//! One tool call, read in full.
//!
//! The transcript's card is a summary: six lines of output, four of a diff. This
//! is where the whole body is read — its own scroll, no follow lock, and `Esc`
//! back to the transcript with the selection where it was.
//!
//! It replaces the in-card expansion T2 shipped. That grew the band in place,
//! which meant an opened shell card could push the turn it belonged to off the
//! screen; a view of its own has the whole frame and can scroll.

use robi_core::message::ToolCall;

use crate::tui::state::tool_card::{self, CardLine};
use crate::tui::state::Phase;

/// How far one `PageUp` or `PageDown` moves, given the viewport.
///
/// A page, not a screenful: leaving one row of the previous page visible is what
/// makes a long read feel continuous.
pub fn page(viewport: usize) -> usize {
    viewport.saturating_sub(1).max(1)
}

/// A card, whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    /// The call being read, so a reload can rebuild this.
    pub call: robi_core::ids::ToolCallId,
    /// `# heading`, then the subject, then the body.
    pub lines: Vec<CardLine>,
    /// The first visible line.
    pub offset: usize,
}

impl Detail {
    /// Build the view for one call.
    ///
    /// `card` is taken in its expanded form — the point of opening it — and
    /// uncapped: `tool_card` caps at forty lines for the band, and this is the
    /// answer to "the rest of it". A shell or MCP result that was compressed on
    /// the way to the model is replaced by `original` when the fetch has landed.
    pub fn new(call: &ToolCall, phase: Phase, original: Option<&str>) -> Self {
        let card = tool_card::card(call, phase, true, None);
        // The heading and subject come from the short parse; the body is the
        // uncapped one. `card.lines` is only used when it is all there is.
        let mut lines = Vec::new();
        lines.push(CardLine {
            kind: tool_card::CardLineKind::Body,
            text: card.heading_line(),
        });
        if let Some(subject) = &card.subject {
            lines.push(CardLine {
                kind: tool_card::CardLineKind::Body,
                text: format!("{} {subject}", card.marker),
            });
        }
        lines.push(CardLine::body(""));
        match original {
            Some(text) => lines.extend(text.lines().map(CardLine::body)),
            // Uncapped, which is the whole point: the card stops at forty lines.
            None => lines.extend(tool_card::full_body(call, phase)),
        }
        Self {
            call: call.id,
            lines,
            offset: 0,
        }
    }

    /// Rebuild from a call that changed underneath an open view.
    pub fn refresh(&mut self, call: &ToolCall, phase: Phase, original: Option<&str>) {
        let offset = self.offset;
        let fresh = Detail::new(call, phase, original);
        self.lines = fresh.lines;
        self.clamp(offset, self.lines.len());
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The lines the view paints, from `offset` for at most `viewport`.
    pub fn visible(&self, viewport: usize) -> &[CardLine] {
        let end = (self.offset + viewport).min(self.lines.len());
        let start = self.offset.min(end);
        &self.lines[start..end]
    }

    /// The largest offset that still fills the viewport.
    fn max_offset(&self, viewport: usize) -> usize {
        self.lines.len().saturating_sub(viewport.max(1))
    }

    fn clamp(&mut self, offset: usize, lines: usize) {
        self.offset = offset.min(lines.saturating_sub(1));
    }

    /// Scroll by `delta` lines, clamped to the body.
    pub fn scroll_by(&mut self, delta: isize, viewport: usize) {
        let max = self.max_offset(viewport);
        self.offset = (self.offset as isize + delta).clamp(0, max as isize) as usize;
    }

    pub fn page_up(&mut self, viewport: usize) {
        self.scroll_by(-(page(viewport) as isize), viewport);
    }

    pub fn page_down(&mut self, viewport: usize) {
        self.scroll_by(page(viewport) as isize, viewport);
    }

    /// The top. No follow lock to rearm, unlike the transcript.
    pub fn top(&mut self) {
        self.offset = 0;
    }

    /// The last page.
    pub fn bottom(&mut self, viewport: usize) {
        self.offset = self.max_offset(viewport);
    }
}

#[cfg(test)]
mod tests {
    use robi_core::ids::ToolCallId;
    use robi_core::message::{ApprovalStatus, ExecutionStatus, ToolCall};

    use super::*;

    fn call(output: &str) -> ToolCall {
        ToolCall {
            id: ToolCallId::new(),
            name: "shell".to_owned(),
            args: serde_json::json!({ "command": "seq 100" }),
            args_error: None,
            approval_status: ApprovalStatus::Approved,
            execution_status: ExecutionStatus::Succeeded,
            result: Some(serde_json::json!({ "stdout": output })),
            error: None,
            truncation: None,
            provider_call_id: None,
            subagent: None,
            original_id: None,
        }
    }

    fn many() -> ToolCall {
        let output: String = (0..100).map(|i| format!("line {i}\n")).collect();
        call(&output)
    }

    #[test]
    fn the_detail_shows_more_than_the_band_and_opens_at_the_top() {
        let call = many();
        let detail = Detail::new(&call, Phase::Idle, None);
        // Heading, subject, blank, then the hundred lines.
        assert_eq!(detail.len(), 103);
        assert_eq!(detail.offset, 0);
        let head = detail.visible(5);
        assert_eq!(head[0].text, "# shell");
        assert_eq!(head[1].text, "$ seq 100");
    }

    #[test]
    fn scrolling_clamps_at_both_ends() {
        let call = many();
        let mut detail = Detail::new(&call, Phase::Idle, None);
        detail.scroll_by(-10, 10);
        assert_eq!(detail.offset, 0, "the top is the top");

        detail.bottom(10);
        assert_eq!(detail.offset, 93);
        detail.scroll_by(10, 10);
        assert_eq!(detail.offset, 93, "the bottom is the bottom");

        detail.top();
        assert_eq!(detail.offset, 0);
    }

    #[test]
    fn a_page_leaves_one_line_of_overlap() {
        assert_eq!(page(20), 19);
        assert_eq!(page(1), 1, "a one-row viewport still moves");
    }

    #[test]
    fn a_fetched_original_replaces_the_compressed_body() {
        let call = call("the compressed view");
        let detail = Detail::new(&call, Phase::Idle, Some("all of it"));
        let text: Vec<&str> = detail.lines.iter().map(|line| line.text.as_str()).collect();
        assert!(text.contains(&"all of it"), "{text:?}");
        assert!(!text.contains(&"the compressed view"), "{text:?}");
    }

    #[test]
    fn a_refresh_keeps_the_offset_inside_the_new_body() {
        let long = many();
        let mut detail = Detail::new(&long, Phase::Idle, None);
        detail.bottom(10);
        assert_eq!(detail.offset, 93);

        // The call comes back with a short body: the old offset cannot survive.
        let short = call("one line");
        detail.refresh(&short, Phase::Idle, None);
        assert!(detail.offset < detail.len(), "{detail:?}");
        assert!(!detail.is_empty());
    }
}
