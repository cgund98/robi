//! Colour and style for the terminal front end.
//!
//! One constant per token in
//! `docs/src/design/shell/visual-style.md`, so the two front ends stay one
//! product: a change there is a change here, and neither invents a hex.
//!
//! Truecolor is assumed. A terminal that does not advertise
//! `COLORTERM=truecolor` still gets these RGB values; the open decision in the
//! T0 plan covers adding a 256-colour fallback.

use ratatui::style::{Color, Modifier, Style};

/// `0xRRGGBB` as a terminal colour.
///
/// `const` so every token below is a compile-time value, and a typo in the
/// token table cannot reach a running frame.
const fn rgb(hex: u32) -> Color {
    Color::Rgb(
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    )
}

// Surfaces.

/// `--bg-canvas`: the transcript background.
pub const BG_CANVAS: Color = rgb(0x0b0b0b);
/// `--bg-sidebar`: the legacy two-column sidebar. Unused by the TUI layout.
pub const BG_SIDEBAR: Color = rgb(0x1f1f1e);
/// `--bg-surface`: the composer field, elevated chips.
pub const BG_SURFACE: Color = rgb(0x1f1f1e);
/// `--bg-surface-hover`: a hovered row or control.
pub const BG_SURFACE_HOVER: Color = rgb(0x282827);
/// `--bg-surface-active`: a selected row, a pressed toggle.
pub const BG_SURFACE_ACTIVE: Color = rgb(0x333333);
/// `--bg-row`: file rows inside the edit summary widget.
pub const BG_ROW: Color = rgb(0x303030);
/// `--bg-user`: the user bubble, one step above the canvas.
pub const BG_USER: Color = rgb(0x181817);
/// `--code-bg`: an inline code chip.
pub const CODE_BG: Color = rgb(0x1a1a19);

// Ink.

/// `--ink`: primary body text, warm rather than pure white.
pub const INK: Color = rgb(0xc3c2b8);
/// `--ink-strong`: titles and emphasis.
pub const INK_STRONG: Color = rgb(0xecece8);
/// `--ink-bright`: assistant prose, and the user bubble's text.
pub const INK_BRIGHT: Color = rgb(0xf7f6f2);
/// `--ink-muted`: labels, timestamps, activity lines.
pub const INK_MUTED: Color = rgb(0x8a8983);
/// `--ink-faint`: placeholders and disabled controls.
pub const INK_FAINT: Color = rgb(0x5c5c57);
/// `--code-ink`: inline code text.
pub const CODE_INK: Color = rgb(0xd4d3cb);

// Accent and state.

/// `--accent`: focus, links, active status. State, never decoration.
pub const ACCENT: Color = rgb(0x55a2fb);
/// `--accent-muted`: accent on dark fills.
pub const ACCENT_MUTED: Color = rgb(0x3a6fa8);
/// `--rule`: a hairline separator.
pub const RULE: Color = rgb(0x2a2a28);
/// `--danger`: a destructive action or a failed tool.
pub const DANGER: Color = rgb(0xe56767);
/// `--success`: a completed tool or an applied edit.
pub const SUCCESS: Color = rgb(0x6fbf7a);
/// `--warn`: reject-with-reason copy.
pub const WARN: Color = rgb(0xe8c547);

// Modes, diffs, find.

/// `--mode-ask`.
pub const MODE_ASK: Color = rgb(0x6fbf7a);
/// `--mode-plan`.
pub const MODE_PLAN: Color = rgb(0xe39a3c);
/// `--diff-add`: `+N` line counts.
pub const DIFF_ADD: Color = rgb(0x5db27b);
/// `--diff-del`: `-N` line counts.
pub const DIFF_DEL: Color = rgb(0xc46b6b);
/// `--find-hit`: a find-in-document match.
pub const FIND_HIT: Color = rgb(0x6b5416);
/// `--find-hit-current`: the active match.
pub const FIND_HIT_CURRENT: Color = rgb(0xe39a3c);

/// The frame background. Every cell the view paints sits on this.
pub fn canvas() -> Style {
    Style::default().bg(BG_CANVAS).fg(INK)
}

/// Assistant prose.
pub fn prose() -> Style {
    Style::default().bg(BG_CANVAS).fg(INK_BRIGHT)
}

/// A user message: a raised band, the peer of a tool call's card.
pub fn user_bubble() -> Style {
    Style::default().bg(BG_USER).fg(INK_BRIGHT)
}

