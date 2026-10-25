//! The transcript as rows, and the follow lock.
//!
//! Everything here is a pure function of the messages and a width, so the
//! wrapping, the turn grouping, and the scroll arithmetic are unit-tested
//! without a terminal.
//!
//! **The rows are pre-wrapped.** [`ratatui::widgets::Paragraph`] counts rows
//! *after* it wraps, so a scroll offset expressed in its coordinates needs
//! `Paragraph::line_count`, which is behind `unstable-rendered-line-info`.
//! Wrapping here instead makes the offset exact, the follow lock trivial, and
//! the wrap itself testable.

use std::collections::{BTreeMap, BTreeSet};

use ratatui::style::Style;
use robi_core::ids::{MessageId, ToolCallId};
use robi_core::message::ExecutionStatus;
use robi_core::message::{Message, Role};
use unicode_width::UnicodeWidthChar;

use crate::tui::state::markdown;
use crate::tui::state::tool_card::{self, CardLine, CardLineKind};
use crate::tui::state::Phase;
use crate::tui::theme;

/// What one row is, which decides its style and its alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// A line of a user message's band: a raised surface with a blue rail.
    User,
    /// Assistant prose.
    Assistant,
    /// The heading row of a tool call's card: the call's description, or the
    /// tool's name. Muted, so it does not compete with the assistant's prose.
    ToolHeading,
    /// One body or subject line of a tool call's card.
    ToolCard,
    /// A line an edit adds. Tinted so a diff reads as a change, not prose.
    DiffAdd,
    /// A line an edit removes.
    DiffDel,
    /// A full-width divider, such as **Context compacted**.
    Divider,
    /// A blank line between turns.
    Spacer,
}

/// One selectable unit of the transcript: a message's band, or one of its calls.
///
/// A block is what the arrow keys move over, and what a key acts on. It is
/// addressed by message id rather than by row offset so a selection survives a
/// rewrap, a reload, or a row arriving underneath it. `call` is `None` for the
/// message's own prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockRef {
    pub message: MessageId,
    pub call: Option<ToolCallId>,
}

impl BlockRef {
    /// The prose band of a message.
    pub fn message(message: MessageId) -> Self {
        Self {
            message,
            call: None,
        }
    }

    /// One tool call's card inside its message.
    pub fn call(message: MessageId, call: ToolCallId) -> Self {
        Self {
            message,
            call: Some(call),
        }
    }
}

/// One visual line of the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    /// The block this row belongs to, when it belongs to one.
    ///
    /// A block is many rows; the selection moves block to block, and `reveal`
    /// finds one by its id, so the association is kept on the row rather than
    /// recomputed from the messages.
    pub block: Option<BlockRef>,
    /// The call this row belongs to is waiting on the user.
    ///
    /// Painted as an orange rail down the card's left edge, so a pause is visible
    /// without a modal and without being selected. Not derived in the view: the
    /// decision is a fact about the call, and the row is where the card already
    /// knows it.
    pub attention: bool,
    /// The whole line, which is what the width arithmetic counts.
    pub text: String,
    /// Styled byte ranges of `text`, sorted and non-overlapping.
    ///
    /// Empty means the view paints the row in the one tone its `RowKind` names.
    /// A tool card's `$` marker and every markdown span set their own: the
    /// range is into `text`, not extra text, so the row's width is the same
    /// however it is painted.
    pub spans: Vec<(std::ops::Range<usize>, Style)>,
}

impl Row {
    fn new(kind: RowKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            block: None,
            attention: false,
            text: text.into(),
            spans: Vec::new(),
        }
    }

    /// Attach the row to the block it belongs to.
    fn in_block(mut self, block: BlockRef) -> Self {
        self.block = Some(block);
        self
    }

    /// Mark the row as part of a call that is waiting on the user.
    fn attention(mut self, attention: bool) -> Self {
        self.attention = attention;
        self
    }

    /// One styled row: the text and the runs of tone inside it.
    pub(crate) fn styled(
        kind: RowKind,
        text: String,
        spans: Vec<(std::ops::Range<usize>, Style)>,
    ) -> Self {
        Self {
            kind,
            block: None,
            attention: false,
            text,
            spans,
        }
    }

    /// A row with one span drawn in `style`. `start` is a byte offset into
    /// `text`, `len` the span's length: the marker's, on a card's subject line.
    fn with_span(
        kind: RowKind,
        start: usize,
        len: usize,
        style: Style,
        text: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            block: None,
            attention: false,
            text: text.into(),
            spans: vec![(start..start + len, style)],
        }
    }

    fn spacer() -> Self {
        Self::new(RowKind::Spacer, "")
    }
}

/// The label the compaction divider shows.
pub const COMPACTION_LABEL: &str = "Context compacted";

/// How close to the last row still counts as following it.
///
/// The GUI's `FOLLOW_THRESHOLD` is 48 pixels, about three lines of prose. In
/// cells it is three rows.
pub const FOLLOW_THRESHOLD: usize = 3;

/// How many output lines a collapsed tool card shows before it truncates.
///
/// The card is a summary, not a reader. Shell output and file bodies run to
/// thousands of lines, and the transcript has to stay scrollable; `retrieve`
/// is the way back to the whole thing.
pub const MAX_TOOL_OUTPUT_LINES: usize = tool_card::MAX_TOOL_OUTPUT_LINES;

/// Cells of padding at each edge of a card's band.
const PAD_X: usize = 1;

/// The vertical rail on a band's left edge. One column, on every row of the
/// band, so the line is continuous from the top padding to the bottom.
pub const RAIL_CHAR: char = '\u{2502}';

/// Cells the rail takes out of the content width, with the padding.
fn inner_width(width: usize) -> usize {
    width.saturating_sub(1 + PAD_X * 2)
}

/// One row of a band: the rail, padding, the text, and the fill to full width.
///
/// The rail is part of the row's text, so a band row is exactly `width` cells
/// however it is styled; the view paints the first cell separately.
fn band_line(text: &str, inner: usize) -> String {
    let pad = inner.saturating_sub(display_width(text));
    format!(
        "{RAIL_CHAR}{}{text}{}",
        " ".repeat(PAD_X),
        " ".repeat(PAD_X + pad)
    )
}

/// What the transcript renders beyond the messages themselves.
///
/// The expansion set is the persisted half: `~/.robi/tui.json` holds one key per
/// opened call. The originals are not persisted — they are fetched when a card
/// opens, because the expanded shell card shows the uncapped output the model
/// did not read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Render {
    /// Opened cards, by call id. Shared with `ViewState`.
    pub expanded: BTreeSet<String>,
    /// The uncompressed body of a compressed result, once fetched.
    pub originals: BTreeMap<ToolCallId, String>,
}

impl Render {
    pub fn is_expanded(&self, call: ToolCallId) -> bool {
        self.expanded.contains(&call.to_string())
    }

