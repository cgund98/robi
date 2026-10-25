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
use crate::tui::state::audit;
use crate::tui::state::settings::{self, Loaded, McpPaths};
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
        // The audit row is built here because this is the one place every call
        // passes, and because the duration has to be measured around `run`.
        let audited = !action.skips_audit();
        let call = action.label();
        let started = std::time::Instant::now();
        let outcome = run(&state, &options, action).await;
        if audited {
            let entry = audit::Entry {
                seq: 0,
                call,
                ok: !outcome.is_failure(),
                duration_ms: started.elapsed().as_millis(),
                body: outcome.summary(),
                at: chrono::Utc::now(),
            };
            // Before `Done`, so a test that waits for the outcome and then reads
            // the table sees the row it just caused.
            let _ = results.send(Msg::Audited(entry)).await;
        }
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
            images,
        } => {
            // The image store is keyed by session, so the bytes the composer held
            // are written here, once the row exists. A store failure is the
            // submit's failure: nothing is sent with an image the provider could
            // not resolve.
            let images = match store_images(state, session, images).await {
                Ok(images) => images,
                Err(message) => {
                    return ActionOutcome::SubmitFailed { session, message };
                }
            };
            match state
                .chat_message_service
                .submit_instruction(session, &text, images, files)
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
            images,
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
            let images = match store_images(state, session.id, images).await {
                Ok(images) => images,
                Err(message) => {
                    // The row exists either way. `error` says why nothing was
                    // sent; the text and the attachments stay in the composer.
                    return ActionOutcome::SessionStarted {
                        session: Box::new(session),
                        outcome: None,
                        error: Some(message),
                    };
                }
            };
            match state
                .chat_message_service
                .submit_instruction(session.id, &text, images, files)
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
        Action::LoadSettings { generation } => {
            let fields: Vec<&'static settings::Field> = settings::all_fields().collect();
            let keys: Vec<String> = fields.iter().map(|field| field.key.to_owned()).collect();
            match state.settings_service.get_many(&keys).await {
                Ok(read) => {
                    let mut values = std::collections::BTreeMap::new();
                    for (field, setting) in fields.iter().zip(read) {
                        let loaded = if field.kind == settings::Kind::Secret {
                            // The reduction that keeps a credential out of `App`:
                            // the read's plaintext stops here and never travels as
                            // an outcome.
                            Loaded::credential(setting.value.is_some())
                        } else {
                            Loaded::text(setting.value)
                        };
                        values.insert(field.key, loaded);
                    }
                    ActionOutcome::Settings { generation, values }
                }
                Err(error) => ActionOutcome::failed(error.to_string()),
            }
        }
        Action::SaveSetting { key, value } => {
            // The secret flag is the store's, not the surface's: a wrong one is a
            // plaintext credential in `config.toml`, so it is re-read here rather
            // than carried on the action.
            let secret =
                crate::domain::settings::keys::known_setting(key).is_some_and(|known| known.secret);
            let result = match value {
                Some(value) => state.settings_service.set(key, value, secret).await,
                // `remove` refuses a secret key, which is why an empty buffer on
                // one is `None` and never reaches here.
                None => state.settings_service.remove(key).await,
            };
            match result {
                Ok(()) => ActionOutcome::SettingSaved {
                    key,
                    label: settings::label_of(key).unwrap_or(key),
                },
                Err(error) => ActionOutcome::SettingFailed {
                    key,
                    message: error.to_string(),
                },
            }
        }
        Action::Attach {
            workspace,
            source,
            session,
        } => match read_attachment(state, workspace, &source).await {
            Ok((source, pending)) => ActionOutcome::Attached {
                session,
                source,
                pending,
            },
            Err(message) => ActionOutcome::AttachFailed { message },
        },
        Action::OpenFile {
            workspace,
            path,
            line,
        } => match state
            .settings_service
            .get(crate::domain::settings::keys::FILE_OPEN_COMMAND)
            .await
        {
            Err(error) => ActionOutcome::OpenFailed {
                message: error.to_string(),
            },
            Ok(setting) => match setting.value.filter(|value| !value.trim().is_empty()) {
                // No command is a refusal, never a fallback: `$EDITOR` and a
                // system opener are both what this design rules out.
                None => ActionOutcome::OpenFailed {
                    message: "no file open command is set \u{2014} set one in Ctrl-,".to_owned(),
                },
                Some(template) => {
                    let row = match state.workspace_service.get_workspace(workspace).await {
                        Ok(row) => row,
                        Err(error) => {
                            return ActionOutcome::OpenFailed {
                                message: error.to_string(),
                            };
                        }
                    };
                    let root = canonical_root(&row.root);
                    let opened = path.clone();
                    // The opener waits for an early exit, so it cannot run on the
                    // runtime.
                    match blocking::call(move || {
                        crate::agent::workspace::open(&root, &opened, line, &template)
                    })
                    .await
                    {
                        Ok(Ok(())) => ActionOutcome::Opened { path },
                        Ok(Err(error)) => ActionOutcome::OpenFailed {
                            message: error.to_string(),
                        },
                        Err(message) => ActionOutcome::OpenFailed { message },
                    }
                }
            },
        },
        Action::LoadMcpConfig { workspace } => {
            let row = match state.workspace_service.get_workspace(workspace).await {
                Ok(row) => row,
                Err(error) => return ActionOutcome::failed(error.to_string()),
            };
            let Some(mcp) = &state.mcp else {
                return ActionOutcome::McpConfig(McpPaths::default());
            };
            let mcp = mcp.clone();
            let root = canonical_root(&row.root);
            let hash = row.mcp_project_sha256.clone();
            match blocking::call(move || mcp.config_files(&root, hash.as_deref())).await {
                Ok(files) => ActionOutcome::McpConfig(McpPaths {
                    user_path: files.user_path,
                    project_path: files.project_path,
                    project_enabled: files.project_enabled,
                    // Where each server's own log lands, as `logs.rs` writes it.
                    // Read here rather than in the view: `App` does no I/O, and a
                    // per-frame `HOME` lookup is I/O however cheap.
                    log_root: crate::adapters::settings::home_dir()
                        .map(|home| home.join("logs").join("mcp"))
                        .map(|dir| dir.display().to_string())
                        .unwrap_or_default(),
                }),
                Err(error) => ActionOutcome::failed(error),
            }
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

/// Write the composer's waiting images under `session`, and return the
/// references the transcript carries.
///
/// The store is keyed by session, which is why the bytes waited: on the landing
/// page there is no session to write under until the create comes back.
async fn store_images(
    state: &AppState,
    session: robi_core::ids::SessionId,
    images: Vec<crate::tui::state::composer::PendingImage>,
) -> Result<Vec<robi_core::message::ImageAttachment>, String> {
    let mut stored = Vec::with_capacity(images.len());
    for image in images {
        let id = uuid::Uuid::now_v7().to_string();
        state
            .image_source
            .store(&session.to_string(), &id, &image.media_type, image.bytes)
            .await
            .map_err(|error| format!("{} could not be stored: {error}", image.name))?;
        stored.push(robi_core::message::ImageAttachment {
            id,
            media_type: image.media_type,
        });
    }
    Ok(stored)
}

/// Resolve the workspace, then read the file.
///
/// The split exists so the rules below can be tested against a temp directory
/// without an `AppState`: everything after this point is
/// [`attachment_from_path`], which takes a root and nothing else.
async fn read_attachment(
    state: &AppState,
    workspace: robi_core::ids::WorkspaceId,
    source: &str,
) -> Result<(String, crate::tui::state::composer::Pending), String> {
    let row = state
        .workspace_service
        .get_workspace(workspace)
        .await
        .map_err(|error| error.to_string())?;
    let root = canonical_root(&row.root);
    let home = user_home().map_err(|error| error.to_string())?;
    let source = source.to_owned();
    let read = blocking::call(move || attachment_from_path(&root, &home, &source))
        .await
        .map_err(|error| error.to_string())?;
    read
}

/// Resolve, read, slice, and classify one file the reader named.
///
/// `source` is what they typed: a path, optionally with `:start-end`. The path
/// resolves against the workspace root, and `~` and an absolute path are accepted
/// — an explicit attach is the consent, so a file outside the workspace attaches
/// with its name alone. `/open` is the one that refuses them: it launches a
/// program rather than reading bytes.
///
/// The image check runs on the signature, so a `.png` that is text attaches as
/// text and a `.txt` that is a png attaches as an image.
fn attachment_from_path(
    root: &std::path::Path,
    home: &std::path::Path,
    source: &str,
) -> Result<(String, crate::tui::state::composer::Pending), String> {
    use crate::domain::chat_message::attachment::{
        self, LineRange, MAX_IMAGE_BYTES, MAX_TEXT_FILE_BYTES,
    };
    use crate::tui::state::composer::{Pending, PendingImage};

    let (path, range) = attachment::split_range(source);
    if path.trim().is_empty() {
        return Err(format!("{source} names a line range and no file"));
    }
    let resolved = resolve_path(root, &path, home).map_err(|error| error.to_string())?;
    let metadata = std::fs::metadata(&resolved.absolute)
        .map_err(|error| format!("{path} could not be read: {}", io_reason(&error)))?;
    if !metadata.is_file() {
        return Err(format!("{path} is not a file"));
    }
    // One read bound covers both kinds: an image is capped at 10 MiB and a text
    // file at 64 KiB, and neither is read past the larger of the two.
    if metadata.len() > MAX_IMAGE_BYTES as u64 {
        return Err(format!("{path} is over the {MAX_IMAGE_BYTES} byte limit"));
    }
    let bytes = std::fs::read(&resolved.absolute)
        .map_err(|error| format!("{path} could not be read: {}", io_reason(&error)))?;

    let name = resolved
        .absolute
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());

    if let Some(media_type) = attachment::detect_media_type(&bytes) {
        if range.is_some() {
            return Err(format!(
                "{name} is an image \u{2014} a line range needs a text file"
            ));
        }
        return Ok((
            source.to_owned(),
            Pending::Image(PendingImage {
                name,
                media_type: media_type.to_owned(),
                bytes,
            }),
        ));
    }

    // Text: the whole file, then the range. The cap applies to what is attached,
    // so a range is how a file bigger than the cap gets in at all.
    let text = attachment::decode_text(&name, bytes).map_err(|error| error.to_string())?;
    let (text, range): (String, Option<LineRange>) = match range {
        Some(range) => {
            let (slice, taken) = attachment::slice_lines(&text, range);
            (slice, Some(taken))
        }
        None => {
            if text.len() > MAX_TEXT_FILE_BYTES {
                return Err(format!(
                    "{name} is {} bytes, over the {MAX_TEXT_FILE_BYTES} limit \u{2014} \
                     attach a range with {name}:40-80",
                    text.len()
                ));
            }
            (text, None)
        }
    };
    let file = attachment::text_attachment(&name, root, &resolved.absolute, text, range)
        .map_err(|error| error.to_string())?;
    Ok((source.to_owned(), Pending::File(file)))
}

/// The reason out of an I/O error, without the `(os error N)` tail.
fn io_reason(error: &std::io::Error) -> String {
    let text = error.to_string();
    text.split(" (os error").next().unwrap_or(&text).to_owned()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp workspace with one nested text file and one png.
    fn workspace() -> (std::path::PathBuf, std::path::PathBuf) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("robi-attach-{unique}"));
        std::fs::create_dir_all(root.join("src")).expect("mkdir");
        std::fs::write(
            root.join("src").join("app.rs"),
            "one\ntwo\nthree\nfour\nfive\n",
        )
        .expect("write");
        let png = [
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        std::fs::write(root.join("shot.png"), png).expect("write");
        // A home **outside** the root, so a `~/` attach is an outside one.
        let home = std::env::temp_dir().join(format!("robi-home-{unique}"));
        std::fs::create_dir_all(&home).expect("mkdir");
        (root.canonicalize().expect("root"), home)
    }

    #[test]
    fn a_relative_text_path_attaches_with_its_workspace_path() {
        let (root, home) = workspace();
        let (source, pending) =
            attachment_from_path(&root, &home, "src/app.rs").expect("a text file");
        assert_eq!(source, "src/app.rs");
        match pending {
            crate::tui::state::composer::Pending::File(file) => {
                assert_eq!(file.name, "app.rs");
                assert_eq!(file.path.as_deref(), Some("src/app.rs"));
                assert_eq!(file.start_line, None, "no range means the whole file");
                assert!(file.text.starts_with("one"));
            }
            other => panic!("expected a text attachment, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_range_slices_the_file_and_reports_the_lines_it_took() {
        let (root, home) = workspace();
        let (_, pending) = attachment_from_path(&root, &home, "src/app.rs:2-3").expect("a slice");
        match pending {
            crate::tui::state::composer::Pending::File(file) => {
                assert_eq!(file.text, "two\nthree");
                assert_eq!((file.start_line, file.end_line), (Some(2), Some(3)));
            }
            other => panic!("expected a text attachment, got {other:?}"),
        }

        // A tail past the end clamps rather than failing, and says what it took.
        let (_, pending) =
            attachment_from_path(&root, &home, "src/app.rs:4-99").expect("a clamped slice");
        match pending {
            crate::tui::state::composer::Pending::File(file) => {
                assert_eq!((file.start_line, file.end_line), (Some(4), Some(5)));
                assert_eq!(file.text, "four\nfive");
            }
            other => panic!("expected a text attachment, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_image_is_decided_by_its_bytes_not_its_name() {
        let (root, home) = workspace();
        let (source, pending) = attachment_from_path(&root, &home, "shot.png").expect("an image");
        assert_eq!(source, "shot.png");
        match pending {
            crate::tui::state::composer::Pending::Image(image) => {
                assert_eq!(image.media_type, "image/png");
                assert_eq!(image.name, "shot.png");
                assert_eq!(image.bytes.len(), 16);
            }
            other => panic!("expected an image, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_range_on_an_image_is_refused() {
        let (root, home) = workspace();
        let error = attachment_from_path(&root, &home, "shot.png:1-2").unwrap_err();
        assert!(error.contains("a line range needs a text file"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_directory_a_missing_file_and_a_bare_range_are_each_named() {
        let (root, home) = workspace();
        let error = attachment_from_path(&root, &home, "src").unwrap_err();
        assert!(error.contains("is not a file"), "{error}");

        let error = attachment_from_path(&root, &home, "src/nope.rs").unwrap_err();
        assert!(error.contains("could not be read"), "{error}");

        let error = attachment_from_path(&root, &home, ":10-20").unwrap_err();
        assert!(error.contains("names a line range and no file"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_home_relative_path_attaches_as_outside_the_workspace() {
        let (root, home) = workspace();
        std::fs::write(home.join("notes.md"), "hi").expect("write");
        let (source, pending) =
            attachment_from_path(&root, &home, "~/notes.md").expect("a file in home");
        assert_eq!(source, "~/notes.md", "the chip quotes what was typed");
        match pending {
            crate::tui::state::composer::Pending::File(file) => {
                assert_eq!(file.name, "notes.md");
                assert_eq!(file.path, None, "outside the workspace, so no path");
            }
            other => panic!("expected a text attachment, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_text_file_over_the_cap_is_refused_with_the_range_suggestion() {
        let (root, home) = workspace();
        let big = "x".repeat(crate::domain::chat_message::attachment::MAX_TEXT_FILE_BYTES + 1);
        std::fs::write(root.join("big.txt"), big).expect("write");
        let error = attachment_from_path(&root, &home, "big.txt").unwrap_err();
        assert!(error.contains("over the"), "{error}");
        assert!(error.contains("big.txt:40-80"), "{error}");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&home);
    }
}
