//! View state that is not the persisted ids.
//!
//! `state::transcript` is the one substantial piece: the transcript is
//! pre-wrapped into rows so both the follow lock and the view are pure
//! functions of state and a width.

pub mod transcript;

/// Which surface owns the keyboard. Exactly one at a time.
///
/// T0 only ever enters [`Mode::Chat`]. The rest are declared here because the
/// mode stack is the shape the later phases fill in, and a keymap that grows a
/// variant at a time is how a mode ends up half-registered.
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
    /// The approval modal. It owns the keyboard while a turn is paused.
    Approval,
    /// A one-line prompt: rename, delete, reject reason.
    Prompt,
    /// A suspended terminal running `$EDITOR`.
    Editor,
}

/// How far the open turn has got.
///
/// T0 never leaves [`Phase::Idle`]: there is no composer and no submit. T1
/// moves it from `message_delta` and `turn_finished`, never from text.
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
