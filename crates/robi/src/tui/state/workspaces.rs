//! The workspace switcher: two screens, the recents order, and path completion.
//!
//! The terminal's peer of `WorkspaceSwitcher` (the eight most recent) plus the
//! creation half of `WorkspacesPage`. Managing every workspace is a dedicated
//! view of its own (`state::manage`), because reading paths and deleting rows is
//! a place the reader stays; this overlay is a menu they leave in one keystroke.
//! The GUI's folder dialog has no terminal equivalent, so the create screen is a
//! typed path with directory completion over the same resolver the tools use.

use ratatui::crossterm::event::KeyEvent;
use ratatui::style::Style;
use ratatui_textarea::TextArea;
use robi_core::ids::WorkspaceId;

use crate::domain::workspace::model::Workspace;

/// Workspaces the recent screen lists, as the GUI's switcher does.
pub const RECENT_LIMIT: usize = 8;

/// Candidates the path field offers at once.
pub const CANDIDATE_LIMIT: usize = 8;

/// Which of the switcher's two screens owns the keys.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Screen {
    /// The eight most recent, then `Manage workspaces…` and `New workspace…`.
    #[default]
    Recent,
    /// A typed path, with completion.
    New,
}

/// The workspaces a query matches, by name or by root.
pub fn filtered<'a>(workspaces: &'a [Workspace], query: &str) -> Vec<&'a Workspace> {
    let needle = query.trim().to_lowercase();
    workspaces
        .iter()
        .filter(|workspace| {
            needle.is_empty()
                || workspace.name.to_lowercase().contains(&needle)
                || workspace.root.to_lowercase().contains(&needle)
        })
        .collect()
}

/// Most recently selected first, then the service's own order.
///
/// The service lists by `created_at DESC`, so a workspace that was opened once
/// and never touched again keeps a stable place in the tail.
pub fn order_recent<'a>(workspaces: &'a [Workspace], recent: &[WorkspaceId]) -> Vec<&'a Workspace> {
    let mut ordered: Vec<&Workspace> = Vec::with_capacity(workspaces.len());
    for id in recent {
        if let Some(workspace) = workspaces.iter().find(|workspace| workspace.id == *id) {
            if !ordered.iter().any(|held| held.id == workspace.id) {
                ordered.push(workspace);
            }
        }
    }
    for workspace in workspaces {
        if !ordered.iter().any(|held| held.id == workspace.id) {
            ordered.push(workspace);
        }
    }
    ordered
}

/// Split a typed path into the directory part and the name being completed.
///
/// The directory part keeps its separator, so a candidate is the typed text with
/// its last segment replaced. `~` is a directory in its own right: completing it
/// lists the home directory rather than looking for a folder called `~` in the
/// working directory.
pub fn split_path(text: &str) -> (&str, &str) {
    if text == "~" {
        return ("~/", "");
    }
    match text.rfind('/') {
        Some(index) => (&text[..=index], &text[index + 1..]),
        None => ("", text),
    }
}

/// The typed prefix with `name` in place of the segment being completed.
pub fn candidate(prefix: &str, name: &str) -> String {
    format!("{prefix}{name}")
}

/// The overlay's own state: the screen, the selection, and the two fields.
#[derive(Debug)]
pub struct WorkspacesView {
    pub screen: Screen,
    /// The row the keys act on, per screen.
    pub selected: usize,
    /// The highlighted candidate on the create screen.
    pub candidate: Option<usize>,
    filter: TextArea<'static>,
    path: TextArea<'static>,
    candidates: Vec<String>,
}

impl Default for WorkspacesView {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspacesView {
    pub fn new() -> Self {
        let mut filter = TextArea::default();
        filter.set_cursor_line_style(Style::default());
        let mut path = TextArea::default();
        path.set_cursor_line_style(Style::default());
        Self {
            screen: Screen::Recent,
            selected: 0,
            candidate: None,
            filter,
            path,
            candidates: Vec::new(),
        }
    }

    /// Open on the recent list, with the active workspace selected.
    pub fn open(&mut self, active: Option<WorkspaceId>, recent: &[&Workspace]) {
        self.screen = Screen::Recent;
        self.filter = TextArea::default();
        self.filter.set_cursor_line_style(Style::default());
        self.path = TextArea::default();
        self.path.set_cursor_line_style(Style::default());
        self.candidates.clear();
        self.candidate = None;
        self.selected = active
            .and_then(|id| recent.iter().position(|workspace| workspace.id == id))
            .unwrap_or(0);
    }

    pub fn query(&self) -> String {
        self.filter.lines().join(" ")
    }

    /// The manage screen's filter, for the frame.
    pub fn filter(&self) -> &TextArea<'static> {
        &self.filter
    }

