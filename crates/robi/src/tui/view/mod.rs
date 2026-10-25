//! The frame: layout, the size guard, and one module per region.
//!
//! `draw` reads `App` and nothing else. It never mutates, never awaits, and
//! never decides anything a test cannot reproduce: the transcript rows were
//! wrapped in `state::transcript` and this module only styles them.

pub mod composer;
pub mod detail;
pub mod header;
pub mod landing;
pub mod notice;
pub mod manage;
pub mod missing;
pub mod prompt;
pub mod rows;
pub mod sessions;
pub mod strip;
pub mod transcript;
pub mod workspaces;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::tui::app::App;
use crate::tui::theme;

/// Below this, one centred sentence instead of a broken layout.
pub const MIN_WIDTH: u16 = 40;
/// Below this, one centred sentence instead of a broken layout.
pub const MIN_HEIGHT: u16 = 12;

/// Cells of canvas kept clear at the left and right of everything.
///
/// One gutter for the whole frame, not padding per region: the header, the
/// transcript, and the footer all share the same edges, so the screen reads as
/// one column with a margin rather than three boxes that happen to line up.
/// The transcript is wrapped to this width, so a card's band ends where the
/// header's text does.
pub const GUTTER: u16 = 2;

/// The columns the content occupies, after the gutter.
pub fn content_width(width: u16) -> u16 {
    width.saturating_sub(GUTTER * 2)
}

/// Rows of canvas kept clear above the header and around the hint row.
///
/// The frame's vertical counterpart to `GUTTER`: one blank row at the top edge,
/// one between the composer's underside and the hint, and one at the bottom
/// edge. The header and the hint are chrome, and chrome that touches the
/// terminal's edge reads as cut off rather than placed.
pub const FRAME_PAD: u16 = 1;

/// Rows of canvas between the transcript and the composer band.
///
/// The band is a raised surface; without a gap the last transcript row reads as
/// part of it. One row, the same "one blank row between blocks" the transcript
/// itself uses, so the spacing answers to one rule.
pub const COMPOSER_GAP: u16 = 1;

/// Rows of canvas between the status strip and the composer band.
///
/// The strip's words sit directly over the field, so without a gap the activity
/// line reads as part of the band's top edge. Zero when the strip is not drawn,
/// which keeps the count in `App::body_height` and this layout in step.
pub const STRIP_GAP: u16 = 1;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    // Paint the canvas first, so no cell keeps the terminal's own background.
    frame.render_widget(Block::default().style(theme::canvas()), area);

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        draw_too_small(frame, area);
        return;
    }

    // The dedicated views own the frame while they are up: each is a place the
    // reader went on purpose, and the composer underneath is not where they are.
    match app.mode() {
        crate::tui::state::Mode::Detail => {
            detail::draw(frame, area, app);
            return;
        }
        crate::tui::state::Mode::Manage => {
            manage::draw(frame, area, app);
            return;
        }
        _ => {}
    }

    // The page: the startup question while the path has no workspace, the chat
    // frame otherwise. The question stays the page when a switcher is opened
    // from it — closing that overlay returns here, not to a chat with nothing
    // open behind it.
    if app.missing_workspace_page() {
        missing::draw(frame, area, app);

        // Only the switcher can sit over the question: a sessions list with no
        // workspace would be an empty panel, and the overlays' own keys are the
        // only thing worth offering here.
        if app.workspaces_open() {
            workspaces::draw(frame, inner(area), app);
        }
        return;
    }

    let content = inner(area);
    // The landing page: a workspace with no session open. A greeting and the
    // composer as one centred group, and no transcript to lay out — nothing is
    // open yet, and `Ctrl-N` deliberately creates nothing, so this page *is* the
    // new chat until the first message is sent.
    if app.landing() {
        let [top, rule, body, _, bottom] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(FRAME_PAD),
            Constraint::Length(1),
        ])
        .areas(content);
        header::draw(frame, top, app);
        header::rule(frame, rule);
        landing::draw(frame, body, app);
        notice::draw(frame, bottom, app);
        draw_overlays(frame, content, app);
        return;
    }

    // Seven regions from T3 on: the header and the rule under it, the transcript,
    // the gap, the status strip and the gap under it, the composer band, and the
    // prompt row and footer beneath it. `content` has already given up the blank
    // row at each edge. The band's height comes from the field, which grows with
    // its text up to `MAX_COMPOSER_ROWS`; the strip, its gap, and the prompt row
    // exist only while there is something to say, so `App::body_height` and this
    // layout agree by construction. Nothing status-like is drawn below the
    // composer: the strip is where a reader looks, and the footer under the band
    // is only the hint.
    let band = app.composer().height();
    let [top, rule, body, _, strip, _, composer, prompt_row, _, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(COMPOSER_GAP),
        Constraint::Length(app.status_rows()),
        Constraint::Length(app.strip_gap()),
        Constraint::Length(band),
        Constraint::Length(app.prompt_rows()),
        Constraint::Length(FRAME_PAD),
        Constraint::Length(1),
    ])
    .areas(content);

    header::draw(frame, top, app);
    header::rule(frame, rule);
    transcript::draw(frame, body, app);
    // The gaps and the pad row are deliberately undrawn: the canvas is already
    // painted.
    if app.status_rows() > 0 {
        strip::draw(frame, strip, app);
    }
    composer::draw(frame, composer, app);
    if app.prompt_rows() > 0 {
        prompt::draw(frame, prompt_row, app);
    }
    notice::draw(frame, bottom, app);

    draw_overlays(frame, content, app);
}

/// The overlays: panels over the transcript, drawn last so they cover it.
///
/// They own the keyboard while they are up, which is why covering the frame is
/// right here and was not right for the approval.
fn draw_overlays(frame: &mut Frame, content: Rect, app: &App) {
    if app.sessions_open() {
        sessions::draw(frame, content, app);
    }
    if app.workspaces_open() {
        workspaces::draw(frame, content, app);
    }
}

/// `area` inset by the gutter on both sides and by `FRAME_PAD` top and bottom.
///
/// One rect for every region, so the header, the transcript, and the hint share
/// the frame's edges and the blank rows around them answer to one rule.
fn inner(area: Rect) -> Rect {
    let top = FRAME_PAD.min(area.height);
    let rest = area.height - top;
    Rect {
        x: area.x.saturating_add(GUTTER),
        y: area.y.saturating_add(top),
        width: content_width(area.width),
        height: rest.saturating_sub(FRAME_PAD.min(rest)),
    }
}

fn draw_too_small(frame: &mut Frame, area: Rect) {
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(Line::from(
            "terminal too small \u{2014} 40x12 minimum"
                .to_string()
                .fg(theme::INK_MUTED),
        ))
        .style(theme::canvas())
        .centered(),
        middle,
    );
}
