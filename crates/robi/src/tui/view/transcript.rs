//! The transcript region: the pre-wrapped rows, styled.
//!
//! No wrapping happens here. `state::transcript` produced one `Row` per visual
//! line, and `App` sliced the visible ones, so a scroll offset is row-exact.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::state::tool_card;
use crate::tui::state::transcript::{display_width, wrap_text, Row, RowKind, RAIL_CHAR};
use crate::tui::state::Phase;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let transcript = app.transcript();
    if transcript.is_empty() {
        draw_empty(frame, area, app);
        return;
    }
    let paint = Paint {
        selection: transcript.selection(),
        dots: app.pending_dots(),
        mode: app.agent_mode(),
    };
    let mut lines: Vec<Line<'_>> = transcript
        .visible()
        .iter()
        .map(|row| line_for(row, area.width, paint))
        .collect();

    // A caret on the last row while the reply is arriving. It is appended to
    // the built line rather than drawn by the row builder, so the wrapped rows
    // and the scroll offset stay exactly what `state::transcript` computed.
    if transcript.phase() == Phase::Responding {
        if let Some(line) = lines.last_mut() {
            if line.width() < usize::from(area.width) {
                line.spans.push(Span::styled("\u{258d}", theme::caret()));
            }
        }
    }

    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), area);
}

/// One row, painted.
///
/// `selection` is the selected block. Its rows are marked so the keyboard's
/// target is visible: a heading carries a `\u{25b8}` in the accent tone, and the
/// rest of the block brightens. The row's width never changes, so the scroll
/// offset does not move when the selection does.
/// What the view needs that is not in the rows.
///
/// Both fields are time-varying or view-only, which is why they are painted here
/// and not stored: a row that changed every 250 ms would rebuild and rewrap the
/// whole transcript three times a second.
#[derive(Debug, Clone, Copy)]
struct Paint {
    selection: Option<crate::tui::state::transcript::BlockRef>,
    /// Dots after a waiting card's label. 1, 2, or 3.
    dots: usize,
    /// The mode the session is in now, which a row with no stamp falls back to.
    ///
    /// A row is stamped by the user message that started its turn, so a turn keeps
    /// the colour of the mode that produced it after `Ctrl-T`. Everything written
    /// before that label existed is unstamped, and follows the session: one
    /// coherent colour for an old transcript rather than a bare one.
    mode: crate::domain::chat_session::model::AgentMode,
}

fn line_for(row: &Row, width: u16, paint: Paint) -> Line<'static> {
    let selected = paint.selection.is_some() && row.block == paint.selection;
    let line = plain_line(row, width, paint);
    if selected {
        selected_line(line, width)
    } else {
        line
    }
}

