//! The workspaces view: every workspace, filtered, with remove.
//!
//! A view, not an overlay: it owns the frame the way the detail view does, with
//! its own title row, filter, list, and hint. Managing is a place the reader
//! stays while they read paths and delete rows; the switcher is a menu they leave
//! in one keystroke, and stacking a full list on top of it read as the same thing
//! twice.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;
use crate::tui::view::rows;
use crate::tui::view::GUTTER;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let inner = Rect {
        x: area.x.saturating_add(GUTTER),
        width: area.width.saturating_sub(GUTTER * 2),
        ..area
    };
    let [top, filter_area, list_area, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let rows_now = app.manage_rows();
    let title = format!(
        "workspaces \u{b7} {} of {}",
        if rows_now.is_empty() {
            0
        } else {
            app.manage().selected + 1
        },
        rows_now.len()
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("\u{2039} Esc", theme::muted()),
            Span::raw("  "),
            Span::styled(title, theme::strong()),
        ]))
        .style(theme::canvas()),
        top,
    );

    frame.render_widget(app.manage().field(), filter_area);

    let active = app.active_workspace_id();
    let lines: Vec<Line<'static>> = if rows_now.is_empty() {
        vec![Line::from(Span::styled(
            "no workspace matches the filter",
            theme::faint(),
        ))]
    } else {
        rows_now
            .iter()
            .enumerate()
            .map(|(index, workspace)| {
                let (glyph, glyph_style) = if Some(workspace.id) == active {
                    (
                        '\u{25cf}',
                        ratatui::style::Style::default().fg(theme::ACCENT),
                    )
                } else {
                    (' ', ratatui::style::Style::default())
                };
                rows::row(
                    list_area.width,
                    index == app.manage().selected,
                    vec![
                        Span::styled(glyph.to_string(), glyph_style),
                        Span::styled(format!(" {}", workspace.name), theme::prose()),
                        Span::styled(format!("  {}", workspace.root), theme::faint()),
                    ],
                )
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), list_area);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Enter open \u{b7} typing filters \u{b7} Ctrl-N new \u{b7} \
             Ctrl-D remove \u{b7} Esc back",
            theme::faint(),
        )))
        .style(theme::canvas()),
        bottom,
    );
}
