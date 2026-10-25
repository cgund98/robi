//! Assistant prose as markdown, rendered to pre-wrapped styled rows.
//!
//! The terminal's peer of `AssistantMarkdown.tsx`. The GUI hands the text to
//! `solid-markdown` with `remark-gfm`; here it goes to [`tui_markdown`], which
//! is a `pulldown-cmark` front end producing a ratatui `Text`. Headings, lists,
//! tables, fenced code, links, quotes and GFM alerts all arrive as styled
//! spans, so the terminal and the desktop read the same document.
//!
//! **Styling is a `StyleSheet`, not a post-pass.** The renderer asks for a
//! `Style` per construct, and every answer here is a token from
//! [`crate::tui::theme`]: the palette still lives in one file, and the desktop
//! and the terminal cannot drift.
//!
//! The rows this produces are the transcript's own: one `Row` per visual line,
//! already wrapped, so the scroll offset stays row-exact.

use ratatui::style::Style;
use ratatui::text::Line;
use tui_markdown::{AlertKind, Options, StyleSheet};

use crate::tui::state::transcript::{split_cells, styled_row, Row, RowKind};
use crate::tui::theme;

/// Render `content` to rows no wider than `width` cells.
pub fn rows(content: &str, width: usize) -> Vec<Row> {
    let width = width.max(8);
    let text = tui_markdown::from_str_with_options(content, &Options::new(StyleSheetImpl));
    let mut rows = Vec::new();
    for line in &text.lines {
        let cells = cells_of(line);
        if is_table(&line.to_string()) {
            // A table's box is drawn to its own width. Wrapping it would tear
            // the borders apart, so an over-wide one is cut instead, which at
            // least keeps the columns aligned.
            let cut = truncate(cells, width);
            let (text, spans) = styled_row(cut);
            rows.push(Row::styled(RowKind::Assistant, text, spans));
            continue;
        }
        for chunk in split_cells(&cells, width) {
            let (text, spans) = styled_row(chunk);
            rows.push(Row::styled(RowKind::Assistant, text, spans));
        }
    }
    rows
}

/// The characters of one rendered line, each with its effective style.
///
/// The line's own style is the base — a heading, a code block, a quote — and a
/// span's style is layered over it, which is how `**bold**` inside a quote stays
/// both.
fn cells_of(line: &Line<'_>) -> Vec<(char, Style)> {
    let base = line.style;
    let mut cells = Vec::new();
    for span in &line.spans {
        let style = base.patch(span.style);
        for ch in span.content.chars() {
            cells.push((ch, style));
        }
    }
    cells
}

/// Whether a rendered line is part of a table.
///
/// `tui_markdown` draws tables with box-drawing characters and no other
/// construct does, so the leading corner is the whole test.
fn is_table(line: &str) -> bool {
    matches!(
        line.chars().next(),
        Some('\u{250c}') | Some('\u{2502}') | Some('\u{251c}') | Some('\u{2514}')
    )
}

/// `cells` cut to `width`, with an ellipsis when anything was dropped.
fn truncate(cells: Vec<(char, Style)>, width: usize) -> Vec<(char, Style)> {
    use unicode_width::UnicodeWidthChar;

    let total: usize = cells.iter().map(|(c, _)| c.width().unwrap_or(0)).sum();
    if total <= width {
        return cells;
    }
    let mut kept = Vec::new();
    let mut used = 0;
    for (ch, style) in cells {
        let cell = ch.width().unwrap_or(0);
        // One cell is held back for the ellipsis.
        if used + cell > width.saturating_sub(1) {
            break;
        }
        used += cell;
        kept.push((ch, style));
    }
    if let Some(style) = kept.last().map(|(_, style)| *style) {
        kept.push(('\u{2026}', style));
    }
    kept
}