/// Every row of the selected block, on the selection's own background.
///
/// One tone for every kind of row is the point: a marker on a heading is not
/// enough to say "this is the block the keys act on", and prose had no marker at
/// all. The foreground is kept, so a diff stays green and red and a heading stays
/// muted; only the background moves. The row is also filled to the full width, so
/// a paragraph highlights as one bar rather than stopping at its last glyph.
fn selected_line(mut line: Line<'static>, width: u16) -> Line<'static> {
    for span in &mut line.spans {
        span.style = span.style.bg(theme::BG_SELECTED);
    }
    let used: usize = line
        .spans
        .iter()
        .map(|span| display_width(&span.content))
        .sum();
    let fill = usize::from(width).saturating_sub(used);
    if fill > 0 {
        line.spans.push(Span::styled(
            " ".repeat(fill),
            theme::selected(theme::canvas()),
        ));
    }
    line
}

/// Cells reserved for the animation, so the dots never move as they are added.
const DOT_CELLS: usize = 3;

/// The label row of a card that is waiting, with `dots` of animation after it.
///
/// The dots go inside the row's own padding, so the band is exactly as wide as it
/// was: `band_line` fills to `width` from the text it is given, and the phrase got
/// longer by the number of dots the fill got shorter by. That is what keeps the
/// scroll offset and the wrap unchanged while the marker moves.
fn waiting_row(row: &Row, dots: usize) -> Option<Row> {
    if !row.attention || !row.text.contains(tool_card::WAITING_LABEL) {
        return None;
    }
    // A row with spans is one of the card's marked lines; the label never is, and
    // rewriting its text would shift those byte ranges.
    if !row.spans.is_empty() {
        return None;
    }
    // Rebuilt from the label, not edited in place. `row.text` is already banded and
    // padded to the frame's width, so appending a dot would make the row `width + 1`
    // cells long: `band_line`'s fill saturates at zero and the band loses its right
    // edge instead of the padding giving up a cell. Building the content and letting
    // `band_line` pad it keeps the row exactly `width` cells wide whatever `dots` is.
    //
    // The dots sit in a fixed three-cell field for the same reason they are even:
    // the first dot is always in the same column, so adding one never moves the
    // others.
    let mut animated = row.clone();
    animated.text = format!(
        "{RAIL_CHAR} {}{:<width$}",
        tool_card::WAITING_LABEL,
        ".".repeat(dots.min(DOT_CELLS)),
        width = DOT_CELLS
    );
    Some(animated)
}

/// A card's rail: orange while the call is waiting on the user, quiet otherwise.
///
/// The rail is the one column every card row shares, so it is where "this needs
/// you" can be said without touching the text, and without depending on the
/// selection — a pause two cards up the transcript still reads as a pause.
fn card_rail(row: &Row) -> ratatui::style::Style {
    if row.attention {
        theme::attention_rail()
    } else {
        theme::tool_card_rail()
    }
}

fn plain_line(row: &Row, width: u16, paint: Paint) -> Line<'static> {
    let selected = paint.selection.is_some() && row.block == paint.selection;
    // The turn's own mode, or the session's when the row has no stamp.
    let mode = row.mode.unwrap_or(paint.mode);
    let (band, rail) = match row.kind {
        RowKind::User => (theme::user_bubble(), theme::user_rail(mode)),
        RowKind::ToolHeading => (theme::tool_card_heading(), card_rail(row)),
        RowKind::ToolHint => (theme::tool_card_marker(), card_rail(row)),
        RowKind::DiffAdd => (theme::diff_add(), card_rail(row)),
        RowKind::DiffDel => (theme::diff_del(), card_rail(row)),
        _ => (theme::tool_card(), card_rail(row)),
    };
    if let Some(animated) = waiting_row(row, paint.dots) {
        return band_line(&animated, width, band, rail);
    }
    if selected && row.kind == RowKind::ToolHeading {
        // `# heading` becomes `\u{25b8} heading`. Two cells for two cells, so the
        // row's width and the scroll offset do not move when the cursor lands.
        if let Some(marked) = marked_heading(row) {
            return band_line(&marked, width, band, rail);
        }
    }
    match row.kind {
        RowKind::User => band_line(row, width, band, rail),
        RowKind::ToolHeading => band_line(row, width, band, rail),
        RowKind::ToolCard | RowKind::ToolHint => band_line(row, width, band, rail),
        RowKind::DiffAdd | RowKind::DiffDel => band_line(row, width, band, rail),
        // Railed prose: the band is the canvas, so only the rail and the aligned
        // column show. **The rail is quiet, not the turn's mode**: the mode belongs
        // to what a person asked for, so it colours the user's band and the composer
        // and stops there. A reply is the model's, whichever mode asked for it.
        RowKind::Assistant => band_line(row, width, theme::prose(), theme::prose_rail()),
        // The closed turn's duration. Gray and unmarked, like the webview's:
        // it is a footnote to the turn, not another block in it.
        RowKind::TurnFooter => Line::from(Span::styled(row.text.clone(), theme::muted())),
        RowKind::Divider => Line::from(Span::styled(
            format!("\u{2014} {} \u{2014}", row.text),
            theme::divider(),
        ))
        .alignment(Alignment::Center),
        RowKind::Spacer => Line::default(),
    }
}

/// `row` with its `# ` label replaced by the cursor marker.
///
/// `None` when the row does not carry the label, which leaves the plain heading
/// rather than a row with a stray glyph in it.
fn marked_heading(row: &Row) -> Option<Row> {
    let at = row.text.find("# ")?;
    let mut text = row.text.clone();
    text.replace_range(at..at + 2, "\u{25b8} ");
    let mut marked = row.clone();
    marked.text = text;
    marked.spans = vec![(at..at + '\u{25b8}'.len_utf8(), theme::cursor_marker())];
    Some(marked)
}

