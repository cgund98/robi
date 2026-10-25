//! The one-line prompt: the reject reason.
//!
//! The GUI has a reason field inside its approval bar. Here the field is a mode
//! of its own, because a terminal has one keyboard and both the list and the
//! field cannot own it: while the prompt is up, every printable key is text.
//!
//! The buffer itself lives on [`Approval`](super::approval::Approval), so closing
//! this and reopening it restores what was typed. This module only owns the mode.

use ratatui::crossterm::event::KeyEvent;
use ratatui_textarea::TextArea;
use robi_core::ids::SessionId;

/// What the prompt is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// Why the selected call is being refused.
    RejectReason,
    /// The new title for a session.
    RenameSession(SessionId),
}

impl PromptKind {
    /// The hint under the field.
    pub fn hint(self) -> &'static str {
        match self {
            Self::RejectReason => {
                "Enter reject \u{b7} Esc back \u{b7} empty sends the default reason"
            }
            Self::RenameSession(_) => "Enter save \u{b7} Esc cancel \u{b7} empty is refused",
        }
    }
}

/// The prompt's handle on the shared text area.
#[derive(Debug)]
pub struct Prompt {
    pub kind: PromptKind,
    /// What is about to be refused, so the field is never asked in a vacuum.
    pub label: String,
    /// The buffer to draw. The text belongs to `Approval`; this is a clone, kept
    /// in step on push and pop so the editor is never one keystroke behind.
    pub field: TextArea<'static>,
}

impl Prompt {
    pub fn new(kind: PromptKind) -> Self {
        let mut field = TextArea::default();
        field.set_cursor_line_style(ratatui::style::Style::default());
        Self {
            kind,
            label: String::new(),
            field,
        }
    }

    /// Feed one key to the field.
    pub fn input(&mut self, key: &KeyEvent) {
        self.field.input(*key);
    }

    /// The field's text, as typed.
    pub fn text(&self) -> String {
        self.field.lines().join("\n")
    }
}
