//! View state that is not the persisted ids.
//!
//! `state::transcript` is the one substantial piece: the transcript is
//! pre-wrapped into rows so both the follow lock and the view are pure
//! functions of state and a width. `state::composer` owns the text field and
//! the per-session drafts, and `state::activity` the open turn's phase,
//! elapsed time, and catch-up counter.

pub mod activity;
pub mod approval;
pub mod composer;
pub mod detail;
pub mod markdown;
pub mod prompt;
pub mod tool_card;
pub mod transcript;

/// Which surface owns the keyboard. Exactly one at a time.
///
/// T2 made this a real stack: a pause no longer adds a mode — approvals are
/// answered on the selected card — but the detail view does. The rest are
/// declared here because the mode stack is the shape the later phases fill in,
/// and a keymap that grows a variant at a time is how a mode ends up
/// half-registered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// The base mode: the transcript, the composer, and the global bindings.
    #[default]
    Chat,
    /// The sessions overlay.
    Sessions,
    /// The workspace overlay.
    Workspaces,
    /// The model and effort picker.
    Models,
    /// The plan surface.
    Plan,
    /// The review surface.
    Review,
    /// The docs surface.
    Docs,
    /// The settings surface.
    Settings,
    /// One tool call, read in full: its own scroll, `Esc` back.
    Detail,
    /// A one-line prompt: rename, delete, reject reason.
    Prompt,
    /// A suspended terminal running `$EDITOR`.
    Editor,
}

/// Which region of the chat frame takes the keystrokes.
///
/// T1 is the first phase with a text field on screen, so it is the first that
/// needs this. It is not cosmetic: without it, `q` typed into the composer
/// would quit the app. The composer holds focus by default, because typing is
/// what a chat is for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Focus {
    /// The text field: every printable key is text.
    #[default]
    Composer,
    /// The transcript: the scroll bindings, `q` to quit, and no text input.
    Transcript,
}

impl Focus {
    /// The other region.
    pub fn other(self) -> Self {
        match self {
            Self::Composer => Self::Transcript,
            Self::Transcript => Self::Composer,
        }
    }

    pub fn is_composer(self) -> bool {
        self == Self::Composer
    }
}

/// How far the open turn has got.
///
/// Moved by `message_delta` and `turn_finished`, never by text: the bus drops a
/// queued delta under backpressure, so the terminal renders stored rows instead
/// and reads the phase from the stream of frame kinds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    /// No turn is running.
    #[default]
    Idle,
    /// The model is working but has not produced text yet.
    Thinking,
    /// The model is producing text.
    Responding,
}

/// What a notice says about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// A fact worth showing: a startup message, a save failure that is not now.
    Info,
    /// Something failed, or a call was refused.
    Error,
}

/// One line in the footer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

impl Notice {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            kind: NoticeKind::Info,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            kind: NoticeKind::Error,
            text: text.into(),
        }
    }

    pub fn is_error(&self) -> bool {
        self.kind == NoticeKind::Error
    }
}