/// One row of a band: the rail cell, then the content and its fill.
///
/// The row's spans are the byte ranges it wants painted in a finer tone — a
/// card's `$` marker — and are clamped so a stale range cannot panic.
fn band_line(
    row: &Row,
    width: u16,
    band: ratatui::style::Style,
    rail: ratatui::style::Style,
) -> Line<'static> {
    let rail_span = Span::styled(RAIL_CHAR.to_string(), rail);
    let body = row.text.strip_prefix(RAIL_CHAR).unwrap_or(&row.text);
    // Span ranges are measured against `row.text`, so shift them past the rail.
    let shift = row.text.len() - body.len();

    let mut spans = vec![rail_span];
    let mut cursor = 0usize;
    for (range, style) in &row.spans {
        let start = range.start.saturating_sub(shift).min(body.len());
        let end = range.end.saturating_sub(shift).min(body.len());
        if start > cursor {
            spans.push(Span::styled(body[cursor..start].to_owned(), band));
        }
        if end > start {
            spans.push(Span::styled(body[start..end].to_owned(), *style));
        }
        cursor = cursor.max(end);
    }
    if cursor < body.len() {
        spans.push(Span::styled(body[cursor..].to_owned(), band));
    }

    // The fill to the full band width: `width` cells, minus the rail, minus
    // what the text already covers.
    let fill = usize::from(width).saturating_sub(display_width(&row.text));
    if fill > 0 {
        spans.push(Span::styled(" ".repeat(fill), band));
    }
    Line::from(spans)
}

/// Why there is nothing to show, wrapped and centred.
///
/// A workspace list comes back empty for one of two reasons that look identical
/// from here: no workspace has ever been opened, or this process opened a
/// **different database** from the one the desktop app uses. The default
/// database is relative to the working directory, so starting the terminal in
/// another directory is enough to do it. Naming the database turns a puzzle
/// into a fact.
fn draw_empty(frame: &mut Frame, area: Rect, app: &App) {
    let lines = empty_lines(app, area.width);
    let height = (lines.len() as u16).min(area.height);
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(lines).style(theme::canvas()).centered(),
        middle,
    );
}

