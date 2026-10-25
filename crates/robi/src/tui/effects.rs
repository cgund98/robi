//! The only module that touches `AppState`, and the only `.await`.
//!
//! Each action is one service call. It runs on the caller's runtime as its own
//! task, so a slow query cannot stall the pump, and it reports back as
//! [`Msg::Done`]. Filesystem work goes through `agent::blocking::call`.

use std::time::Duration;

use tokio::sync::mpsc;

use crate::agent::blocking;
use crate::agent::workspace::{resolve_path, user_home};
use crate::domain::chat_session::model::{CreateChatSessionCommand, UpdateChatSessionCommand};
use crate::tui::action::{Action, ActionOutcome};
use crate::tui::app::Msg;
use crate::tui::persist;
use crate::tui::state::workspaces::{candidate, split_path, CANDIDATE_LIMIT};
use crate::tui::Options;
use crate::web_api::state::AppState;

/// How long the health probe waits before it calls the store wedged.
///
/// The GUI's `TIMEOUT_MS`. The query keeps its connection until it returns, so
/// this bounds the mark rather than the query.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

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
        Action::LoadWorkspaces { generation } => {
            match state.workspace_service.list_workspaces().await {
                Ok(workspaces) => ActionOutcome::Workspaces {
                    generation,
                    workspaces,
                },
                Err(error) => ActionOutcome::failed(error.to_string()),
            }
        }
        Action::LoadSessions {
            workspace,
            generation,
        } => {
            let sessions = state
                .chat_session_service
                .list_chat_sessions(Some(workspace))
                .await;
            match sessions {
                Ok(sessions) => ActionOutcome::Sessions {
                    workspace,
                    generation,
                    sessions,
                    running: state.chat_message_service.running_session_ids().await,
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
        Action::Decide {
            session,
            call,
            decision,
        } => {
            // The word and the optional reason are the service's interface: an
            // empty reason is sent as `None`, so the service supplies its own
            // default rather than the model reading a blank line.
            match state
                .chat_message_service
                .decide_tool_call(session, call, decision.word(), decision.reason())
                .await
            {
                Ok(()) => ActionOutcome::Decided { session, call },
                Err(error) => ActionOutcome::DecideFailed {
                    session,
                    call,
                    message: error.to_string(),
                },
            }
        }
        Action::LoadModels { generation } => {
            let models =
                crate::adapters::model_source::pickable_models(&state.settings_service).await;
            match crate::adapters::model_source::ModeDefaults::load(&state.settings_service).await {
                Ok(defaults) => ActionOutcome::Models {
                    generation,
                    models,
                    defaults,
                },
                Err(error) => ActionOutcome::failed(error.to_string()),
            }
        }
        Action::LoadSkills {
            workspace,
            generation,
        } => match skills_for(state, workspace).await {
            Ok(skills) => ActionOutcome::Skills {
                workspace,
                generation,
                skills,
            },
            Err(message) => ActionOutcome::Failed {
                generation: Some(generation),
                message,
            },
        },
        Action::SetSessionMode { session, mode } => {
            let command = crate::domain::chat_session::model::UpdateChatSessionCommand::set_mode(
                session, mode,
            );
            match state
                .chat_session_service
                .update_chat_session(command)
                .await
            {
                Ok(session) => ActionOutcome::SessionUpdated(Box::new(session)),
                Err(error) => ActionOutcome::failed(error.to_string()),
            }
        }
        Action::SetSessionChoice {
            session,
            mode,
            pick,
        } => {
            let command = crate::domain::chat_session::model::UpdateChatSessionCommand::set_choice(
                session,
                mode,
                pick.key(),
                pick.value(),
            );
            match state
                .chat_session_service
                .update_chat_session(command)
                .await
            {
                Ok(session) => ActionOutcome::SessionUpdated(Box::new(session)),
                Err(error) => ActionOutcome::failed(error.to_string()),
            }
        }
        Action::Compact { session } => match state.chat_message_service.compact(session).await {
            Ok(()) => ActionOutcome::Compacted { session },
            Err(error) => ActionOutcome::failed(error.to_string()),
        },
        Action::LoadOriginal {
            session,
            call,
            original_id,
        } => match state.originals.lookup(session, &original_id).await {
            Ok(crate::agent::compress::Lookup::One(body)) => ActionOutcome::Original {
                session,
                call,
                text: crate::tui::state::tool_card::original_text(&body),
            },
            Ok(crate::agent::compress::Lookup::Missing)
            | Ok(crate::agent::compress::Lookup::Ambiguous(_)) => ActionOutcome::OriginalFailed {
                session,
                call,
                message: format!("the full output for {original_id} is gone"),
            },
            Err(error) => ActionOutcome::OriginalFailed {
                session,
                call,
                message: error,
            },
        },
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
        Action::CreateWorkspace { root } => match absolute_path(&root) {
            Ok(root) => match state.workspace_service.open_workspace(&root).await {
                Ok(opened) => ActionOutcome::WorkspaceOpened {
                    workspace: Box::new(opened.workspace),
                    created: opened.created,
                },
                Err(error) => ActionOutcome::CreateWorkspaceFailed {
                    message: error.to_string(),
                },
            },
            Err(message) => ActionOutcome::CreateWorkspaceFailed { message },
        },
        Action::DeleteWorkspace { workspace } => {
            // The same pair the HTTP handler runs: the row goes, then the
            // connections it owned are closed.
            match state.workspace_service.delete_workspace(workspace).await {
                Ok(()) => {
                    if let Some(mcp) = &state.mcp {
                        mcp.close(workspace).await;
                    }
                    ActionOutcome::WorkspaceDeleted { workspace }
                }
                Err(error) => ActionOutcome::DeleteFailed {
                    message: error.to_string(),
                },
            }
        }
        Action::StartSession {
            workspace,
            text,
            files,
            mode,
            model_config,
        } => {
            // The landing page's one gesture: create the row, then send into it.
            // Creating first is what makes the row exist at all — the terminal has
            // no draft session — and sending immediately is what keeps an
            // abandoned new chat from leaving an empty one behind.
            // The draft's mode and choice ride into the create, exactly as the
            // webview's `draftConfig` does: the terminal has no draft session, so
            // the first message is what fixes them.
            let command = CreateChatSessionCommand {
                workspace_id: workspace,
                title: None,
                mode,
                model_config,
            };
            let session = match state
                .chat_session_service
                .create_chat_session(command)
                .await
            {
                Ok(session) => session,
                Err(error) => return ActionOutcome::failed(error.to_string()),
            };
            match state
                .chat_message_service
                .submit_instruction(session.id, &text, Vec::new(), files)
                .await
            {
                Ok(outcome) => ActionOutcome::SessionStarted {
                    session: Box::new(session),
                    outcome: Some(outcome),
                    error: None,
                },
                Err(error) => ActionOutcome::SessionStarted {
                    // The row exists either way, so it is shown with the failure
                    // rather than thrown away: the text is still in the composer
                    // and one `Enter` retries it.
                    session: Box::new(session),
                    outcome: None,
                    error: Some(error.to_string()),
                },
            }
        }
        Action::RenameSession { session, title } => {
            let command = UpdateChatSessionCommand::rename(session, title);
            match state
                .chat_session_service
                .update_chat_session(command)
                .await
            {
                Ok(session) => ActionOutcome::SessionRenamed(Box::new(session)),
                Err(error) => ActionOutcome::RenameFailed {
                    message: error.to_string(),
                },
            }
        }
        Action::DeleteSession { session } => {
            // Stop before the row goes, as `DELETE /chat_sessions/{id}` does. A
            // no-op for an idle session, so the delete is not conditional.
            if let Err(error) = state.chat_message_service.stop(session).await {
                return ActionOutcome::DeleteFailed {
                    message: error.to_string(),
                };
            }
            match state
                .chat_session_service
                .delete_chat_session(session)
                .await
            {
                Ok(()) => ActionOutcome::SessionDeleted { session },
                Err(error) => ActionOutcome::DeleteFailed {
                    message: error.to_string(),
                },
            }
        }
        Action::LoadMcp {
            workspace,
            generation,
        } => match workspace_marks(state, workspace).await {
            Ok(servers) => ActionOutcome::Mcp {
                workspace,
                generation,
                servers,
            },
            Err(error) => ActionOutcome::Failed {
                generation: None,
                message: error,
            },
        },
        Action::FocusMcp { workspace } => {
            focus_mcp(state, workspace).await;
            ActionOutcome::McpFocused { workspace }
        }
        Action::Probe => {
            let probe =
                tokio::time::timeout(PROBE_TIMEOUT, state.workspace_service.list_workspaces());
            ActionOutcome::Probed {
                error: match probe.await {
                    Ok(Ok(_)) => None,
                    Ok(Err(error)) => Some(error.to_string()),
                    Err(_) => Some(format!("no answer in {} s", PROBE_TIMEOUT.as_secs())),
                },
            }
        }
        Action::CompletePath { text, generation } => ActionOutcome::Paths {
            generation,
            candidates: complete(&text).await,
            text,
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

/// Start this workspace's servers and close the others.
///
/// A failure is dropped, as the GUI's `.catch` drops it: the servers stay
/// `disconnected` until a turn focuses them through the agent.
async fn focus_mcp(state: &AppState, workspace: robi_core::ids::WorkspaceId) {
    let Some(mcp) = &state.mcp else {
        return;
    };
    let Ok(row) = state.workspace_service.get_workspace(workspace).await else {
        return;
    };
    let root = canonical_root(&row.root);
    let opener = mcp.opener();
    mcp.focus(workspace, &root, row.mcp_project_sha256.as_deref(), &opener)
        .await;
}

/// The active workspace's servers, as `GET /workspaces/{id}/mcp` reports them.
async fn workspace_marks(
    state: &AppState,
    workspace: robi_core::ids::WorkspaceId,
) -> Result<Vec<crate::agent::mcp::McpServerStatus>, String> {
    let Some(mcp) = &state.mcp else {
        return Ok(Vec::new());
    };
    let row = state
        .workspace_service
        .get_workspace(workspace)
        .await
        .map_err(|error| error.to_string())?;
    let root = canonical_root(&row.root);
    Ok(mcp
        .status(workspace, &root, row.mcp_project_sha256.as_deref())
        .await)
}

/// A stored root is canonical already; a root that has since moved is used as
/// written, the same fallback the HTTP handler takes.
fn canonical_root(root: &str) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(root);
    path.canonicalize().unwrap_or(path)
}

/// Resolve what the user typed into an absolute path for `open_workspace`.
///
/// `~` and relative paths resolve against the working directory, the way a
/// shell would, and through the same resolver the tools use. The service still
/// refuses a path that is not a directory.
fn absolute_path(argument: &str) -> Result<String, String> {
    let argument = argument.trim();
    if argument.is_empty() {
        return Err("a path is required".to_owned());
    }
    let cwd = std::env::current_dir().map_err(|error| format!("no working directory: {error}"))?;
    let home = user_home().map_err(|error| error.to_string())?;
    let resolved = resolve_path(&cwd, argument, &home)
        .map_err(|error| format!("could not resolve: {error}"))?;
    Ok(resolved.absolute.to_string_lossy().into_owned())
}

/// The directories the typed path could complete to.
///
/// Directories only: a path input is about picking a workspace root, and a file
/// is never one. No session path filter applies — there is no session yet, and
/// the folder dialog this replaces has none either.
async fn complete(text: &str) -> Vec<String> {
    let (prefix_part, needle) = split_path(text);
    let prefix_part = prefix_part.to_owned();
    let needle = needle.to_owned();
    let Ok(cwd) = std::env::current_dir() else {
        return Vec::new();
    };
    let Ok(home) = user_home() else {
        return Vec::new();
    };
    let directory = if prefix_part.is_empty() {
        cwd.clone()
    } else {
        match resolve_path(&cwd, &prefix_part, &home) {
            Ok(resolved) => resolved.absolute,
            Err(_) => return Vec::new(),
        }
    };
    let read = blocking::call(move || {
        let mut names = Vec::new();
        let Ok(entries) = std::fs::read_dir(&directory) else {
            return names;
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(&needle) {
                continue;
            }
            names.push(candidate(&prefix_part, &name));
        }
        names.sort();
        names
    })
    .await
    .unwrap_or_default();
    let mut read = read;
    read.truncate(CANDIDATE_LIMIT);
    read
}

/// This workspace's user-invocable skills, mapped to the menu's row type.
///
/// The scan walks the filesystem, so it goes through `agent::blocking::call` the
/// way every other blocking read in this crate does. The whole `Skill` is not
/// carried any further than here: the menu shows an id, a label, a description and
/// a scope, and the body is loaded by the runtime from the message text.
async fn skills_for(
    state: &AppState,
    workspace: robi_core::ids::WorkspaceId,
) -> Result<Vec<crate::tui::state::completion::SkillEntry>, String> {
    let workspace = state
        .workspace_service
        .get_workspace(workspace)
        .await
        .map_err(|error| error.to_string())?;
    let root = std::path::PathBuf::from(&workspace.root);
    let root = root.canonicalize().unwrap_or(root);
    let home = crate::adapters::settings::home_dir()
        .ok()
        .and_then(|dir| dir.parent().map(|parent| parent.to_path_buf()));
    let skills =
        blocking::call(move || crate::agent::skills::user_invocable(home.as_deref(), Some(&root)))
            .await
            .map_err(|error| error.to_string())?;
    Ok(skills
        .into_iter()
        .map(|skill| {
            let scope = crate::tui::state::completion::SkillScope::parse(skill.scope_name());
            crate::tui::state::completion::SkillEntry {
                id: skill.id,
                label: skill.label,
                description: skill.description,
                scope,
                manual: !skill.model_invocable,
            }
        })
        .collect())
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
