//! The composer: the text area, and one draft per session.
//!
//! The terminal's peer of `src/features/chat/Composer.tsx` and
//! `src/state/composerDrafts.ts`. Both hold drafts in memory and key them by
//! session id, with a bare session-less key for a chat with no row yet. This
//! module does the same, and nothing here reaches `~/.robi/tui.json`: typed
//! text can be a secret, and the GUI does not persist it either.
//!
//! The text area is `ratatui_textarea::TextArea`. Keeping it here rather than
//! in `App` means the drafts, the buffer, and the in-flight flag move together,
//! and `App` never has to know how a keystroke becomes a character.

use std::collections::BTreeMap;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
use ratatui_textarea::{CursorMove, TextArea};
use robi_core::ids::SessionId;

/// The most rows the field grows to before it scrolls instead.
///
/// A terminal keeps the transcript on screen; a composer that can eat the whole
/// frame is a transcript with no room to read the reply in.
pub const MAX_COMPOSER_ROWS: u16 = 6;

/// What the empty field says.
///
/// Kept short: the widget does not clip its placeholder to the field, so a long
/// one would paint past the frame on a narrow terminal. The newline binding is
/// in the footer instead.
pub const PLACEHOLDER: &str = "Message robi\u{2026}";

/// The draft key for a chat with no session row yet.
///
/// The GUI's `composerDrafts.ts` uses `"draft"`. A session id is a UUID, so the
/// two keys can never collide.
const DRAFT_KEY: &str = "draft";

/// The composer's buffer and the drafts that survive a session switch.
#[derive(Debug)]
pub struct Composer {
    textarea: TextArea<'static>,
    /// Unsent text, keyed the same way the GUI keys it.
    drafts: BTreeMap<String, String>,
    /// A submit is in flight. The field is locked for its duration.
    sending: bool,
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

impl Composer {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        // The widget underlines the cursor's line by default, which reads as
        // emphasis this design does not use. The band and the cursor are the
        // affordance.
        textarea.set_cursor_line_style(Style::default());
        textarea.set_placeholder_text(PLACEHOLDER);
        Self {
            textarea,
            drafts: BTreeMap::new(),
            sending: false,
        }
    }

    pub fn textarea(&self) -> &TextArea<'static> {
        &self.textarea
    }

    /// The field itself, for the completion menu's rewrite.
    ///
    /// Nothing outside this module inserts text otherwise, and opening the buffer
    /// is what lets the menu replace a token in place without `Composer` knowing
    /// what a completion is.
    pub fn textarea_mut(&mut self) -> &mut TextArea<'static> {
        &mut self.textarea
    }

    /// The buffer as it would be sent: the lines joined, without the trailing
    /// newline `TextArea` always keeps.
    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Nothing to send: empty, or only whitespace.
    pub fn is_blank(&self) -> bool {
        self.text().trim().is_empty()
    }

    pub fn sending(&self) -> bool {
        self.sending
    }

    pub fn set_sending(&mut self, sending: bool) {
        self.sending = sending;
    }

    /// Feed one key to the text area.
    ///
    /// Global and mode bindings are claimed by `keys::command` first, so
    /// anything that reaches here belongs to the field.
    pub fn input(&mut self, key: &KeyEvent) {
        self.textarea.input(*key);
    }

    /// `Alt-Enter` and `Ctrl-J`: a newline without sending.
    pub fn insert_newline(&mut self) {
        self.textarea.insert_newline();
    }

    /// The text that was sent, and an empty buffer.
    ///
    /// Called when a submit succeeds, never when it is issued: a failed send
    /// has to leave the buffer exactly as it was.
    pub fn take_sent(&mut self) -> String {
        let text = self.text();
        self.clear();
        text
    }

    /// Empty the field, keeping no draft for it.
    pub fn clear(&mut self) {
        self.textarea = Composer::new().textarea;
    }

    /// Put text back, for a refused send.
    pub fn restore(&mut self, text: &str) {
        self.textarea = TextArea::new(text.split('\n').map(str::to_owned).collect());
        self.textarea.set_cursor_line_style(Style::default());
        self.textarea.move_cursor(CursorMove::Bottom);
    }

    /// Move the buffer from `previous` to `next`, saving and restoring drafts.
    ///
    /// `previous` is `None` for a chat that has no session row yet, which is
    /// the `"draft"` key.
    pub fn switch_to(&mut self, previous: Option<SessionId>, next: Option<SessionId>) {
        let from = draft_key(previous);
        let to = draft_key(next);
        if from == to {
            return;
        }
        let text = self.text();
        if text.trim().is_empty() {
            self.drafts.remove(&from);
        } else {
            self.drafts.insert(from, text);
        }
        match self.drafts.get(&to) {
            Some(text) => {
                let text = text.clone();
                self.restore(&text);
            }
            None => self.clear(),
        }
    }

    /// How many rows the field needs, including the band's two padding rows.
    /// How many rows the band needs.
    ///
    /// One padding row above the text, the text, a blank row, the facts row, and a
    /// blank row under it. The gap above keeps the mode and model line from reading
    /// as part of the message being typed; the gap below keeps it off the band's
    /// edge, so it reads as a caption rather than as a last line.
    pub fn height(&self) -> u16 {
        let lines = self
            .textarea
            .lines()
            .len()
            .clamp(1, usize::from(MAX_COMPOSER_ROWS));
        (lines as u16).saturating_add(4)
    }
}

fn draft_key(session: Option<SessionId>) -> String {
    match session {
        Some(id) => id.to_string(),
        None => DRAFT_KEY.to_owned(),
    }
}

/// Whether `key` is one the field should receive at all.
///
/// `Ctrl-C` is a global binding, so it never reaches the text area; that is
/// stated here rather than left to the `match` order in `app::update`.
pub fn is_field_key(key: &KeyEvent) -> bool {
    !(key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn a_draft_survives_a_session_switch_and_a_sent_one_does_not() {
        let first = SessionId::new();
        let second = SessionId::new();
        let mut composer = Composer::new();

        composer.input(&plain(KeyCode::Char('a')));
        composer.input(&plain(KeyCode::Char('b')));
        composer.switch_to(Some(first), Some(second));
        assert!(composer.is_blank(), "the other session starts empty");
        assert!(!composer.sending());

        composer.input(&plain(KeyCode::Char('c')));
        composer.switch_to(Some(second), Some(first));
        assert_eq!(composer.text(), "ab", "the first draft came back");

        // A sent draft is gone: the buffer is cleared and nothing is kept.
        assert_eq!(composer.take_sent(), "ab");
        assert!(composer.is_blank());
        composer.switch_to(Some(first), Some(second));
        composer.switch_to(Some(second), Some(first));
        assert!(
            composer.is_blank(),
            "a sent draft must not come back on the next switch"
        );
    }
}