fn empty_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    let texts: Vec<(&str, ratatui::style::Style)> = if app.active_workspace().is_none() {
        vec![
            ("No workspaces in this database.", theme::muted()),
            ("database:", theme::faint()),
            (app.database_url(), theme::faint()),
            (
                "Open one in the desktop app, or point ROBI_DATABASE_URL at its database.",
                theme::faint(),
            ),
        ]
    } else {
        vec![("No messages yet.", theme::muted())]
    };

    let mut lines = Vec::new();
    for (text, style) in texts {
        for wrapped in wrap_text(text, width) {
            lines.push(Line::from(Span::styled(wrapped, style)));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::persist::ViewState;
    use crate::tui::testing;

    #[test]
    fn the_empty_state_names_the_database_it_opened() {
        let mut app = App::new(
            ViewState::default(),
            "sqlite:///tmp/other.db",
            std::path::PathBuf::from("/tmp"),
            crate::tui::state::landing::Greeting::Morning,
        );
        app.resize(80, 24);
        let frame = testing::screen(&app, 80, 24);
        assert!(frame.contains("No workspaces in this database"), "{frame}");
        assert!(
            frame.contains("sqlite:///tmp/other.db"),
            "the database has to be named, or an empty list is a puzzle: {frame}"
        );
        assert!(frame.contains("ROBI_DATABASE_URL"), "{frame}");
    }
}

#[cfg(test)]
mod waiting_tests {
    use robi_core::message::Message;

    use crate::tui::app::Msg;
    use crate::tui::testing;

    use super::*;

    /// The pending card's label row, as drawn.
    fn label(app: &App) -> String {
        let frame = testing::rows(app, 80, 24);
        frame
            .into_iter()
            .find(|row| row.contains(tool_card::WAITING_LABEL))
            .unwrap_or_default()
    }

    /// A paused turn with one waiting card, landed and ready to draw.
    fn waiting_app() -> App {
        let (mut app, _, session) = testing::booted(80, 24);
        let call = testing::pending_call("shell", serde_json::json!({ "command": "rm -rf x" }));
        app.update(Msg::Agent(testing::approval_frame(session, call.id)));
        testing::land(
            &mut app,
            session,
            vec![Message::assistant_with_tool_calls("", vec![call])],
        );
        app
    }

    /// The dots on the label row, counted from the end of the un-padded text.
    fn dots(app: &App) -> usize {
        label(app)
            .trim_end()
            .chars()
            .rev()
            .take_while(|cell| *cell == '.')
            .count()
    }

    #[test]
    fn the_dots_advance_once_a_second_not_once_a_tick() {
        let mut app = waiting_app();
        // One sample per dot step: four ticks of 250 ms.
        let mut seen = Vec::new();
        for _ in 0..6 {
            seen.push(dots(&app));
            for _ in 0..4 {
                app.update(Msg::Tick);
            }
        }
        assert_eq!(
            seen,
            vec![1, 2, 3, 1, 2, 3],
            "the dots cycle, once a second"
        );

        // A single tick is not a step: three quarters of a second later it has not
        // moved yet.
        let mut app = waiting_app();
        let first = dots(&app);
        app.update(Msg::Tick);
        app.update(Msg::Tick);
        app.update(Msg::Tick);
        assert_eq!(dots(&app), first, "three ticks is not yet a second");
        app.update(Msg::Tick);
        assert_ne!(dots(&app), first, "the fourth tick is");
    }

    #[test]
    fn the_band_keeps_its_right_edge_while_the_dots_move() {
        // The failure this replaced: the row grew past the frame, the paragraph
        // clipped it, and the band's fill vanished for the last two dots. Measured
        // on an *unselected* waiting card, so the tone is the band's own rather
        // than the selection's.
        let mut app = waiting_app();
        // `Esc` clears the selection, which also returns focus to the composer; the
        // card stays waiting, which is the state that matters here.
        app.update(Msg::Key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Esc,
            ratatui::crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(app.focus(), crate::tui::state::Focus::Composer);

        let row_y = |app: &App| -> u16 {
            testing::rows(app, 80, 24)
                .iter()
                .position(|row| row.contains(tool_card::WAITING_LABEL))
                .expect("the label row") as u16
        };
        let rows_before = app.transcript().rows().len();
        let offset_before = app.transcript().offset();
        for step in 0..6 {
            let y = row_y(&app);
            // The last content column, inside the two-cell gutter.
            assert_eq!(
                testing::background(&app, 80, 24, 77, y),
                crate::tui::theme::CODE_BG,
                "the band reaches its right edge on step {step}"
            );
            for _ in 0..4 {
                app.update(Msg::Tick);
            }
        }
        // And nothing about the layout moved while it did: the marker animates in
        // the band's own padding, so no row is added and the offset is stable.
        assert_eq!(app.transcript().rows().len(), rows_before);
        assert_eq!(app.transcript().offset(), offset_before);
    }

    #[test]
    fn adding_a_dot_never_moves_the_ones_already_there() {
        // The complaint, encoded: the first dot must always sit in the same column,
        // so growing to two and three extends to the right instead of shifting.
        let mut app = waiting_app();
        let mut columns = Vec::new();
        for _ in 0..3 {
            let row = label(&app);
            columns.push(
                row.char_indices()
                    .filter(|(_, cell)| *cell == '.')
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>(),
            );
            for _ in 0..4 {
                app.update(Msg::Tick);
            }
        }
        assert_eq!(columns[0].len(), 1);
        assert_eq!(columns[1].len(), 2);
        assert_eq!(columns[2].len(), 3);
        // Each step keeps the columns it had and appends one.
        assert_eq!(columns[0], columns[1][..1], "the first dot did not move");
        assert_eq!(columns[1], columns[2][..2], "nor the first two");
        // Evenly spaced: one cell apart.
        assert_eq!(
            columns[2],
            vec![columns[2][0], columns[2][0] + 1, columns[2][0] + 2]
        );
    }

    #[test]
    fn the_label_reads_with_its_dots() {
        // The phrase, then the marker, then the band's own fill.
        let (mut app, _, _) = testing::booted(80, 24);
        let call = testing::pending_call("shell", serde_json::json!({ "command": "rm -rf x" }));
        app.update(Msg::Agent(testing::approval_frame(
            app.open_session_id().expect("a session"),
            call.id,
        )));
        let session = app.open_session_id().expect("a session");
        testing::land(
            &mut app,
            session,
            vec![Message::assistant_with_tool_calls("", vec![call])],
        );
        let mut drawn = Vec::new();
        for _ in 0..3 {
            drawn.push(label(&app).trim_end().to_owned());
            for _ in 0..4 {
                app.update(Msg::Tick);
            }
        }
        // The gutter's two cells, then the rail, then one cell of pad. The dot
        // field is three cells wide, so it reads the same however many are lit.
        let label = |dots: &str| format!("  {RAIL_CHAR} {}{dots}", tool_card::WAITING_LABEL);
        assert_eq!(
            drawn,
            vec![label("."), label(".."), label("...")],
            "{drawn:?}"
        );
    }

    #[test]
    fn a_settled_card_has_no_dots() {
        let (mut app, _, session) = testing::booted(80, 24);
        let mut ran = testing::pending_call("shell", serde_json::json!({ "command": "ls" }));
        ran.approval_status = robi_core::message::ApprovalStatus::Approved;
        ran.execution_status = robi_core::message::ExecutionStatus::Succeeded;
        testing::land(
            &mut app,
            session,
            vec![Message::assistant_with_tool_calls("", vec![ran])],
        );
        // The phrase does not appear at all once the call has run.
        assert!(
            testing::rows(&app, 80, 24)
                .iter()
                .all(|row| !row.contains(tool_card::WAITING_LABEL)),
            "{:?}",
            testing::rows(&app, 80, 24)
        );
    }
}