    pub fn set_expanded(&mut self, call: ToolCallId, expanded: bool) {
        let key = call.to_string();
        if expanded {
            self.expanded.insert(key);
        } else {
            self.expanded.remove(&key);
        }
    }

    pub fn toggle(&mut self, call: ToolCallId) -> bool {
        let expanded = !self.is_expanded(call);
        self.set_expanded(call, expanded);
        expanded
    }
}

/// The open transcript: messages, their rows, and where the reader is.
#[derive(Debug, Clone, Default)]
pub struct Transcript {
    messages: Vec<Message>,
    rows: Vec<Row>,
    width: u16,
    /// The first visible row.
    offset: usize,
    /// How many rows the view has for the transcript.
    viewport: usize,
    /// Follow the last row as it grows.
    pinned: bool,
    phase: Phase,
    /// The generation the next reload is issued at. A `Transcript` outcome
    /// from an older one is dropped.
    generation: u64,
    /// Expansion flags and fetched originals.
    render: Render,
    /// The block the keyboard has selected, if any.
    ///
    /// Not persisted: a selection is a gesture, not a fact about the session.
    selection: Option<BlockRef>,
}

impl Transcript {
    pub fn new() -> Self {
        Self {
            pinned: true,
            ..Self::default()
        }
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn pinned(&self) -> bool {
        self.pinned
    }

    pub fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
    }

    /// Replace the messages and rebuild. Keeps the follow lock.
    pub fn set_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
        self.rebuild();
    }

    /// One stored row arrived: replace it by id, or append it.
    ///
    /// This is what `message_added` and `message_updated` land as, so the
    /// transcript never has to be rebuilt from a delta. A row that is already
    /// here is replaced in place, which keeps its position; a row that is not
    /// is the newest, so it goes at the end.
    pub fn upsert(&mut self, message: Message) {
        match self.messages.iter_mut().find(|row| row.id == message.id) {
            Some(existing) => *existing = message,
            None => self.messages.push(message),
        }
        self.rebuild();
    }

    /// Drop the messages, for a session switch.
    pub fn clear(&mut self) {
        self.messages.clear();
        self.rows.clear();
        self.offset = 0;
        self.pinned = true;
        self.phase = Phase::Idle;
    }

    /// The terminal width changed. Rewraps only when it actually moved.
    pub fn set_width(&mut self, width: u16) {
        if width == self.width {
            return;
        }
        self.width = width;
        self.rebuild();
    }

    /// The view's height changed, which changes the largest offset.
    pub fn set_viewport(&mut self, viewport: usize) {
        if viewport == self.viewport {
            return;
        }
        self.viewport = viewport.max(1);
        self.follow();
    }

    /// The next reload's generation.
    pub fn next_generation(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    fn rebuild(&mut self) {
        self.rows = build_rows_with(&self.messages, self.width, &self.render);
        // A block that vanished cannot stay selected.
        if let Some(selected) = self.selection {
            if !self.rows.iter().any(|row| row.block == Some(selected)) {
                self.selection = None;
            }
        }
        self.follow();
    }

    /// The largest offset that still fills the viewport.
    fn max_offset(&self) -> usize {
        self.rows.len().saturating_sub(self.viewport)
    }

    /// Move to the last row when the reader is following.
    fn follow(&mut self) {
        if self.pinned {
            self.offset = self.max_offset();
        } else {
            self.offset = self.offset.min(self.max_offset());
        }
    }

    /// The rows the view paints.
    pub fn visible(&self) -> &[Row] {
        let end = (self.offset + self.viewport).min(self.rows.len());
        let start = self.offset.min(end);
        &self.rows[start..end]
    }

    /// Scroll by `delta` rows: negative is up, positive is down.
    pub fn scroll_by(&mut self, delta: isize) {
        let previous_offset = self.offset;
        let previous_rows = self.rows.len();
        let max = self.max_offset();
        let next = (self.offset as isize + delta).clamp(0, max as isize) as usize;
        self.offset = next;
        self.pinned = pinned_after_scroll(
            self.pinned,
            self.offset,
            previous_offset,
            self.rows.len(),
            previous_rows,
            self.viewport,
            FOLLOW_THRESHOLD,
        );
    }

    /// One screen up.
    pub fn page_up(&mut self) {
        self.scroll_by(-(self.viewport as isize));
    }

    /// One screen down.
    pub fn page_down(&mut self) {
        self.scroll_by(self.viewport as isize);
    }

    /// Half a screen up.
    pub fn half_page_up(&mut self) {
        self.scroll_by(-((self.viewport / 2) as isize));
    }

    /// Half a screen down.
    pub fn half_page_down(&mut self) {
        self.scroll_by((self.viewport / 2) as isize);
    }

    /// The first row.
    pub fn top(&mut self) {
        self.offset = 0;
        self.pinned = false;
    }

    /// The last row, which re-arms the follow lock.
    pub fn bottom(&mut self) {
        self.offset = self.max_offset();
        self.pinned = true;
    }

    // -- Expansion (dormant) ------------------------------------------------

    /// The render state, for the view and for `App` to mirror into `ViewState`.
    pub fn render(&self) -> &Render {
        &self.render
    }

    /// Adopt the persisted expansion set.
    ///
    /// **Dormant.** In-card expansion is replaced by the detail view, so nothing
    /// calls the mutators below; they and `Render.expanded` stay until that is a
    /// decision rather than a phase. Re-enabling is deleting these markers.
    #[allow(dead_code)]
    pub fn set_expanded(&mut self, expanded: BTreeSet<String>) {
        if self.render.expanded == expanded {
            return;
        }
        self.render.expanded = expanded;
        self.rebuild();
    }

    #[allow(dead_code)]
    pub fn is_expanded(&self, call: ToolCallId) -> bool {
        self.render.is_expanded(call)
    }

    /// Toggle one card and say what it became.
    #[allow(dead_code)]
    pub fn toggle_expanded(&mut self, call: ToolCallId) -> bool {
        let expanded = self.render.toggle(call);
        self.rebuild();
        expanded
    }

    /// Keep a fetched original for the detail view.
    pub fn set_original(&mut self, call: ToolCallId, text: String) {
        self.render.originals.insert(call, text);
        self.rebuild();
    }

    pub fn original(&self, call: ToolCallId) -> Option<&str> {
        self.render.originals.get(&call).map(String::as_str)
    }

    // -- Selection -----------------------------------------------------------

    /// The block the keyboard has selected.
    pub fn selection(&self) -> Option<BlockRef> {
        self.selection
    }

    /// The call the selection names, when the selected block is a card.
    pub fn selected_call(&self) -> Option<ToolCallId> {
        self.selection.and_then(|block| block.call)
    }

    /// Every block that has rows, in transcript order.
    fn blocks(&self) -> Vec<BlockRef> {
        let mut seen = BTreeSet::new();
        self.rows
            .iter()
            .filter_map(|row| row.block)
            .filter(|block| seen.insert(*block))
            .collect()
    }

    /// Move the selection by `delta` blocks.
    ///
    /// From nothing, `+1` selects the first block and `-1` the last: a reader who
    /// has never selected one reaches for the direction they are reading in. A
    /// walk stops at the ends rather than wrapping, so holding an arrow key does
    /// not teleport the reader across the turn.
    pub fn select(&mut self, delta: isize) {
        let blocks = self.blocks();
        if blocks.is_empty() {
            self.selection = None;
            return;
        }
        let current = self
            .selection
            .and_then(|selected| blocks.iter().position(|block| *block == selected));
        let next = match (current, delta.is_negative()) {
            (Some(index), false) => (index + delta as usize).min(blocks.len() - 1),
            (Some(index), true) => index.saturating_sub(delta.unsigned_abs()),
            (None, false) => 0,
            (None, true) => blocks.len() - 1,
        };
        self.select_block(blocks[next]);
    }

    /// Select a block by id, if it is still there, and bring it into view.
    pub fn select_block(&mut self, block: BlockRef) {
        if !self.rows.iter().any(|row| row.block == Some(block)) {
            return;
        }
        self.selection = Some(block);
        self.reveal(block);
    }

    /// Select the last block, which is what `Up` on an empty composer does: the
    /// reader is reaching for the newest thing without leaving the field.
    pub fn select_last(&mut self) -> Option<BlockRef> {
        let last = *self.blocks().last()?;
        self.select_block(last);
        Some(last)
    }

    /// Drop the selection, so the transcript keys are a no-op again.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Scroll the minimum distance that puts `block` in view.
    ///
    /// Minimal on purpose: revealing a block that is already visible must not
    /// jump the reader out of the prose they were reading. Reaching the last row
    /// re-arms the follow lock, the same rule `End` follows.
    pub fn reveal(&mut self, block: BlockRef) {
        let Some(start) = self.rows.iter().position(|row| row.block == Some(block)) else {
            return;
        };
        if start < self.offset {
            self.offset = start;
        } else if start >= self.offset + self.viewport {
            self.offset = (start + 1).saturating_sub(self.viewport);
        }
        if self.offset >= self.max_offset() {
            self.offset = self.max_offset();
            self.pinned = true;
        } else {
            self.pinned = false;
        }
    }
}