/// The terminal's answer for every markdown construct.
///
/// Every method here has a default in the trait, and each one is overridden
/// only where the default would use a colour this design does not own.
// `Clone` is a supertrait; the sheet is stateless, so the derive is free.
#[derive(Clone, Copy)]
struct StyleSheetImpl;

impl StyleSheet for StyleSheetImpl {
    /// Headings carry weight instead of a marker, so `heading_marker` is empty.
    /// Level 1 is the only one that is also underlined, which is the only
    /// distinction a terminal can make beyond brightness.
    fn heading(&self, level: u8) -> Style {
        theme::markdown_heading(level)
    }

    fn heading_marker(&self, _level: u8) -> &str {
        ""
    }

    fn code(&self) -> Style {
        theme::code()
    }

    /// A link is `label (url)` in this renderer; the accent marks both.
    fn link(&self) -> Style {
        theme::markdown_link()
    }

    fn blockquote(&self) -> Style {
        theme::markdown_quote()
    }

    fn list_marker(&self) -> Style {
        theme::muted()
    }

    fn table_header(&self) -> Style {
        theme::markdown_table_header()
    }

    fn table_cell(&self) -> Style {
        theme::markdown_table_cell()
    }

    fn table_border(&self) -> Style {
        theme::markdown_table_border()
    }

    /// Raw HTML is shown, not interpreted, and marked as not-prose.
    fn html(&self) -> Style {
        theme::faint()
    }

    fn image_alt(&self) -> Style {
        theme::muted()
    }

    fn alert(&self, _kind: AlertKind) -> Style {
        theme::markdown_alert()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|row| row.text.as_str()).collect()
    }

    /// The one test here: a heading and a table arrive as rows, and the table's
    /// box survives. Everything else about the renderer is its own test suite's
    /// job, and a per-construct test of ours would only pin its wording.
    #[test]
    fn a_heading_and_a_table_become_rows() {
        let markdown =
            "# Release notes\n\n| File | Change |\n|:-----|-------:|\n| a.rs | added  |\n";
        let rows = rows(markdown, 40);
        let lines = texts(&rows);

        assert!(lines.contains(&"Release notes"), "{lines:?}");
        assert!(
            !lines.iter().any(|line| line.starts_with('#')),
            "the heading marker is dropped and the style carries the weight: {lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.starts_with('\u{250c}')),
            "the table keeps its box: {lines:?}"
        );
        // The heading row is styled, and differently from the body.
        let heading = rows
            .iter()
            .find(|row| row.text == "Release notes")
            .expect("the heading row");
        assert!(!heading.spans.is_empty(), "a heading carries a span");
        assert_ne!(
            heading.spans[0].1,
            Style::default(),
            "the heading is not raw default style"
        );
    }

    /// A mermaid fence is **source**, not a diagram.
    ///
    /// There is no portable cell-accurate way to draw a graph in a terminal, so
    /// T6's rule is that the fence renders as its own text with the language on the
    /// opening line — which is what `tui-markdown` already does by default. This
    /// test is the pin: a later change that stripped the fence, or that tried to
    /// draw the diagram, fails here rather than in a rendering a reader sees.
    #[test]
    fn a_mermaid_fence_renders_as_its_source_and_does_not_panic() {
        let markdown = "Before.\n\n```mermaid\ngraph LR\n  A --> B\n```\n\nAfter.\n";
        let rows = rows(markdown, 40);
        let lines = texts(&rows);

        assert!(
            lines.contains(&"```mermaid"),
            "the fence keeps its language: {lines:?}"
        );
        assert!(lines.contains(&"graph LR"), "{lines:?}");
        assert!(lines.contains(&"  A --> B"), "{lines:?}");
        assert!(
            lines.iter().filter(|line| **line == "```").count() >= 1,
            "the fence is closed: {lines:?}"
        );
        // The prose either side survives, so the fence is a block and not a
        // truncation.
        assert!(lines.contains(&"Before."), "{lines:?}");
        assert!(lines.contains(&"After."), "{lines:?}");
    }
}
