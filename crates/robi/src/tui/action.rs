//! The effects `update` asks for, and what came back.
//!
//! `update` never performs I/O. Each `Action` names one service call;
//! `effects::run` is the only place a `.await` exists. An outcome comes back as
//! `Msg::Done`, so the loop stays one state machine.
//!
//! # Generations
//!
//! Two refetches of one session can be in flight together, so the ones that can
//! land out of order carry the generation they were issued at:
//!
//! - [`Action::Reload`] reads the whole transcript. It bumps the generation and
//!   applies only while that number is still current, so an older full read
//!   never overwrites a newer one.
//! - [`Action::LoadMessage`] reads one row. It carries the generation that was
//!   current when it was issued and applies only while it still matches, so a
//!   full reload supersedes it. Two per-row reads of the same id are harmless
//!   either way: applying a message is an upsert by id.
//!
//! A `Done` with a stale generation is dropped, never merged.

use robi_core::ids::{MessageId, SessionId, WorkspaceId};
use robi_core::message::{FileAttachment, Message};

use crate::domain::chat_message::runtime::SubmitOutcome;
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
    /// Hand one instruction to the session's actor.
    ///
    /// T1 sends text only; T6 adds `files`, and images arrive with it. The
    /// vectors are here now so the ask does not change shape when they do.
    Submit {
        session: SessionId,
        text: String,
        files: Vec<FileAttachment>,
    },
    /// Cancel the running turn.
    Stop { session: SessionId },
    /// Refetch one stored message, after a frame said that row changed.
    LoadMessage {
        session: SessionId,
        message: MessageId,
        generation: u64,
    },
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
    /// The actor took the instruction. `AwaitingApproval` is an outcome, not a
    /// failure: the turn is paused on a decision, so nothing was queued.
    Submitted {
        session: SessionId,
        outcome: SubmitOutcome,
    },
    /// The turn was cancelled, and the actor has exited.
    Stopped {
        session: SessionId,
    },
    /// One row, for an upsert into the transcript.
    ///
    /// Boxed: a `Message` carries a whole body and its tool calls, and every
    /// other variant here is a handful of ids.
    Message {
        session: SessionId,
        generation: u64,
        message: Box<Message>,
    },
    /// The submit itself failed, so the composer keeps its text.
    ///
    /// Separate from [`Self::Failed`] because the composer's lock clears here
    /// and nowhere else: a reload that fails must not unlock a send that is
    /// still in flight.
    SubmitFailed {
        session: SessionId,
        message: String,
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