/// Rows between the reader and the end of the transcript. Zero means the last
/// row is in view.
pub fn distance_from_bottom(offset: usize, rows: usize, viewport: usize) -> usize {
    rows.saturating_sub(offset + viewport)
}

/// Whether the transcript should stay pinned after a scroll.
///
/// A port of `src/features/chat/transcriptFollow.ts::pinnedAfterScroll`. A
/// taller list, such as a tool call arriving, is not the reader leaving the
/// end; an upward move that abandons the end is.
///
/// The upward test is `offset < previous_offset`, **not** the GUI's
/// `offset + 1 < previous_offset`. The GUI counts pixels, where a one-pixel
/// move is a rounding artefact; here one row is a gesture, and an arrow key
/// that cannot release the follow lock would snap back on the next frame. A
/// move that stays within [`FOLLOW_THRESHOLD`] of the end has already returned
/// above, so this only fires when the reader really has left the end.
pub fn pinned_after_scroll(
    pinned: bool,
    offset: usize,
    previous_offset: usize,
    rows: usize,
    previous_rows: usize,
    viewport: usize,
    threshold: usize,
) -> bool {
    if distance_from_bottom(offset, rows, viewport) <= threshold {
        return true;
    }
    if pinned && rows > previous_rows + 1 {
        return true;
    }
    if offset < previous_offset {
        return false;
    }
    pinned
}

/// Turn grouping, then one row per visual line.
///
/// Mirrors `src/features/chat/Transcript.tsx::groupTurns`: a turn starts at a
/// `Role::User` message or at the first message. A compaction summary is a
/// user message, so it starts its own turn and paints as a divider.
///
/// Every message — and every tool call, which is its own block — is separated
/// from the next by one blank row. Separating in one place is what keeps the
/// spacing even: a block never supplies its own leading or trailing blank, so
/// two tool calls cannot end up two rows apart while a message pair is one.
pub fn build_rows(messages: &[Message], width: u16) -> Vec<Row> {
    build_rows_with(messages, width, &Render::default())
}

/// [`build_rows`] with the expansion set and the fetched originals.
pub fn build_rows_with(messages: &[Message], width: u16, render: &Render) -> Vec<Row> {
    let width = usize::from(width).max(1);
    // Every block is tagged with what it is, because the arrows move over blocks
    // and a key acts on the selected one. A compaction divider is not a block: it
    // has nothing to act on.
    let mut blocks: Vec<Vec<Row>> = Vec::new();
    for message in messages {
        let prose = BlockRef::message(message.id);
        match message.role {
            Role::User => {
                if message.compaction {
                    blocks.push(vec![Row::new(RowKind::Divider, COMPACTION_LABEL)]);
                    continue;
                }
                // A band like a tool card's, with a blue rail instead of gray.
                let inner = inner_width(width);
                let mut block = vec![Row::new(RowKind::User, band_line("", inner)).in_block(prose)];
                for line in wrap_text(&message.content, inner) {
                    block.push(Row::new(RowKind::User, band_line(&line, inner)).in_block(prose));
                }
                block.push(Row::new(RowKind::User, band_line("", inner)).in_block(prose));
                blocks.push(block);
            }
            Role::Assistant => {
                if !message.content.trim().is_empty() {
                    // Markdown, not raw text: the assistant writes headings,
                    // lists, tables and code, and a terminal that shows the
                    // source characters is showing the author's markup rather
                    // than the document.
                    blocks.push(
                        markdown::rows(&message.content, width)
                            .into_iter()
                            .map(|row| row.in_block(prose))
                            .collect(),
                    );
                }
                // One block per call, so each gets its own blank line and its own
                // place in the selection.
                for call in &message.tool_calls {
                    blocks.push(tool_card_rows(
                        call,
                        width,
                        render,
                        BlockRef::call(message.id, call.id),
                    ));
                }
            }
            // A tool result's body lives on the card above it.
            Role::Tool => {}
        }
    }

    let mut rows = Vec::new();
    for block in blocks {
        if block.is_empty() {
            continue;
        }
        if !rows.is_empty() {
            rows.push(Row::spacer());
        }
        rows.extend(block);
    }
    rows
}

