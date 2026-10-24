//! The only module that touches `AppState`, and the only `.await`.
//!
//! Each action is one service call. It runs on the caller's runtime as its own
//! task, so a slow query cannot stall the pump, and it reports back as
//! [`Msg::Done`]. Filesystem work goes through `agent::blocking::call`.

use tokio::sync::mpsc;

use crate::agent::blocking;
use crate::tui::action::{Action, ActionOutcome};
use crate::tui::app::Msg;
use crate::tui::persist;
use crate::tui::Options;
use crate::web_api::state::AppState;

/// Run `action` on its own task and send the outcome to the pump.
pub fn dispatch(state: &AppState, options: &Options, action: Action, results: &mpsc::Sender<Msg>) {
    let state = state.clone();
    let options = options.clone();
    let results = results.clone();
    tokio::spawn(async move {
        let outcome = run(&state, &options, action).await;
        let _ = results.send(Msg::Done(outcome)).await;
    });
}

/// The effect itself.
async fn run(state: &AppState, options: &Options, action: Action) -> ActionOutcome {
    match action {
        Action::LoadWorkspaces => match state.workspace_service.list_workspaces().await {
            Ok(workspaces) => ActionOutcome::Workspaces(workspaces),
            Err(error) => ActionOutcome::failed(error.to_string()),
        },
        Action::LoadSessions { workspace } => {
            match state
                .chat_session_service
                .list_chat_sessions(Some(workspace))
                .await
            {
                Ok(sessions) => ActionOutcome::Sessions {
                    workspace,
                    sessions,
                },
                Err(error) => ActionOutcome::failed(error.to_string()),
            }
        }
        Action::Reload {
            session,
            generation,
        } => match state.chat_message_service.list_messages(session).await {
            Ok(messages) => ActionOutcome::Transcript {
                session,
                generation,
                messages,
            },
            Err(error) => ActionOutcome::Failed {
                generation: Some(generation),
                message: error.to_string(),
            },
        },
        Action::Submit {
            session,
            text,
            files,
        } => {
            // T1 sends text only: the composer has no attachment surface until
            // T6, and `submit_instruction` rejects an empty instruction with an
            // empty attachment set on its own.
            match state
                .chat_message_service
                .submit_instruction(session, &text, Vec::new(), files)
                .await
            {
                Ok(outcome) => ActionOutcome::Submitted { session, outcome },
                Err(error) => ActionOutcome::SubmitFailed {
                    session,
                    message: error.to_string(),
                },
            }
        }
        Action::Stop { session } => match state.chat_message_service.stop(session).await {
            Ok(()) => ActionOutcome::Stopped { session },
            Err(error) => ActionOutcome::Failed {
                generation: None,
                message: error.to_string(),
            },
        },
        Action::LoadMessage {
            session,
            message,
            generation,
        } => match state
            .chat_message_service
            .get_message(session, message)
            .await
        {
            Ok(message) => ActionOutcome::Message {
                session,
                generation,
                message: Box::new(message),
            },
            Err(error) => ActionOutcome::Failed {
                generation: Some(generation),
                message: error.to_string(),
            },
        },
        Action::SaveView { view } => {
            let state_dir = options.state_dir.clone();
            match blocking::call(move || persist::save(&state_dir, &view)).await {
                Ok(Ok(())) => ActionOutcome::Saved,
                Ok(Err(error)) => {
                    ActionOutcome::failed(format!("could not save the view state: {error}"))
                }
                Err(error) => ActionOutcome::failed(error),
            }
        }
    }
}

/// Write the view state before the process leaves, and wait for it.
///
/// `dispatch` is fire-and-forget, which is wrong on the way out: the runtime is
/// about to stop. The failure is logged rather than shown, because the footer
/// is a frame away from being restored.
pub async fn save_view(options: &Options, view: &persist::ViewState) {
    let state_dir = options.state_dir.clone();
    let view = view.clone();
    match blocking::call(move || persist::save(&state_dir, &view)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, "could not save the view state on exit"),
        Err(error) => tracing::warn!(%error, "could not save the view state on exit"),
    }
}
