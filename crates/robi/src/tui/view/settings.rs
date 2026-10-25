//! The settings surface: a section rail, a pane of rows, and the editor band.
//!
//! The same dedicated-view shape `detail`, `manage`, and `help` use — a title
//! row, a body, one hint row — with a rail because there are six pages where the
//! other views have one list. The rows go through [`rows::row`], so the selection
//! is the same band every list in the app uses.
//!
//! **The band is the terminal's answer to a blur.** A webview saves a text field
//! when it loses focus; a terminal has no focus to lose, so `Enter` opens a band,
//! `Enter` applies, and the row it came from reads `editing…` until it closes.
//! Inline editing in one row could not hold a newline-separated path list, and a
//! row that grew in place would move the list under the reader.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::state::settings::Section;
use crate::tui::theme;
use crate::tui::view::rows;
use crate::tui::view::GUTTER;

/// Below this width the rail is not drawn: at `MIN_WIDTH` (40) a 19-column rail
/// would leave the pane 19 cells, and every value would clip to nothing. The
/// section then rides on the title row, and `h`/`l` still switch it.
const RAIL_MIN_WIDTH: u16 = 60;

/// The rail's own width: the widest section title plus its marker and gap.
const RAIL_WIDTH: u16 = 21;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let inner = Rect {
        x: area.x.saturating_add(GUTTER),
        width: area.width.saturating_sub(GUTTER * 2),
        ..area
    };
    let band = app.settings().editor_height();
    let [top, body, editor, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(band),
        Constraint::Length(1),
    ])
    .areas(inner);

    let section = app.settings().section();
    let title = if inner.width >= RAIL_MIN_WIDTH {
        "settings".to_owned()
    } else {
        format!("settings \u{b7} {}", section.title())
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("\u{2039} Esc", theme::muted()),
            Span::raw("  "),
            Span::styled(title, theme::strong()),
        ]))
        .style(theme::canvas()),
        top,
    );

    if inner.width >= RAIL_MIN_WIDTH {
        let [rail, pane] =
            Layout::horizontal([Constraint::Length(RAIL_WIDTH), Constraint::Min(1)]).areas(body);
        draw_rail(frame, rail, app);
        draw_pane(frame, pane, app);
    } else {
        draw_pane(frame, body, app);
    }

    if band > 0 {
        draw_editor(frame, editor, app);
    }

    draw_footer(frame, bottom, app);
}

fn draw_rail(frame: &mut Frame, area: Rect, app: &App) {
    let current = app.settings().section();
    let lines: Vec<Line<'static>> = Section::ALL
        .iter()
        .map(|section| {
            let selected = *section == current;
            let marker = if selected { '\u{25cf}' } else { ' ' };
            let style = if selected {
                theme::strong()
            } else {
                theme::faint()
            };
            rows::row(
                area.width,
                false,
                vec![Span::styled(
                    format!(" {marker} {}", section.title()),
                    style,
                )],
            )
        })
        .collect();
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), area);
}

fn draw_pane(frame: &mut Frame, area: Rect, app: &App) {
    let rows_now = app.settings_rows();
    if rows_now.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "nothing to configure here",
                theme::faint(),
            )))
            .style(theme::canvas()),
            area,
        );
        return;
    }
    let selected = app.settings().selected;
    let label_width = rows_now
        .iter()
        .map(|row| row.label.chars().count())
        .max()
        .unwrap_or(0)
        // The value needs a third of the pane to be worth anything; a long label
        // is clipped so a path can still be read.
        .min(usize::from(area.width).saturating_sub(12).max(8));

    // A row, plus the body an expanded audit row carries. The body is painted as
    // its own lines under the row it belongs to, so the window has to be taken
    // over lines rather than over rows.
    let mut painted: Vec<(Option<usize>, Line<'static>)> = Vec::new();
    for (index, row) in rows_now.iter().enumerate() {
        let is_selected = index == selected;
        let label = format!("{:<label_width$}", clip(&row.label, label_width));
        let value_style = if row.field.is_none() {
            theme::muted()
        } else if row.value == "\u{2014}" {
            theme::faint()
        } else {
            theme::prose()
        };
        painted.push((
            Some(index),
            rows::row(
                area.width,
                is_selected,
                vec![
                    Span::raw(" "),
                    Span::styled(label, theme::muted()),
                    Span::raw("  "),
                    Span::styled(row.value.clone(), value_style),
                ],
            ),
        ));
        if let Some(body) = body_of(app, index) {
            let width = usize::from(area.width).saturating_sub(6).max(8);
            for line in wrap(&body, width) {
                painted.push((
                    None,
                    Line::from(vec![Span::raw("    "), Span::styled(line, theme::faint())]),
                ));
            }
        }
    }

    // Keep the selected row visible, with the rows above it as the window allows.
    let anchor = painted
        .iter()
        .position(|(row, _)| *row == Some(selected))
        .unwrap_or(0);
    let first = anchor.saturating_sub(area.height.saturating_sub(1) as usize);
    let lines: Vec<Line<'static>> = painted
        .into_iter()
        .skip(first)
        .take(usize::from(area.height))
        .map(|(_, line)| line)
        .collect();
    frame.render_widget(Paragraph::new(lines).style(theme::canvas()), area);
}

/// The editor band: the field's name, then its lines.
fn draw_editor(frame: &mut Frame, area: Rect, app: &App) {
    let Some(field) = app.settings().editing_field() else {
        return;
    };
    let [heading, field_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
    let hint = match field.kind {
        crate::tui::state::settings::Kind::Secret => {
            " Enter save \u{b7} Esc revert \u{b7} empty leaves the stored key"
        }
        _ => " Enter save \u{b7} Esc revert \u{b7} empty clears the key",
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("  "),
            Span::styled(field.label, theme::strong()),
            Span::styled(hint, theme::faint()),
        ]))
        .style(theme::composer_surface()),
        heading,
    );
    if let Some(editor) = app.settings().editor() {
        frame.render_widget(editor, field_area);
    }
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let (text, style) = match app.settings().note() {
        Some(note) if note.error => (note.message.clone(), theme::error()),
        Some(note) => (note.message.clone(), theme::info()),
        None => (
            "Enter open \u{b7} h l section \u{b7} j k row \u{b7} Esc back".to_owned(),
            theme::faint(),
        ),
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, style))).style(theme::canvas()),
        area,
    );
}

/// The body under the row at `index`, when it is an expanded audit row.
///
/// Only the audit table carries one: every other row's value is the whole fact.
fn body_of(app: &App, index: usize) -> Option<String> {
    if app.settings().section() != Section::Audit || !app.audit().is_expanded(index) {
        return None;
    }
    app.audit()
        .entries()
        .get(index)
        .map(|entry| entry.body.clone())
}

/// Break `text` into lines no wider than `width`, on spaces where it can.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Cut `text` to `width` characters, marking the cut.
fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let mut kept: String = text.chars().take(width.saturating_sub(1)).collect();
    kept.push('\u{2026}');
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_label_is_cut_with_a_marker_and_a_short_one_is_not() {
        assert_eq!(clip("Mechanism", 12), "Mechanism");
        assert_eq!(clip("Container network", 9), "Containe\u{2026}");
    }

    #[test]
    fn a_body_wraps_on_spaces_and_never_past_the_width() {
        assert_eq!(wrap("one two three", 40), ["one two three"]);
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert!(wrap("a b c d e", 3).iter().all(|line| line.len() <= 3));
        assert!(wrap("", 10).is_empty());
    }
}