/// One card per tool call: a raised band, no border.
///
/// A block rather than a bare line, because a call is a unit: the reader wants
/// to see what ran and what came back together. The band is the grouping — a
/// lighter background against the canvas — with no box-drawing characters, so
/// it reads as a surface rather than a table cell. Padding above and below sets
/// one command apart from the next.
///
/// The rows are generated here, in the pre-wrapped set, rather than drawn by
/// `Block` in the view: a card then still counts as exactly the rows it
/// occupies, which is what keeps the scroll offset row-exact.
///
/// What the card *says* is `tool_card`'s job. This only wraps it, tints it, and
/// tags every row with the call's block so the selection and `reveal` can find
/// it.
fn tool_card_rows(
    call: &robi_core::message::ToolCall,
    width: usize,
    render: &Render,
    block: BlockRef,
) -> Vec<Row> {
    let width = width.max(8);
    let inner = inner_width(width);
    let id = call.id;
    let expanded = render.is_expanded(id);
    let card = tool_card::card(call, expanded);
    // A call the loop is waiting on. The same predicate the modal, the pending
    // list, and `pending_calls` use.
    let attention = call.approval_status == robi_core::message::ApprovalStatus::Pending
        && call.execution_status == ExecutionStatus::NotStarted;
    let mut rows = Vec::new();

    // One row of padding at the top, inside the band.
    rows.push(
        Row::new(RowKind::ToolCard, band_line("", inner))
            .in_block(block)
            .attention(attention),
    );

    // The heading: `# what ran`. Muted, so it reads as a label.
    for line in wrap_text(&card.heading_line(), inner) {
        rows.push(
            Row::new(RowKind::ToolHeading, band_line(&line, inner))
                .in_block(block)
                .attention(attention),
        );
    }

    let body = body_lines(call, &card, render, expanded);

    // Everything below the heading is the contents, one blank row down.
    if card.subject.is_some() || !body.is_empty() {
        rows.push(
            Row::new(RowKind::ToolCard, band_line("", inner))
                .in_block(block)
                .attention(attention),
        );
    }

    if let Some(subject) = &card.subject {
        let marker = card.marker;
        let first = format!("{marker} {subject}");
        for (index, line) in wrap_text(&first, inner).into_iter().enumerate() {
            if index == 0 {
                // The marker gets its own, quieter tone. Its offset is where
                // the text starts inside the row's padding.
                let text = band_line(&line, inner);
                let start = text.find(&line).unwrap_or(0);
                rows.push(
                    Row::with_span(
                        RowKind::ToolCard,
                        start,
                        marker.len_utf8(),
                        theme::tool_card_marker(),
                        text,
                    )
                    .in_block(block)
                    .attention(attention),
                );
            } else {
                // Continuation lines line up under the text, not the marker.
                rows.push(
                    Row::new(RowKind::ToolCard, band_line(&format!("  {line}"), inner))
                        .in_block(block)
                        .attention(attention),
                );
            }
        }
    }

    if !body.is_empty() {
        if card.subject.is_some() {
            rows.push(
                Row::new(RowKind::ToolCard, band_line("", inner))
                    .in_block(block)
                    .attention(attention),
            );
        }
        for line in &body {
            // No indent: the output starts in the same column as `#` and `$`.
            let kind = match line.kind {
                CardLineKind::DiffAdd => RowKind::DiffAdd,
                CardLineKind::DiffDel => RowKind::DiffDel,
                _ => RowKind::ToolCard,
            };
            for wrapped in wrap_text(&line.text, inner) {
                rows.push(
                    Row::new(kind, band_line(&wrapped, inner))
                        .in_block(block)
                        .attention(attention),
                );
            }
        }
    }

    // One row of padding at the bottom, mirroring the top.
    rows.push(
        Row::new(RowKind::ToolCard, band_line("", inner))
            .in_block(block)
            .attention(attention),
    );
    rows
}

/// The card's body: the parsed lines, plus the fetched original when the card is
/// opened over a compressed result.
///
/// A shell or MCP result may have been compressed before it was stored, so the
/// transcript holds a summary the model read and the original is elsewhere. Only
/// an opened card pays for it, and the fetch itself belongs to `App`.
fn body_lines(
    call: &robi_core::message::ToolCall,
    card: &tool_card::Card,
    render: &Render,
    expanded: bool,
) -> Vec<CardLine> {
    if expanded && (call.name == "shell" || call.name.starts_with("mcp_")) {
        if let Some(original) = render.originals.get(&call.id) {
            let mut lines: Vec<CardLine> = original.lines().map(CardLine::body).collect();
            let limit = tool_card::MAX_EXPANDED_TOOL_LINES;
            if lines.len() > limit {
                let hidden = lines.len() - limit;
                lines.truncate(limit);
                lines.push(more_line(hidden));
            }
            return lines;
        }
    }
    let mut lines = card.lines.clone();
    if card.hidden > 0 {
        lines.push(more_line(card.hidden));
    }
    lines
}

fn more_line(hidden: usize) -> CardLine {
    CardLine {
        kind: CardLineKind::More,
        text: format!("\u{2026} (+{hidden} more lines)"),
    }
}

/// Cells, not chars: a wide character occupies two columns.
pub fn display_width(text: &str) -> usize {
    text.chars().filter_map(|c| c.width()).sum()
}

/// Wrap `text` to `width` cells.
///
/// Breaks at a space where one is available and hard-splits a token wider than
/// the whole row. Cells, not chars, so a wide character cannot overflow the
/// frame. No character is dropped except the single space a break replaces.
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for hard_line in text.split('\n') {
        let cells: Vec<(char, Style)> = hard_line.chars().map(|c| (c, Style::default())).collect();
        for chunk in split_cells(&cells, width) {
            rows.push(chunk.into_iter().map(|(c, _)| c).collect());
        }
    }
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