    /// The create screen's path field, for the frame.
    pub fn path_field(&self) -> &TextArea<'static> {
        &self.path
    }

    pub fn path(&self) -> String {
        self.path.lines().join("")
    }

    /// Empty the path field and everything it completed to.
    pub fn reset_path(&mut self) {
        self.path = TextArea::default();
        self.path.set_cursor_line_style(Style::default());
        self.candidates.clear();
        self.candidate = None;
    }

    pub fn input_filter(&mut self, key: &KeyEvent) {
        self.filter.input(*key);
    }

    pub fn input_path(&mut self, key: &KeyEvent) {
        self.path.input(*key);
    }

    pub fn candidates(&self) -> &[String] {
        &self.candidates
    }

    /// Apply a completion answer, if it is for the text still in the field.
    pub fn set_candidates(&mut self, for_text: &str, candidates: Vec<String>) {
        if self.path() != for_text {
            return;
        }
        self.candidates = candidates;
        self.candidate = (!self.candidates.is_empty()).then_some(0);
    }

    /// Put the highlighted candidate in the field, ready to be typed on from.
    ///
    /// The trailing separator means a second `Tab` descends into the directory
    /// without another keystroke, which is how a path is usually walked.
    pub fn accept_candidate(&mut self) -> bool {
        let Some(index) = self.candidate else {
            return false;
        };
        let Some(candidate) = self.candidates.get(index).cloned() else {
            return false;
        };
        let mut field = TextArea::default();
        field.set_cursor_line_style(Style::default());
        field.insert_str(format!("{candidate}/"));
        field.move_cursor(ratatui_textarea::CursorMove::End);
        self.path = field;
        self.candidates.clear();
        self.candidate = None;
        true
    }

    /// Move the highlighted candidate, clamped.
    pub fn select_candidate(&mut self, delta: isize) {
        if self.candidates.is_empty() {
            self.candidate = None;
            return;
        }
        let current = self.candidate.unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, self.candidates.len() as isize - 1) as usize;
        self.candidate = Some(next);
    }

    /// Move the row selection within `rows`, clamped.
    pub fn select(&mut self, delta: isize, rows: usize) {
        if rows == 0 {
            self.selected = 0;
            return;
        }
        let next = (self.selected as isize + delta).clamp(0, rows as isize - 1) as usize;
        self.selected = next;
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn workspace(name: &str) -> Workspace {
        Workspace {
            id: WorkspaceId::new(),
            name: name.to_owned(),
            root: format!("/code/{name}"),
            mcp_project_sha256: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn the_recent_order_is_the_recorded_order_then_the_services_own() {
        let one = workspace("one");
        let two = workspace("two");
        let three = workspace("three");
        let list = vec![three.clone(), two.clone(), one.clone()];

        let ordered = order_recent(&list, &[two.id, one.id]);
        assert_eq!(
            ordered.iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![two.id, one.id, three.id]
        );

        // An id that is gone is skipped, and a duplicate is not listed twice.
        let ordered = order_recent(&list, &[WorkspaceId::new(), three.id, three.id]);
        assert_eq!(
            ordered.iter().map(|w| w.id).collect::<Vec<_>>(),
            vec![three.id, two.id, one.id]
        );
    }

    #[test]
    fn the_filter_matches_the_name_or_the_root() {
        let list = vec![workspace("robi"), workspace("pocket")];
        assert_eq!(filtered(&list, "").len(), 2);
        assert_eq!(filtered(&list, "pock")[0].id, list[1].id);
        assert_eq!(filtered(&list, "/code/robi")[0].id, list[0].id);
        assert!(filtered(&list, "nothing").is_empty());
    }

    #[test]
    fn a_path_splits_into_the_directory_and_the_segment_being_typed() {
        assert_eq!(split_path(""), ("", ""));
        assert_eq!(split_path("code"), ("", "code"));
        assert_eq!(split_path("/a/b"), ("/a/", "b"));
        assert_eq!(split_path("/a/b/"), ("/a/b/", ""));
        assert_eq!(split_path("~/code/r"), ("~/code/", "r"));
        assert_eq!(split_path("~"), ("~/", ""));
        assert_eq!(candidate("~/code/", "robi"), "~/code/robi");
    }

    fn typed(view: &mut WorkspacesView, text: &str) {
        for ch in text.chars() {
            view.input_path(&ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::Char(ch),
                ratatui::crossterm::event::KeyModifiers::NONE,
            ));
        }
    }

    #[test]
    fn a_candidate_is_accepted_into_the_field_and_walked_on_from() {
        let mut view = WorkspacesView::new();
        typed(&mut view, "~/code/r");
        view.set_candidates("~/code/r", vec!["~/code/robi".to_owned()]);
        assert!(view.accept_candidate());
        assert_eq!(view.path(), "~/code/robi/");
        assert!(!view.accept_candidate(), "nothing is highlighted now");

        // An answer for text the field no longer holds is dropped.
        view.input_path(&ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Backspace,
            ratatui::crossterm::event::KeyModifiers::NONE,
        ));
        view.set_candidates("stale", vec!["/nope".to_owned()]);
        assert!(view.candidates().is_empty());
    }

    #[test]
    fn the_selection_clamps_on_both_screens() {
        let mut view = WorkspacesView::new();
        view.select(1, 3);
        assert_eq!(view.selected, 1);
        view.select(5, 3);
        assert_eq!(view.selected, 2);
        view.select(-9, 3);
        assert_eq!(view.selected, 0);
        view.select(1, 0);
        assert_eq!(view.selected, 0);

        view.set_candidates("", vec!["/a".to_owned(), "/b".to_owned()]);
        assert_eq!(view.candidate, Some(0));
        view.select_candidate(1);
        assert_eq!(view.candidate, Some(1));
        view.select_candidate(4);
        assert_eq!(view.candidate, Some(1));
    }
}