/// The rail on a user message's left edge. `--accent`, because the user's turn
/// is the one thing in the transcript that is *state* rather than output.
pub fn user_rail() -> Style {
    Style::default().bg(BG_USER).fg(ACCENT)
}

/// The rail on a tool call's card. Gray, so a card reads as machinery beside
/// the blue of a user turn.
pub fn tool_card_rail() -> Style {
    Style::default().bg(CODE_BG).fg(INK_FAINT)
}

/// One muted line: a tool call, a hint, a timestamp.
pub fn muted() -> Style {
    Style::default().bg(BG_CANVAS).fg(INK_MUTED)
}

/// The header's product mark and separators.
pub fn faint() -> Style {
    Style::default().bg(BG_CANVAS).fg(INK_FAINT)
}

/// The header's workspace name and session title.
pub fn strong() -> Style {
    Style::default().bg(BG_CANVAS).fg(INK_STRONG)
}

/// A divider row, such as **Context compacted**.
pub fn divider() -> Style {
    Style::default().bg(BG_CANVAS).fg(INK_FAINT)
}

/// A notice that reports a failure.
pub fn error() -> Style {
    Style::default()
        .bg(BG_CANVAS)
        .fg(DANGER)
        .add_modifier(Modifier::BOLD)
}

/// A notice that reports a startup fact.
pub fn info() -> Style {
    Style::default().bg(BG_CANVAS).fg(ACCENT)
}

/// A tool call's card: a lighter band than the canvas, with no drawn border.
///
/// `--code-bg` is the token for a raised code surface, which is exactly what a
/// call and its output are.
pub fn tool_card() -> Style {
    Style::default().bg(CODE_BG).fg(CODE_INK)
}

/// The composer band: the same raised surface a tool card uses, so "this is a
/// surface" means one thing in the whole frame.
pub fn composer_surface() -> Style {
    Style::default().bg(BG_SURFACE).fg(INK_BRIGHT)
}

/// The composer's rail: `--accent`, the same column a user message wears.
///
/// The composer is the user's turn before it is sent, so it is state, not
/// machinery — the tool card's gray rail is the contrast, not the model.
pub fn composer_rail() -> Style {
    Style::default().bg(BG_SURFACE).fg(ACCENT)
}

/// The composer's metadata row: the mode and model from T4, and the lock status
/// until then.
pub fn composer_status() -> Style {
    Style::default().bg(BG_SURFACE).fg(INK_MUTED)
}

/// The header's activity segment. `--ink-muted`, the same tone as a timestamp:
/// state the user can glance at, not state that competes with the title.
pub fn activity() -> Style {
    Style::default().bg(BG_CANVAS).fg(INK_MUTED)
}

/// The footer progress widget's track. `--ink-faint`, the tone
/// `visual-style.md` gives a running agent's spinner track.
pub fn progress_track() -> Style {
    Style::default().bg(BG_CANVAS).fg(INK_FAINT)
}

/// The lit cell of the footer progress widget: the spinner's leading edge,
/// which `visual-style.md` puts in `--ink`.
pub fn progress_head() -> Style {
    Style::default().bg(BG_CANVAS).fg(INK)
}

/// The caret drawn at the end of streaming text.
pub fn caret() -> Style {
    Style::default().bg(BG_CANVAS).fg(ACCENT)
}

// Markdown. One function per construct `tui/state/markdown.rs` asks for, so the
// assistant's prose is styled from this file and nowhere else.

/// A markdown heading. Weight replaces the `#` marker; level 1 also underlines,
/// which is the only distinction a terminal has left after brightness.
pub fn markdown_heading(level: u8) -> Style {
    let style = Style::default()
        .bg(BG_CANVAS)
        .fg(INK_STRONG)
        .add_modifier(Modifier::BOLD);
    if level <= 1 {
        style.add_modifier(Modifier::UNDERLINED)
    } else {
        style
    }
}

/// A link's label and its appended destination. `--accent`, underlined, so it
/// reads as something the reader could open even though a terminal cannot.
pub fn markdown_link() -> Style {
    Style::default()
        .bg(BG_CANVAS)
        .fg(ACCENT)
        .add_modifier(Modifier::UNDERLINED)
}

