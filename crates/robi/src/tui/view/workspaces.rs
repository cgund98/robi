//! The workspace switcher: the eight most recent, and the create screen.
//!
//! A panel over the frame, because it is a menu the reader leaves in one
//! keystroke. Managing every workspace is a view of its own
//! (`view/manage.rs`): reading paths and deleting rows is a place to stay, and a
//! menu is not.
//!
//! One panel with two screens, so `Esc` steps back a screen before it closes.
//! The recent screen ends with its two actions as rows, which is why the
//! selection is an index rather than an id.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::state::workspaces::Screen;
use crate::tui::theme;
use crate::tui::view::rows;

const MAX_WIDTH: u16 = 64;
const CHROME_ROWS: u16 = 2;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let view = app.workspaces_view();
    let list_rows = match view.screen {
        Screen::Recent => app.workspace_rows().len() + 2,
        Screen::New => 1 + view.candidates().len(),
    };
    let width = area.width.min(MAX_WIDTH);
    let inner_rows = u16::try_from(list_rows.max(1)).unwrap_or(u16::MAX);
    let height = inner_rows
        .saturating_add(CHROME_ROWS)
        .saturating_add(u16::from(view.screen == Screen::New))
        .min(area.height);
    if width < 8 || height <= CHROME_ROWS {
        return;
    }
    let panel = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    // Erase what the transcript drew here before painting the panel: the empty
    // state's centred sentence sits exactly where this panel lands.
    rows::fill(frame, panel);
    let title = match view.screen {
        Screen::Recent => " workspaces ",
        Screen::New => " workspaces \u{b7} new ",
    };
    let block = Block::bordered()
        .title(Span::styled(title, theme::strong()))
        .border_style(Style::default().fg(theme::INK_FAINT))
        .style(theme::canvas());
    let inner = block.inner(panel);
    frame.render_widget(block, panel);
    if inner.height == 0 {
        return;
    }

    match view.screen {
        Screen::Recent => draw_recent(frame, inner, app),
        Screen::New => draw_new(frame, inner, app),
    }
}

fn draw_recent(frame: &mut Frame, area: Rect, app: &App) {
    let view = app.workspaces_view();
    let workspaces = app.workspace_rows();
    let active = app.active_workspace_id();
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (index, workspace) in workspaces.iter().enumerate() {
        let (glyph, glyph_style) = if Some(workspace.id) == active {
            ('\u{25cf}', Style::default().fg(theme::ACCENT))
        } else {
            (' ', Style::default())
        };
        lines.push(rows::row(
            area.width,
            index == view.selected,
            vec![
                Span::styled(glyph.to_string(), glyph_style),
                Span::styled(format!(" {}", workspace.name), theme::prose()),
                Span::styled(format!("  {}", workspace.root), theme::faint()),
            ],
        ));
    }
    // The two actions are rows, so the keyboard reaches them the same way and
    // they are highlighted the same way.
    for (offset, label) in ["Manage workspaces\u{2026}", "New workspace\u{2026}"]
        .into_iter()
        .enumerate()
    {
        lines.push(rows::row(
            area.width,
            view.selected == workspaces.len() + offset,
            vec![Span::styled(label.to_owned(), theme::muted())],
        ));
    }
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), area);
}

fn draw_new(frame: &mut Frame, area: Rect, app: &App) {
    let view = app.workspaces_view();
    let [label_area, field_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("path", theme::muted()))).style(theme::canvas()),
        label_area,
    );
    frame.render_widget(view.path_field(), field_area);

    let lines: Vec<Line<'static>> = if view.candidates().is_empty() {
        vec![Line::from(Span::styled(
            "no directory matches \u{2014} Enter opens the path as typed",
            theme::faint(),
        ))]
    } else {
        view.candidates()
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                rows::row(
                    list_area.width,
                    Some(index) == view.candidate,
                    vec![Span::styled(format!(" {candidate}"), theme::prose())],
                )
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), list_area);
}

/// Re-exported so a test can assert the highlight without naming the theme.
pub const SELECTED_BG: Color = rows::SELECTED_BG;
