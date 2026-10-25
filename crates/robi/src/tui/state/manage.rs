//! The workspaces view: every workspace, filtered, with remove.
//!
//! A dedicated view rather than a screen of the switcher overlay. The switcher
//! is a menu: eight rows and two actions, gone in one keystroke. Managing is a
//! place the reader stays while they read paths, filter, and delete, so it takes
//! the frame the way the detail view does.

use ratatui::crossterm::event::KeyEvent;
use ratatui::style::Style;
use ratatui_textarea::TextArea;

/// The view's own state: the filter and the row the keys act on.
#[derive(Debug)]
pub struct ManageView {
    filter: TextArea<'static>,
    pub selected: usize,
}

impl Default for ManageView {
    fn default() -> Self {
        Self::new()
    }
}

impl ManageView {
    pub fn new() -> Self {
        let mut filter = TextArea::default();
        filter.set_cursor_line_style(Style::default());
        filter.set_placeholder_text("filter by name or path");
        Self {
            filter,
            selected: 0,
        }
    }

    /// Open with the active workspace selected and no filter.
    ///
    /// The query clears on open, as the switcher's does: a filter left from last
    /// time would hide the row the reader came to look at.
    pub fn open(&mut self, active_index: Option<usize>) {
        self.filter = TextArea::default();
        self.filter.set_cursor_line_style(Style::default());
        self.filter.set_placeholder_text("filter by name or path");
        self.selected = active_index.unwrap_or(0);
    }

    pub fn query(&self) -> String {
        self.filter.lines().join(" ")
    }

    pub fn field(&self) -> &TextArea<'static> {
        &self.filter
    }

    pub fn input(&mut self, key: &KeyEvent) {
        self.filter.input(*key);
    }

    /// Move the selection within `rows`, clamped.
    pub fn select(&mut self, delta: isize, rows: usize) {
        if rows == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as isize + delta).clamp(0, rows as isize - 1) as usize;
    }

    /// Keep the selection inside a list that just changed under it.
    pub fn reconcile(&mut self, rows: usize) {
        if rows == 0 {
            self.selected = 0;
        } else if self.selected >= rows {
            self.selected = rows - 1;
        }
    }
}