/// A blockquote's `>` prefix and its body.
pub fn markdown_quote() -> Style {
    Style::default()
        .bg(BG_CANVAS)
        .fg(INK_MUTED)
        .add_modifier(Modifier::ITALIC)
}

/// A table's header row. On the canvas, because the box-drawing border already
/// delimits the cell and a band would fight it.
pub fn markdown_table_header() -> Style {
    Style::default()
        .bg(BG_CANVAS)
        .fg(INK_STRONG)
        .add_modifier(Modifier::BOLD)
}

/// An ordinary table cell: the prose ink, one step quieter than a body sentence.
pub fn markdown_table_cell() -> Style {
    Style::default().bg(BG_CANVAS).fg(INK)
}

/// A table's box-drawing border. `--ink-faint`, the hairline tone, so the box
/// frames the data without competing with it.
pub fn markdown_table_border() -> Style {
    Style::default().bg(BG_CANVAS).fg(INK_FAINT)
}

/// A GFM alert (`> [!NOTE]`) heading and body. `--warn`: it is a callout, and
/// the accent is already the link's.
pub fn markdown_alert() -> Style {
    Style::default().bg(BG_CANVAS).fg(WARN)
}

/// A tool card's heading. Gray rather than the card's bright ink, so the name
/// of the tool reads as a label and the command below it is what stands out.
pub fn tool_card_heading() -> Style {
    Style::default().bg(CODE_BG).fg(INK_MUTED)
}

/// The `$` before a card's command, and the `>` before a non-shell subject.
pub fn tool_card_marker() -> Style {
    Style::default().bg(CODE_BG).fg(INK_MUTED)
}

/// Inline code and fenced blocks.
pub fn code() -> Style {
    Style::default().bg(CODE_BG).fg(CODE_INK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_bubble_is_lighter_than_the_canvas() {
        let (Color::Rgb(cr, _, _), Color::Rgb(ur, _, _)) = (BG_CANVAS, BG_USER) else {
            panic!("the tokens are RGB");
        };
        assert!(ur > cr, "the bubble must step above the canvas");
    }

    #[test]
    fn a_cards_heading_and_marker_are_gray_rather_than_its_ink() {
        // The card's body is near-white; the label and the `$` are gray, so the
        // command is what stands out on the band.
        let ink = match tool_card().fg {
            Some(Color::Rgb(r, g, b)) => (r, g, b),
            other => panic!("the card ink is RGB: {other:?}"),
        };
        for style in [tool_card_heading(), tool_card_marker()] {
            let (r, g, b) = match style.fg {
                Some(Color::Rgb(r, g, b)) => (r, g, b),
                other => panic!("{other:?} is not RGB"),
            };
            assert_eq!(
                (r, g, b),
                (0x8a, 0x89, 0x83),
                "both are the `--ink-muted` token"
            );
            assert!(
                r < ink.0 && g < ink.1 && b < ink.2,
                "the gray is darker than the card ink"
            );
            assert_eq!(style.bg, tool_card().bg, "the band is unbroken");
        }
    }

    #[test]
    fn every_token_is_an_opaque_rgb_value() {
        // A named colour would render with the terminal's own palette and the
        // two front ends would stop matching.
        let tokens = [
            BG_CANVAS,
            BG_SIDEBAR,
            BG_SURFACE,
            BG_SURFACE_HOVER,
            BG_SURFACE_ACTIVE,
            BG_ROW,
            BG_USER,
            CODE_BG,
            INK,
            INK_STRONG,
            INK_BRIGHT,
            INK_MUTED,
            INK_FAINT,
            CODE_INK,
            ACCENT,
            ACCENT_MUTED,
            RULE,
            DANGER,
            SUCCESS,
            WARN,
            MODE_ASK,
            MODE_PLAN,
            DIFF_ADD,
            DIFF_DEL,
            FIND_HIT,
            FIND_HIT_CURRENT,
        ];
        for token in tokens {
            match token {
                Color::Rgb(_, _, _) => {}
                other => panic!("{other:?} is not an RGB token"),
            }
        }
    }

    #[test]
    fn rgb_unpacks_channel_by_channel() {
        assert_eq!(rgb(0x55a2fb), Color::Rgb(0x55, 0xa2, 0xfb));
        assert_eq!(rgb(0x000000), Color::Rgb(0, 0, 0));
        assert_eq!(rgb(0xffffff), Color::Rgb(255, 255, 255));
    }
}
