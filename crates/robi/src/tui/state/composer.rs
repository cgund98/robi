//! The composer: the text area, and one draft per session.
//!
//! The terminal's peer of `src/features/chat/Composer.tsx` and
//! `src/state/composerDrafts.ts`. Both hold drafts in memory and key them by
//! session id, with a bare session-less key for a chat with no row yet. This
//! module does the same, and nothing here reaches `~/.robi/tui.json`: typed
//! text can be a secret, and the GUI does not persist it either.
//!
//! The text area is `ratatui_textarea::TextArea`. Keeping it here rather than
//! in `App` means the drafts, the buffer, the attachments, and the in-flight flag
//! move together, and `App` never has to know how a keystroke becomes a character.
//!
//! **Attachments are draft state, not session state.** `/attach` puts a file on
//! the message being composed; the session only ever sees them once a send is
//! taken. So they are cleared by `take_sent`, they go with the draft on a session
//! switch, and an image's bytes wait here until the session they belong to
//! exists — see [`Pending`].

use std::collections::BTreeMap;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
use ratatui_textarea::{CursorMove, TextArea};
use robi_core::ids::SessionId;
use robi_core::message::FileAttachment;

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

/// One attachment that has not been sent yet.
///
/// A text file is already sliced and carries its own bytes; an image is not,
/// because the image store is keyed by session and the landing page has no
/// session yet. So the bytes wait here and `effects` writes them at submit time.
#[derive(Clone, PartialEq)]
pub enum Pending {
    File(FileAttachment),
    Image(PendingImage),
}

/// An image waiting for a session.
///
/// The bytes are here because the image store is keyed by session and the landing
/// page has no session yet: `effects` writes them at submit time, once the row
/// exists. `robi-core`'s `ImageAttachment` stays `{ id, media_type }` — the name
/// and the weight are the composer's, and are never stored on the message.
#[derive(Clone, PartialEq)]
pub struct PendingImage {
    pub name: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for PendingImage {
    /// A length, not a megabyte of pixels: this is printed by every failing test
    /// that happens to hold one.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingImage")
            .field("name", &self.name)
            .field("media_type", &self.media_type)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// An attachment and the source the reader named it by.
///
/// `source` is what `/detach` matches and what a refusal quotes: the attachment's
/// own `name` is a basename, which two files can share.
#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub source: String,
    pub pending: Pending,
}

impl Pending {
    /// The name a chip and a refusal show.
    pub fn name(&self) -> &str {
        match self {
            Self::File(file) => &file.name,
            Self::Image(image) => &image.name,
        }
    }

    /// Everything the model will be asked to read, for the total-size cap.
    ///
    /// An image is not counted against the text total — its own cap is separate
    /// and much larger.
    pub fn text_len(&self) -> usize {
        match self {
            Self::File(file) => file.text.len(),
            Self::Image(_) => 0,
        }
    }
}

// Bytes are the reason this is hand-written: the derive would print a
// megabyte of image data into any failing test's message.
impl std::fmt::Debug for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(file) => f
                .debug_struct("File")
                .field("name", &file.name)
                .field("path", &file.path)
                .field("bytes", &file.text.len())
                .finish(),
            Self::Image(image) => f.debug_tuple("Image").field(image).finish(),
        }
    }
}

/// Why `/detach <name>` removed nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetachError {
    /// No attachment answers to that name.
    NotFound(String),
    /// More than one does, and picking one would be a guess.
    Ambiguous(Vec<String>),
}

impl DetachError {
    /// The one line the footer shows.
    pub fn message(&self) -> String {
        match self {
            Self::NotFound(name) => format!("{name} is not attached"),
            Self::Ambiguous(sources) => {
                format!("{} are attached \u{2014} name one", sources.join(", "))
            }
        }
    }
}

/// The composer's buffer and the drafts that survive a session switch.
#[derive(Debug)]
pub struct Composer {
    textarea: TextArea<'static>,
    /// Unsent text, keyed the same way the GUI keys it.
    drafts: BTreeMap<String, String>,
    /// A submit is in flight. The field is locked for its duration.
    sending: bool,
    /// Files and images on the message being composed, in the order they were
    /// attached. Never sent until a submit succeeds.
    attachments: Vec<Attachment>,
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
            attachments: Vec::new(),
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

    /// Nothing to send: no text and no attachments.
    ///
    /// An attachment alone is sendable — `submit_instruction` accepts an empty
    /// instruction with files or images — so this is the GUI's `canSend`, not a
    /// text-only test.
    pub fn is_blank(&self) -> bool {
        self.text().trim().is_empty() && self.attachments.is_empty()
    }

    /// The attachments on the message being composed.
    pub fn attachments(&self) -> &[Attachment] {
        &self.attachments
    }

    /// The images waiting for a session to be stored under.
    pub fn pending_images(&self) -> Vec<PendingImage> {
        self.attachments
            .iter()
            .filter_map(|attachment| match &attachment.pending {
                Pending::Image(image) => Some(image.clone()),
                Pending::File(_) => None,
            })
            .collect()
    }