/// Split one line of characters into rows of at most `width` cells.
///
/// The one wrap algorithm: the plain-text path and the markdown path both go
/// through it, so a hard split behaves the same whether or not the line is
/// styled. Each character carries its own style so a wrapped row can carry the
/// tone of the source span it came from.
pub(crate) fn split_cells(cells: &[(char, Style)], width: usize) -> Vec<Vec<(char, Style)>> {
    let width = width.max(1);
    if cells.is_empty() {
        return vec![Vec::new()];
    }
    let mut rows = Vec::new();
    let mut row: Vec<(char, Style)> = Vec::new();
    let mut row_width = 0usize;
    let mut break_at: Option<usize> = None;
    for &(ch, style) in cells {
        let cell = ch.width().unwrap_or(0);
        while row_width + cell > width && !row.is_empty() {
            match break_at {
                Some(index) if index > 0 => {
                    rows.push(row[..index].to_vec());
                    let rest = row.split_off(index + 1);
                    row = rest;
                }
                _ => {
                    rows.push(std::mem::take(&mut row));
                }
            }
            row_width = row.iter().map(|(c, _)| c.width().unwrap_or(0)).sum();
            break_at = row.iter().position(|(c, _)| *c == ' ');
        }
        if ch == ' ' {
            break_at = Some(row.len());
        }
        row.push((ch, style));
        row_width += cell;
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

/// One row of styled text: the characters, and the runs of tone inside them.
///
/// Adjacent characters in the same style become one span, so a paragraph with
/// one bold word carries three spans and not one per character.
pub(crate) fn styled_row(
    cells: Vec<(char, Style)>,
) -> (String, Vec<(std::ops::Range<usize>, Style)>) {
    let mut text = String::new();
    let mut spans: Vec<(std::ops::Range<usize>, Style)> = Vec::new();
    for (ch, style) in cells {
        let start = text.len();
        text.push(ch);
        let end = text.len();
        match spans.last_mut() {
            Some((range, last)) if *last == style && range.end == start => range.end = end,
            _ => spans.push((start..end, style)),
        }
    }
    (text, spans)
}

#[cfg(test)]
mod tests {
    use robi_core::ids::ToolCallId;
    use robi_core::message::{ApprovalStatus, ExecutionStatus, ToolCall};

    use super::*;

    /// A row's text without the rail column: what the eye reads.
    fn content(row: &Row) -> &str {
        row.text.strip_prefix(RAIL_CHAR).unwrap_or(&row.text).trim()
    }

    fn texts(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(content).collect()
    }

    /// The rows grouped into blocks: a spacer is the boundary, not content.
    fn blocks(rows: &[Row]) -> Vec<Vec<Row>> {
        let mut blocks: Vec<Vec<Row>> = Vec::new();
        for row in rows {
            if row.kind == RowKind::Spacer {
                blocks.push(Vec::new());
                continue;
            }
            match blocks.last_mut() {
                Some(block) => block.push(row.clone()),
                None => blocks.push(vec![row.clone()]),
            }
        }
        blocks.retain(|block| !block.is_empty());
        blocks
    }

    /// A block's shape as one letter per row: which kinds it is made of.
    fn shape(block: &[Row]) -> String {
        block
            .iter()
            .map(|row| match row.kind {
                RowKind::User => 'u',
                RowKind::Assistant => 'a',
                RowKind::ToolHeading => 'h',
                RowKind::ToolCard => 'c',
                RowKind::DiffAdd => '+',
                RowKind::DiffDel => '-',
                RowKind::Divider => 'd',
                RowKind::Spacer => ' ',
            })
            .collect()
    }

    #[test]
    fn a_turn_starts_at_every_user_message() {
        let messages = vec![
            Message::user("first"),
            Message::assistant("a one"),
            Message::assistant("a two"),
            Message::user("second"),
            Message::assistant("a three"),
        ];
        let rows = build_rows(&messages, 40);
        // One block per message, one blank row between them.
        let groups = blocks(&rows);
        assert_eq!(
            groups.iter().map(|g| shape(g)).collect::<Vec<_>>(),
            vec!["uuu", "a", "a", "uuu", "a"],
            "a user message is a three-row band; an assistant message is prose"
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row.kind == RowKind::Spacer)
                .count(),
            4,
            "one blank line between each of the five messages, none before the first"
        );
        // Each band's middle row carries the text.
        assert_eq!(texts(&groups[0])[1], "first");
        assert_eq!(texts(&groups[3])[1], "second");
    }

    #[test]
    fn a_compaction_summary_starts_its_own_turn_as_a_divider() {
        let messages = vec![
            Message::user("early"),
            Message::assistant("reply"),
            Message::summary("what happened so far"),
            Message::user("later"),
        ];
        let rows = build_rows(&messages, 40);
        assert_eq!(
            blocks(&rows).iter().map(|g| shape(g)).collect::<Vec<_>>(),
            vec!["uuu", "a", "d", "uuu"],
            "each message is its own block, the divider included"
        );
        assert_eq!(texts(&blocks(&rows)[2]), vec![COMPACTION_LABEL]);
        let dividers: Vec<&Row> = rows
            .iter()
            .filter(|row| row.kind == RowKind::Divider)
            .collect();
        assert_eq!(dividers.len(), 1);
        assert_eq!(
            dividers[0].text, COMPACTION_LABEL,
            "the summary body is hidden; the divider is the whole turn"
        );
    }

    /// A call with a result, at the status the card header shows.
    fn call(name: &str, args: serde_json::Value, result: serde_json::Value) -> ToolCall {
        let mut call = ToolCall::new(name, args);
        call.approval_status = ApprovalStatus::Approved;
        call.execution_status = ExecutionStatus::Succeeded;
        call.result = Some(result);
        call
    }

    #[test]
    fn a_tool_call_renders_as_a_band_without_a_border() {
        let call = call(
            "read_file",
            serde_json::json!({ "path": "a.rs" }),
            serde_json::json!("fn main() {}"),
        );
        let id = call.id;
        let messages = vec![
            Message::user("look"),
            Message::assistant_with_tool_calls("", vec![call]),
            Message::tool_result(ToolCallId::from_uuid(id.as_uuid()), "fn main() {}"),
        ];
        let rows = build_rows(&messages, 40);
        let card: Vec<&Row> = rows
            .iter()
            .filter(|row| matches!(row.kind, RowKind::ToolHeading | RowKind::ToolCard))
            .collect();

        // Padding row, heading, gap, subject, gap, body, padding row.
        assert_eq!(card.len(), 7, "{card:?}");
        assert_eq!(content(card[0]), "", "one row of padding at the top");
        assert_eq!(card[1].kind, RowKind::ToolHeading);
        assert_eq!(content(card[1]), "# read_file", "the heading");
        assert_eq!(content(card[2]), "", "a gap under the heading");
        assert_eq!(content(card[3]), "> a.rs", "the subject");
        assert_eq!(content(card[4]), "", "a gap before the output");
        assert_eq!(content(card[5]), "fn main() {}", "the body");
        assert_eq!(content(card[6]), "", "one row of padding at the bottom");

        // No drawn *border*. The left rail is the one vertical rule allowed.
        for row in &card {
            for border in ['\u{250c}', '\u{2510}', '\u{2514}', '\u{2518}'] {
                assert!(
                    !row.text.contains(border),
                    "no drawn border at all: {:?}",
                    row.text
                );
            }
            assert_eq!(
                row.text.matches(RAIL_CHAR).count(),
                1,
                "{:?} has more than one rail",
                row.text
            );
            assert!(
                row.text.starts_with(RAIL_CHAR),
                "{:?} does not start with the rail",
                row.text
            );
        }

        assert!(
            !rows.iter().any(|row| row.text.contains("the file")),
            "the Role::Tool body is not rendered; the card owns the result"
        );
    }

    #[test]
    fn each_call_gets_its_own_card() {
        let first = call(
            "shell",
            serde_json::json!({ "command": "one" }),
            serde_json::json!({ "stdout": "first out" }),
        );
        let second = call(
            "shell",
            serde_json::json!({ "command": "two" }),
            serde_json::json!({ "stdout": "second out" }),
        );
        let messages = vec![
            Message::user("both"),
            Message::assistant_with_tool_calls("", vec![first, second]),
        ];
        let rows = build_rows(&messages, 40);
        let headings: Vec<&str> = rows
            .iter()
            .filter(|row| row.kind == RowKind::ToolHeading)
            .map(content)
            .collect();
        assert_eq!(headings, vec!["# shell", "# shell"], "two separate cards");

        let texts: Vec<&str> = rows
            .iter()
            .filter(|row| matches!(row.kind, RowKind::ToolHeading | RowKind::ToolCard))
            .map(content)
            .collect();
        let first_body = texts
            .iter()
            .position(|text| *text == "first out")
            .expect("the first body");
        let second_heading = texts
            .iter()
            .rposition(|text| *text == "# shell")
            .expect("the second heading");
        assert!(
            second_heading > first_body,
            "the second card starts after the first one ends: {texts:?}"
        );
    }

    #[test]
    fn the_card_reads_heading_gap_subject_gap_output() {
        let call = call(
            "shell",
            serde_json::json!({ "command": "cat test.txt", "description": "Shell" }),
            serde_json::json!({ "stdout": "the contents" }),
        );
        let rows = build_rows(&[Message::assistant_with_tool_calls("", vec![call])], 40);
        let shape_of: Vec<String> = rows
            .iter()
            .map(|row| {
                let kind = match row.kind {
                    RowKind::ToolHeading => "heading",
                    RowKind::ToolCard => "card",
                    RowKind::DiffAdd => "add",
                    RowKind::DiffDel => "del",
                    RowKind::Spacer => "spacer",
                    RowKind::User => "user",
                    RowKind::Assistant => "assistant",
                    RowKind::Divider => "divider",
                };
                format!("{kind}:{:?}", content(row))
            })
            .collect();
        assert_eq!(
            shape_of,
            vec![
                // One row of padding, then the heading with its `# `.
                "card:\"\"",
                "heading:\"# Shell\"",
                // A blank row under the heading, then the contents.
                "card:\"\"",
                "card:\"$ cat test.txt\"",
                "card:\"\"",
                "card:\"the contents\"",
                // One row of padding at the bottom.
                "card:\"\"",
            ]
        );
    }

    #[test]
    fn the_subject_marker_is_the_rows_muted_span() {
        let shell = call(
            "shell",
            serde_json::json!({ "command": "cat test.txt" }),
            serde_json::json!({ "stdout": "hi" }),
        );
        let read = call(
            "read_file",
            serde_json::json!({ "path": "a.rs" }),
            serde_json::json!("hi"),
        );
        let rows = build_rows(
            &[
                Message::assistant_with_tool_calls("", vec![shell]),
                Message::assistant_with_tool_calls("", vec![read]),
            ],
            40,
        );
        let markers: Vec<&str> = rows
            .iter()
            .flat_map(|row| row.spans.iter().map(|(range, _)| &row.text[range.clone()]))
            .collect();
        assert_eq!(
            markers,
            vec!["$", ">"],
            "the marker is the row's span, so the view can draw it gray"
        );

        // The span is inside the row, so styling never changes the width.
        for row in rows.iter().filter(|row| !row.spans.is_empty()) {
            assert_eq!(
                display_width(&row.text),
                40,
                "{:?} is not the full width",
                row.text
            );
        }
    }

    #[test]
    fn the_card_pads_one_cell_at_each_edge_and_one_row_above_and_below() {
        let call = call(
            "shell",
            serde_json::json!({ "command": "cat test.txt" }),
            serde_json::json!({ "stdout": "contents" }),
        );
        let rows = build_rows(&[Message::assistant_with_tool_calls("", vec![call])], 40);
        let band: Vec<&Row> = rows
            .iter()
            .filter(|row| matches!(row.kind, RowKind::ToolHeading | RowKind::ToolCard))
            .collect();

        // One blank band row at the top and one at the bottom.
        assert_eq!(band.first().map(|row| content(row)), Some(""));
        assert_eq!(band.last().map(|row| content(row)), Some(""));

        // Every row is the rail, one cell of padding, content, then the fill.
        for row in &band {
            assert_eq!(
                display_width(&row.text),
                40,
                "{:?} is not the full width",
                row.text
            );
            assert_eq!(
                row.text.chars().next(),
                Some(RAIL_CHAR),
                "{:?} has no rail",
                row.text
            );
            assert_eq!(
                row.text.chars().nth(1),
                Some(' '),
                "{:?} is not padded by one cell after the rail",
                row.text
            );
            assert!(
                row.text.ends_with(' '),
                "{:?} has no right padding",
                row.text
            );
        }
    }

    #[test]
    fn the_heading_is_prefixed_and_the_body_is_not_indented_further() {
        let call = call(
            "shell",
            serde_json::json!({ "command": "cat test.txt" }),
            serde_json::json!({ "stdout": "line one\nline two" }),
        );
        let rows = build_rows(&[Message::assistant_with_tool_calls("", vec![call])], 40);
        let band: Vec<&Row> = rows
            .iter()
            .filter(|row| matches!(row.kind, RowKind::ToolHeading | RowKind::ToolCard))
            .collect();

        let heading = band
            .iter()
            .find(|row| row.kind == RowKind::ToolHeading)
            .expect("a heading");
        assert_eq!(content(heading), "# shell", "the `# ` prefix");

        // `#`, `$` and the output all start in the same column, counted past
        // the rail.
        let column = |text: &str| {
            let body = text.strip_prefix(RAIL_CHAR).unwrap_or(text);
            body.len() - body.trim_start().len()
        };
        let heading_column = column(&heading.text);
        let marker_column = band
            .iter()
            .find(|row| !row.spans.is_empty())
            .map(|row| column(&row.text))
            .expect("a subject line");
        assert_eq!(
            heading_column, 1,
            "the rail, then one cell of padding, then the heading"
        );
        assert_eq!(
            marker_column, heading_column,
            "the `$` starts in the heading's column"
        );
        for body in band
            .iter()
            .filter(|row| matches!(content(row), "line one" | "line two"))
        {
            assert_eq!(
                column(&body.text),
                heading_column,
                "{:?} is indented past the heading",
                body.text
            );
        }
    }

    #[test]
    fn every_card_row_is_exactly_the_transcript_width() {
        // A row one cell short leaves a gap in the background band.
        let call = call(
            "shell",
            serde_json::json!({ "command": "cargo test --workspace" }),
            serde_json::json!({ "stdout": "ok\nline two" }),
        );
        let messages = vec![
            Message::user("run it"),
            Message::assistant_with_tool_calls("", vec![call]),
        ];
        for width in [40u16, 61, 80] {
            let rows = build_rows(&messages, width);
            for row in rows
                .iter()
                .filter(|row| matches!(row.kind, RowKind::ToolHeading | RowKind::ToolCard))
            {
                assert_eq!(
                    display_width(&row.text),
                    usize::from(width),
                    "{:?} is {}/{} cells",
                    row.text,
                    display_width(&row.text),
                    width
                );
            }
        }
    }

    #[test]
    fn a_long_result_is_truncated_with_a_count() {
        let body: String = (0..40).map(|n| format!("line {n}\n")).collect();
        let call = call(
            "shell",
            serde_json::json!({ "command": "cat big.log" }),
            serde_json::json!({ "stdout": body }),
        );
        let messages = vec![
            Message::user("cat it"),
            Message::assistant_with_tool_calls("", vec![call]),
        ];
        let rows = build_rows(&messages, 60);
        let card: Vec<&str> = rows
            .iter()
            .filter(|row| row.kind == RowKind::ToolCard)
            .map(|row| row.text.as_str())
            .collect();

        assert!(card.iter().any(|row| row.contains("line 0")), "{card:?}");
        assert!(
            !card
                .iter()
                .any(|row| row.contains(&format!("line {MAX_TOOL_OUTPUT_LINES}"))),
            "the tail past the cap is cut: {card:?}"
        );
        assert!(
            card.iter()
                .any(|row| row.contains(&format!("(+{} more lines)", 40 - MAX_TOOL_OUTPUT_LINES))),
            "the cut is counted, not silent: {card:?}"
        );
    }

    #[test]
    fn a_failed_call_shows_its_error_instead_of_a_body() {
        let mut call = ToolCall::new("shell", serde_json::json!({ "command": "nope" }));
        call.approval_status = ApprovalStatus::Approved;
        call.execution_status = ExecutionStatus::Failed;
        call.error = Some("command not found".to_owned());
        let messages = vec![
            Message::user("run"),
            Message::assistant_with_tool_calls("", vec![call]),
        ];
        let rows = build_rows(&messages, 50);
        assert!(
            rows.iter()
                .any(|row| row.kind == RowKind::ToolCard && row.text.contains("command not found")),
            "{:?}",
            texts(&rows)
        );
    }

    #[test]
    fn a_pending_call_says_what_it_is_waiting_for() {
        let call = ToolCall::new("shell", serde_json::json!({ "command": "rm -rf /" }));
        let messages = vec![
            Message::user("clean up"),
            Message::assistant_with_tool_calls("", vec![call]),
        ];
        let rows = build_rows(&messages, 50);
        assert!(
            rows.iter()
                .any(|row| row.text.contains("waiting for approval")),
            "{:?}",
            texts(&rows)
        );
    }

    #[test]
    fn an_empty_transcript_has_no_rows() {
        assert!(build_rows(&[], 40).is_empty());
        assert!(Transcript::new().is_empty());
    }

    #[test]
    fn wrap_breaks_on_spaces() {
        assert_eq!(
            wrap_text("one two three four", 9),
            vec!["one two", "three", "four"]
        );
    }

    #[test]
    fn wrap_hard_splits_a_token_wider_than_the_row() {
        let rows = wrap_text("abcdefghij", 4);
        assert_eq!(rows, vec!["abcd", "efgh", "ij"]);
        assert_eq!(rows.concat(), "abcdefghij", "no character is dropped");
    }

    #[test]
    fn wrap_counts_cells_not_chars() {
        // Each of these is two cells wide.
        let rows = wrap_text("\u{4f60}\u{597d}\u{4e16}\u{754c}", 4);
        assert_eq!(rows, vec!["\u{4f60}\u{597d}", "\u{4e16}\u{754c}"]);
        for row in &rows {
            let width: usize = row.chars().filter_map(|c| c.width()).sum();
            assert!(width <= 4, "{row:?} is {width} cells");
        }
    }

    #[test]
    fn wrap_keeps_a_hard_newline_as_its_own_row() {
        assert_eq!(wrap_text("a\n\nb", 8), vec!["a", "", "b"]);
    }

    #[test]
    fn wrap_of_empty_text_is_one_empty_row() {
        assert_eq!(wrap_text("", 8), vec![""]);
    }

    fn transcript_of(messages: Vec<Message>, width: u16, viewport: usize) -> Transcript {
        let mut transcript = Transcript::new();
        transcript.set_width(width);
        transcript.set_viewport(viewport);
        transcript.set_messages(messages);
        transcript
    }

    fn tall() -> Vec<Message> {
        (0..20)
            .map(|n| {
                if n % 2 == 0 {
                    Message::user(format!("user {n}"))
                } else {
                    Message::assistant(format!("assistant {n}"))
                }
            })
            .collect()
    }

    #[test]
    fn a_new_transcript_follows_the_last_row() {
        let transcript = transcript_of(tall(), 40, 5);
        assert!(transcript.pinned());
        let max = transcript.rows().len() - 5;
        assert_eq!(transcript.offset(), max);
        assert_eq!(
            transcript.visible().last().map(|row| row.text.as_str()),
            Some("assistant 19")
        );
    }

    #[test]
    fn page_up_releases_the_lock_and_the_last_row_rearms_it() {
        let mut transcript = transcript_of(tall(), 40, 5);
        transcript.page_up();
        assert!(!transcript.pinned(), "an upward move leaves the end");

        transcript.page_down();
        assert!(
            transcript.pinned(),
            "coming back to the last row re-arms the lock"
        );
    }

    #[test]
    fn growth_while_pinned_keeps_the_reader_at_the_end() {
        let mut transcript = transcript_of(vec![Message::user("one")], 40, 5);
        assert!(transcript.pinned());
        let before = transcript.offset();

        let mut messages = transcript.messages().to_vec();
        messages.push(Message::assistant("two"));
        messages.push(Message::assistant("three"));
        transcript.set_messages(messages);

        assert!(transcript.offset() >= before);
        assert_eq!(
            transcript.visible().last().map(|row| row.text.as_str()),
            Some("three")
        );
    }

    #[test]
    fn growth_while_released_does_not_move_the_reader() {
        let mut transcript = transcript_of(tall(), 40, 5);
        transcript.top();
        let before = transcript.offset();
        transcript.set_messages(tall());
        assert_eq!(transcript.offset(), before);
        assert!(!transcript.pinned());
    }

    #[test]
    fn pinned_after_scroll_mirrors_the_gui_rule() {
        // At the end: stay pinned.
        assert!(pinned_after_scroll(false, 95, 95, 100, 100, 5, 3));
        // Grew while pinned: stay pinned.
        assert!(pinned_after_scroll(true, 10, 10, 20, 10, 5, 3));
        // Moved up and away from the end: release.
        assert!(!pinned_after_scroll(true, 10, 20, 100, 100, 5, 3));
        // Held still: keep whatever the lock was.
        assert!(pinned_after_scroll(true, 40, 40, 100, 100, 5, 3));
    }

    #[test]
    fn a_narrower_terminal_rewraps_and_keeps_following() {
        let long = Message::user("one two three four five six seven eight nine ten");
        let mut transcript = transcript_of(vec![long], 80, 5);
        let wide_rows = transcript.rows().len();
        assert_eq!(wide_rows, 3, "a band: padding, one line, padding");

        transcript.set_width(12);
        assert!(transcript.rows().len() > wide_rows);
        assert!(transcript.pinned());
    }

    #[test]
    fn the_generation_only_moves_forward() {
        let mut transcript = Transcript::new();
        assert_eq!(transcript.generation(), 0);
        assert_eq!(transcript.next_generation(), 1);
        assert_eq!(transcript.next_generation(), 2);
    }

    #[test]
    fn an_opened_card_shows_more_output_than_a_closed_one() {
        let output: String = (0..20).map(|i| format!("line {i}\n")).collect();
        let call = call(
            "shell",
            serde_json::json!({ "command": "seq 20" }),
            serde_json::json!({ "stdout": output }),
        );
        let id = call.id;
        let messages = vec![Message::assistant_with_tool_calls("", vec![call])];

        let mut transcript = transcript_of(messages, 60, 40);
        let closed = transcript.rows().len();
        assert!(transcript.toggle_expanded(id));
        let opened = transcript.rows().len();
        assert!(
            opened > closed,
            "opening a card has to add rows: {closed} -> {opened}"
        );
        assert!(transcript.is_expanded(id));

        transcript.toggle_expanded(id);
        assert_eq!(transcript.rows().len(), closed);
        assert!(!transcript.is_expanded(id));
    }

    #[test]
    fn a_fetched_original_replaces_the_compressed_output() {
        let call = call(
            "shell",
            serde_json::json!({ "command": "seq" }),
            serde_json::json!({ "stdout": "the compressed view" }),
        );
        let id = call.id;
        let messages = vec![Message::assistant_with_tool_calls("", vec![call])];
        let mut transcript = transcript_of(messages, 60, 40);
        transcript.toggle_expanded(id);
        assert!(texts(transcript.rows())
            .iter()
            .any(|row| row.contains("the compressed view")));

        transcript.set_original(id, "the whole thing".to_owned());
        let rows = texts(transcript.rows()).join("\n");
        assert!(rows.contains("the whole thing"), "{rows}");
        assert!(!rows.contains("the compressed view"), "{rows}");
    }

    #[test]
    fn an_edit_card_tints_its_added_and_removed_rows() {
        let mut call = call(
            "edit_file",
            serde_json::json!({
                "path": "a.rs",
                "old_string": "let a = 1;",
                "new_string": "let a = 2;",
            }),
            serde_json::Value::Null,
        );
        call.result = None;
        call.approval_status = ApprovalStatus::Pending;
        call.execution_status = ExecutionStatus::NotStarted;
        let rows = build_rows(&[Message::assistant_with_tool_calls("", vec![call])], 60);
        let kinds: Vec<RowKind> = rows.iter().map(|row| row.kind).collect();
        assert!(kinds.contains(&RowKind::DiffDel), "{kinds:?}");
        assert!(kinds.contains(&RowKind::DiffAdd), "{kinds:?}");
    }

    /// Prose, then two cards: the three kinds of block a transcript has.
    fn prose_then_two_cards() -> (Vec<Message>, MessageId, ToolCallId, ToolCallId) {
        let first = call(
            "shell",
            serde_json::json!({ "command": "one" }),
            serde_json::json!({ "stdout": "first" }),
        );
        let second = call(
            "shell",
            serde_json::json!({ "command": "two" }),
            serde_json::json!({ "stdout": "second" }),
        );
        let ids = (first.id, second.id);
        let message = Message::assistant_with_tool_calls("here is what I ran", vec![first, second]);
        let message_id = message.id;
        (vec![message], message_id, ids.0, ids.1)
    }

    #[test]
    fn the_selection_walks_prose_and_cards_and_reveals_each_one() {
        let (messages, message_id, first, second) = prose_then_two_cards();
        // A viewport of one row, so revealing a block always has to move.
        let mut transcript = transcript_of(messages, 60, 1);
        assert_eq!(transcript.selection(), None);

        transcript.select(1);
        assert_eq!(
            transcript.selection(),
            Some(BlockRef::message(message_id)),
            "the first block is the message's prose"
        );

        transcript.select(1);
        assert_eq!(
            transcript.selected_call(),
            Some(first),
            "down from the prose is the first card"
        );
        transcript.select(1);
        assert_eq!(transcript.selected_call(), Some(second));
        transcript.select(1);
        assert_eq!(
            transcript.selected_call(),
            Some(second),
            "the last block is the end"
        );

        transcript.select(-1);
        assert_eq!(transcript.selected_call(), Some(first));

        let selected = transcript.selection().expect("something is selected");
        let row = transcript
            .rows()
            .iter()
            .position(|row| row.block == Some(selected))
            .expect("the block has rows");
        assert!(
            transcript
                .visible()
                .iter()
                .any(|row| row.block == Some(selected)),
            "revealing a block puts it in view (block at row {row}, offset {})",
            transcript.offset()
        );
        transcript.clear_selection();
        assert_eq!(transcript.selection(), None);
    }

    #[test]
    fn a_selection_survives_an_upsert_of_its_own_row() {
        let (messages, message_id, first, _) = prose_then_two_cards();
        let mut transcript = transcript_of(messages, 60, 40);
        transcript.select(1);
        transcript.select(1);
        let selected = transcript.selection();
        assert_eq!(transcript.selected_call(), Some(first));

        // The call comes back with a different body, in the same message: new
        // rows, same block id, so the selection is still on that card.
        let mut updated = call(
            "shell",
            serde_json::json!({ "command": "one" }),
            serde_json::json!({ "stdout": "changed" }),
        );
        updated.id = first;
        let mut message = Message::assistant_with_tool_calls("here is what I ran", vec![updated]);
        message.id = message_id;
        transcript.upsert(message);
        assert_eq!(transcript.selection(), selected, "same block, new rows");
    }

    #[test]
    fn a_new_message_id_is_a_different_block() {
        // The block is keyed on the message, so a row replaced by id keeps the
        // selection and one replaced wholesale does not. That is the rule the
        // reconcile depends on, so it is worth stating.
        let (messages, _, first, _) = prose_then_two_cards();
        let mut transcript = transcript_of(messages, 60, 40);
        transcript.select(1);
        transcript.select(1);
        assert_eq!(transcript.selected_call(), Some(first));
        transcript.set_messages(vec![Message::user("just prose")]);
        assert_eq!(transcript.selection(), None);
    }
}
