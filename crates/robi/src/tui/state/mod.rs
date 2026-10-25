//! View state that is not the persisted ids.
//!
//! `state::transcript` is the one substantial piece: the transcript is
//! pre-wrapped into rows so both the follow lock and the view are pure
//! functions of state and a width. `state::composer` owns the text field and
//! the per-session drafts, and `state::activity` the open turn's phase,
//! elapsed time, and catch-up counter. T3 adds the two overlays
//! (`sessions`, `workspaces`), their confirmation (`confirm`), and the status
//! bar's facts (`status`).

pub mod activity;
pub mod approval;
pub mod completion;
pub mod composer;
pub mod confirm;
pub mod context;
pub mod detail;
pub mod facts;
pub mod help;
pub mod landing;
pub mod manage;
pub mod markdown;
pub mod models;
pub mod prompt;
pub mod sessions;
pub mod status;
pub mod tool_card;
pub mod transcript;
pub mod workspaces;

/// Which surface owns the keyboard. Exactly one at a time.
///
/// T2 made this a real stack: a pause no longer adds a mode — approvals are
/// answered on the selected card — but the detail view does. T3 adds the two
/// overlays and the confirmation that sits above one of them. T4 adds the picker,
/// the help page, and the completion menu: each owns the keyboard for what it is,
/// because `keys::command` is a pure function of `(Mode, Focus, KeyEvent)` and
/// cannot see whether a menu is up. `Settings` is declared because T5 fills it in,
/// and a keymap that grows a variant at a time is how a mode ends up
/// half-registered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// The base mode: the transcript, the composer, and the global bindings.
    ///
    /// It is also the landing page's mode: a workspace with no session open draws
    /// the greeting and the composer in place of a transcript, and there is
    /// nothing to own the keyboard that `chat` does not already.
    #[default]
    Chat,
    /// The sessions overlay.
    Sessions,
    /// The workspace switcher overlay: the eight most recent, and the create
    /// screen.
    Workspaces,
    /// Every workspace, filtered, with remove. A dedicated view, not a screen of
    /// the switcher: managing is a place the reader stays.
    Manage,
    /// The path the process was started on has no workspace, and neither answer
    /// has been given yet.
    WorkspaceMissing,
    /// The model and effort picker.
    Models,
    /// The settings surface.
    Settings,
    /// One tool call, read in full: its own scroll, `Esc` back.
    Detail,
    /// The completion menu over the composer, after `/` or `@`.
    Complete,
    /// The help page: every key and command, on its own scroll.
    Help,
    /// A one-line prompt: rename, reject reason.
    Prompt,
    /// The confirmation above an overlay: `y` accepts, `n` or `Esc` cancels.
    Confirm,
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
///
/// A notice is an **answer to a keystroke** — `nothing to send`, `a title is
/// required`, `stopped` — and it is drawn in the footer, next to the hint. A
/// failure the app observed rather than one the user typed into — a failed read,
/// a refused decision, a turn that ended `Failed` — is not a notice: it goes on
/// the strip above the composer and stays until something succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// A fact worth showing about what the reader just did.
    Info,
    /// The keystroke was refused, and this is why.
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