    /// The text files this message will carry.
    pub fn pending_files(&self) -> Vec<FileAttachment> {
        self.attachments
            .iter()
            .filter_map(|attachment| match &attachment.pending {
                Pending::File(file) => Some(file.clone()),
                Pending::Image(_) => None,
            })
            .collect()
    }

    /// How many attachments are held, text files and images together.
    pub fn attachment_count(&self) -> usize {
        self.attachments.len()
    }

    /// The text bytes every text attachment adds up to.
    pub fn text_total(&self) -> usize {
        self.attachments
            .iter()
            .map(|attachment| attachment.pending.text_len())
            .sum()
    }

    /// Whether `source` is already attached, so a second read is not needed.
    pub fn is_attached(&self, source: &str) -> bool {
        self.attachments
            .iter()
            .any(|attachment| attachment.source == source)
    }

    /// Put one attachment on the message.
    pub fn attach(&mut self, attachment: Attachment) {
        self.attachments.push(attachment);
    }

    /// Remove every attachment, and say how many went.
    pub fn detach_all(&mut self) -> usize {
        let count = self.attachments.len();
        self.attachments.clear();
        count
    }

    /// Remove the last attachment, for `Backspace` in an empty field.
    pub fn detach_last(&mut self) -> Option<Attachment> {
        self.attachments.pop()
    }

