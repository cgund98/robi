//! The effects `update` asks for, and what came back.
//!
//! `update` never performs I/O. Each `Action` names one service call;
//! `effects::run` is the only place a `.await` exists. An outcome comes back as
//! `Msg::Done`, so the loop stays one state machine.

use robi_core::ids::{SessionId, WorkspaceId};
use robi_core::message::Message;

use crate::domain::chat_session::model::ChatSession;
use crate::domain::workspace::model::Workspace;
use crate::tui::persist::ViewState;

/// One effect `update` asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// List workspaces, to pick the active one.
    LoadWorkspaces,
    /// List this workspace's sessions, newest first.
    LoadSessions { workspace: WorkspaceId },
    /// Refetch the transcript of the open session.
    ///
    /// `generation` is the value the transcript held when the action was
    /// issued. Two reloads of one session can be in flight together, and the
    /// older one landing last must not win.
    Reload { session: SessionId, generation: u64 },
    /// Persist the view state: on a change, and before the process exits.
    SaveView { view: ViewState },
}

/// What an effect produced.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionOutcome {
    Workspaces(Vec<Workspace>),
    Sessions {
        workspace: WorkspaceId,
        sessions: Vec<ChatSession>,
    },
    Transcript {
        session: SessionId,
        generation: u64,
        messages: Vec<Message>,
    },
    /// The view state was written.
    Saved,
    /// An effect failed. `generation` is set by the effect that can go stale.
    Failed {
        generation: Option<u64>,
        message: String,
    },
}

impl ActionOutcome {
    /// A failure that no reload can make stale.
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed {
            generation: None,
            message: message.into(),
        }
    }
}