    /// Remove one attachment by the source the reader named.
    ///
    /// An exact source wins; otherwise a name that is the basename of exactly one
    /// attachment matches. Two matches are [`DetachError::Ambiguous`] rather than a
    /// guess, because `/detach mod.rs` with two `mod.rs` files could quietly take
    /// the wrong one and the reader would not see it until the model read the
    /// wrong file.
    pub fn detach(&mut self, name: &str) -> Result<Attachment, DetachError> {
        if let Some(index) = self
            .attachments
            .iter()
            .position(|attachment| attachment.source == name)
        {
            return Ok(self.attachments.remove(index));
        }
        let matched: Vec<usize> = self
            .attachments
            .iter()
            .enumerate()
            .filter(|(_, attachment)| basename(&attachment.source) == name)
            .map(|(index, _)| index)
            .collect();
        match matched.as_slice() {
            [] => Err(DetachError::NotFound(name.to_owned())),
            [index] => Ok(self.attachments.remove(*index)),
            many => Err(DetachError::Ambiguous(
                many.iter()
                    .filter_map(|index| self.attachments.get(*index))
                    .map(|attachment| attachment.source.clone())
                    .collect(),
            )),
        }
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

    /// The text that was sent, and an empty buffer with no attachments.
    ///
    /// Called when a submit succeeds, never when it is issued: a failed send
    /// has to leave the buffer and its attachments exactly as they were.
    pub fn take_sent(&mut self) -> String {
        let text = self.text();
        self.clear();
        self.attachments.clear();
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
        // Attachments belong to the draft, so they go with it. The GUI drops them
        // for the same reason: a chip under a message shows what *that* message
        // will carry, and the next session's message carries something else.
        self.attachments.clear();
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
    /// The attachments row is the band's first row when anything is attached, so
    /// the height grows with it and `view::composer` can draw it without asking
    /// the same question twice.
    pub fn height(&self) -> u16 {
        let lines = self
            .textarea
            .lines()
            .len()
            .clamp(1, usize::from(MAX_COMPOSER_ROWS));
        let attachments = u16::from(!self.attachments.is_empty());
        (lines as u16).saturating_add(4).saturating_add(attachments)
    }
}

fn draft_key(session: Option<SessionId>) -> String {
    match session {
        Some(id) => id.to_string(),
        None => DRAFT_KEY.to_owned(),
    }
}

/// The last path segment of `source`, for `/detach`'s shorter spelling.
fn basename(source: &str) -> &str {
    source.rsplit('/').next().unwrap_or(source)
}

/// A text attachment's chip: `name` or `name (1-10)`.
///
/// Shared with the transcript, which draws the same chip from a stored message —
/// on a user band rather than above it — so a sent attachment reads exactly like
/// the one that was attached.
pub fn file_chip(name: &str, start: Option<u32>, end: Option<u32>) -> String {
    match (start, end) {
        (Some(start), Some(end)) if start == end => format!("{name} ({start})"),
        (Some(start), Some(end)) => format!("{name} ({start}-{end})"),
        _ => name.to_owned(),
    }
}

/// An image's chip: `[name, 1.2 MB]`.
///
/// Bracketed because it is not a document, and sized because the terminal cannot
/// show the picture — the bytes and the weight are the only evidence there is.
pub fn image_chip(name: &str, bytes: usize) -> String {
    format!("[{name}, {}]", format_size(bytes))
}

/// A byte count as a person reads it: `999 B`, `12.3 KB`, `1.2 MB`.
///
/// Powers of 1024, one decimal above a kilobyte, and one space before the unit.
/// An image chip is the only caller; the number is the whole reason the row is
/// worth drawing, since the terminal cannot show the picture.
pub fn format_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let size = bytes as f64;
    if size >= MB {
        format!("{:.1} MB", size / MB)
    } else if size >= KB {
        format!("{:.1} KB", size / KB)
    } else {
        format!("{bytes} B")
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

    fn text_attachment(source: &str, name: &str, text: &str) -> Attachment {
        Attachment {
            source: source.to_owned(),
            pending: Pending::File(FileAttachment {
                name: name.to_owned(),
                path: Some(source.to_owned()),
                start_line: None,
                end_line: None,
                text: text.to_owned(),
            }),
        }
    }

    fn image_attachment(source: &str, name: &str, bytes: usize) -> Attachment {
        Attachment {
            source: source.to_owned(),
            pending: Pending::Image(PendingImage {
                name: name.to_owned(),
                media_type: "image/png".to_owned(),
                bytes: vec![0; bytes],
            }),
        }
    }

    #[test]
    fn an_attachment_alone_is_sendable_and_clears_with_the_send() {
        let mut composer = Composer::new();
        assert!(composer.is_blank(), "nothing typed, nothing attached");

        composer.attach(text_attachment("src/error.rs", "error.rs", "boom"));
        assert!(
            !composer.is_blank(),
            "an attachment with no text is still a message"
        );
        assert_eq!(composer.text_total(), 4);

        // Only a taken send clears it, and it clears both halves.
        let sent = composer.take_sent();
        assert_eq!(sent, "");
        assert!(composer.is_blank());
        assert_eq!(composer.attachment_count(), 0);
    }

    #[test]
    fn switching_sessions_takes_the_attachments_with_the_draft() {
        let mut composer = Composer::new();
        let first = SessionId::new();
        composer.attach(text_attachment("src/error.rs", "error.rs", "boom"));
        composer.switch_to(None, Some(first));
        assert_eq!(composer.attachment_count(), 0, "the draft key changed");
    }

    #[test]
    fn detach_takes_a_source_a_unique_basename_or_everything() {
        let mut composer = Composer::new();
        composer.attach(text_attachment("src/error.rs", "error.rs", "boom"));
        composer.attach(text_attachment("src/app.rs", "app.rs", "fn main"));
        composer.attach(image_attachment("shot.png", "shot.png", 1_200_000));

        // The exact source always wins.
        let taken = composer.detach("src/error.rs").expect("an exact source");
        assert_eq!(taken.source, "src/error.rs");
        assert_eq!(composer.attachment_count(), 2);

        // A unique basename is a second spelling of the same thing.
        let taken = composer.detach("app.rs").expect("a unique basename");
        assert_eq!(taken.source, "src/app.rs");

        // Nothing answers to that name.
        assert_eq!(
            composer.detach("nope.rs").unwrap_err(),
            DetachError::NotFound("nope.rs".to_owned())
        );

        // And no argument removes everything, counted back for the notice.
        assert_eq!(composer.detach_all(), 1);
        assert_eq!(composer.attachment_count(), 0);
    }

    #[test]
    fn an_ambiguous_name_is_refused_rather_than_guessed() {
        let mut composer = Composer::new();
        composer.attach(text_attachment("a/mod.rs", "mod.rs", "one"));
        composer.attach(text_attachment("b/mod.rs", "mod.rs", "two"));
        let error = composer.detach("mod.rs").unwrap_err();
        assert_eq!(
            error.clone(),
            DetachError::Ambiguous(vec!["a/mod.rs".to_owned(), "b/mod.rs".to_owned()])
        );
        assert_eq!(
            error.message(),
            "a/mod.rs, b/mod.rs are attached \u{2014} name one"
        );
        assert_eq!(composer.attachment_count(), 2, "nothing was removed");
    }

    #[test]
    fn backspace_removes_the_last_attachment() {
        let mut composer = Composer::new();
        composer.attach(text_attachment("src/error.rs", "error.rs", "boom"));
        composer.attach(image_attachment("shot.png", "shot.png", 10));
        assert_eq!(composer.detach_last().expect("one").source, "shot.png");
        assert_eq!(composer.detach_last().expect("one").source, "src/error.rs");
        assert!(composer.detach_last().is_none());
    }

    #[test]
    fn a_chip_says_the_lines_and_the_weight() {
        assert_eq!(file_chip("error.rs", None, None), "error.rs");
        assert_eq!(file_chip("error.rs", Some(7), Some(7)), "error.rs (7)");
        assert_eq!(file_chip("app.rs", Some(40), Some(80)), "app.rs (40-80)");
        assert_eq!(image_chip("shot.png", 1_200_000), "[shot.png, 1.1 MB]");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(64 * 1024), "64.0 KB");
    }

    #[test]
    fn the_band_grows_a_row_while_something_is_attached() {
        let mut composer = Composer::new();
        let bare = composer.height();
        composer.attach(text_attachment("src/error.rs", "error.rs", "boom"));
        assert_eq!(composer.height(), bare + 1);
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
