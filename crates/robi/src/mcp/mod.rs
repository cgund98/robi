//! MCP host. Robi connects to servers. It does not accept MCP connections.

mod config;
mod connect;
mod content;
mod env;
mod host;
mod names;
mod session;
mod tool;

pub use connect::RmcpOpener;

pub use config::{file_hash, preview, project_path, read_file, user_path, ServerPreview};
pub use host::{register_list, NameIndex, Registered};
pub use session::{Listed, ListedTool, McpSession};
pub use tool::{AllowList, McpTool};

use robi_core::ids::SessionId;

use crate::domain::chat_session::service::ChatSessionService;

/// Reads `mcp_allows` for one chat session.
pub struct SessionAllows {
    pub sessions: Arc<ChatSessionService>,
    pub session: SessionId,
}

#[async_trait::async_trait]
impl AllowList for SessionAllows {
    async fn allows(&self, server: &str, tool: &str) -> bool {
        self.sessions
            .get_chat_session(self.session)
            .await
            .ok()
            .is_some_and(|chat| {
                chat.mcp_allows
                    .iter()
                    .any(|allow| allow.server == server && allow.tool == tool)
            })
    }
}

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use robi_core::ids::WorkspaceId;
use robi_core::tool::ToolRegistry;
use tokio::sync::Mutex;

use crate::domain::events::{EventBus, EventEnvelope};
use crate::domain::settings::store::SettingsStore;

use config::{merge, ServerConfig};

/// One supervisor for the process. Sessions of a workspace share its servers.
pub struct McpHub {
    inner: Mutex<HubState>,
    settings: Arc<dyn SettingsStore>,
    home: PathBuf,
    bus: Arc<EventBus>,
}

struct HubState {
    workspaces: std::collections::HashMap<WorkspaceId, WorkspaceServers>,
}

struct WorkspaceServers {
    names: NameIndex,
    root: PathBuf,
    live: std::collections::HashMap<String, LiveServer>,
}

#[derive(Clone)]
struct LiveServer {
    status: &'static str,
    title: Option<String>,
    icon: Option<String>,
    tool_count: u32,
}

/// The user and project MCP files, unread of their secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpConfigFiles {
    pub user_path: String,
    pub user_text: Option<String>,
    pub project_path: String,
    pub project_text: Option<String>,
    pub project_enabled: bool,
}

/// One configured server and the connection state this process knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerStatus {
    pub id: String,
    pub status: String,
    pub title: Option<String>,
    pub icon: Option<String>,
    pub tool_count: u32,
}

impl McpHub {
    pub fn new(settings: Arc<dyn SettingsStore>, home: PathBuf, bus: Arc<EventBus>) -> Self {
        Self {
            inner: Mutex::new(HubState {
                workspaces: std::collections::HashMap::new(),
            }),
            settings,
            home,
            bus,
        }
    }

    /// Read both JSON files and register tools for the servers that connect.
    ///
    /// Agent mode calls this before the first model request. A server that is
    /// still down is omitted. The project file is used only when `stored_hash`
    /// equals the file's SHA-256.
    pub async fn attach(
        &self,
        workspace: WorkspaceId,
        root: &Path,
        stored_hash: Option<&str>,
        registry: &ToolRegistry,
        allows: Arc<dyn AllowList>,
        open: &dyn session::SessionOpener,
    ) {
        let user = read_file(&user_path(&self.home));
        let project_file = project_path(root);
        let project_bytes = std::fs::read(&project_file).unwrap_or_default();
        let trusted = stored_hash.is_some_and(|hash| hash == file_hash(&project_bytes));
        let project = String::from_utf8_lossy(&project_bytes).into_owned();
        let secrets = self.secrets().await;
        let (servers, errors) = merge(&user, &project, trusted, &secrets);
        tracing::info!(
            %workspace,
            servers = servers.len(),
            project_trusted = trusted,
            "mcp attach"
        );
        if !trusted && !project_bytes.is_empty() {
            tracing::info!(
                %workspace,
                "project mcp.json is not enabled for this workspace"
            );
        }
        for error in errors {
            tracing::error!(server = %error.id, reason = %error.message, "skipped mcp server");
        }
        let mut state = self.inner.lock().await;
        let slot = state
            .workspaces
            .entry(workspace)
            .or_insert_with(|| WorkspaceServers {
                names: NameIndex::default(),
                root: root.to_path_buf(),
                live: std::collections::HashMap::new(),
            });
        slot.root = root.to_path_buf();
        for server in &servers {
            slot.live.insert(
                server.id.clone(),
                LiveServer {
                    status: "starting",
                    title: None,
                    icon: None,
                    tool_count: 0,
                },
            );
        }
        drop(state);
        for server in servers {
            tracing::info!(
                server = %server.id,
                target = %server_target(&server),
                "mcp server starting"
            );
            let opened = connect_server(open, &server, root).await;
            let mut state = self.inner.lock().await;
            let Some(slot) = state.workspaces.get_mut(&workspace) else {
                continue;
            };
            match opened {
                Ok((session, listed)) => {
                    let live = register_connected(
                        registry,
                        allows.clone(),
                        slot,
                        &server.id,
                        session,
                        listed,
                    );
                    tracing::info!(
                        server = %server.id,
                        tools = live.tool_count,
                        "mcp server connected"
                    );
                    if let Some(icon) = live.icon.clone() {
                        remember_icon(&self.home, &server.id, &icon);
                    }
                    slot.live.insert(server.id.clone(), live);
                }
                Err(err) => {
                    tracing::error!(server = %server.id, reason = %err, "mcp server failed");
                    self.bus.publish(EventEnvelope::user_error(format!(
                        "MCP server {} failed to start: {err}",
                        server.id
                    )));
                    let previous = slot.live.get(&server.id).cloned();
                    slot.names.forget_server(&server.id);
                    slot.live.insert(
                        server.id,
                        LiveServer {
                            status: "failed",
                            title: previous.and_then(|row| row.title),
                            icon: None,
                            tool_count: 0,
                        },
                    );
                }
            }
        }
    }

    /// Configured servers for this workspace, with the status of any connection
    /// this process has opened. A server that has not been started is
    /// `disconnected`. Secrets and header values are not included.
    pub async fn status(
        &self,
        workspace: WorkspaceId,
        root: &Path,
        stored_hash: Option<&str>,
    ) -> Vec<McpServerStatus> {
        let user = read_file(&user_path(&self.home));
        let project_bytes = std::fs::read(project_path(root)).unwrap_or_default();
        let trusted = stored_hash.is_some_and(|hash| hash == file_hash(&project_bytes));
        let project = String::from_utf8_lossy(&project_bytes).into_owned();
        let (servers, _) = merge(&user, &project, trusted, &self.secrets().await);
        let cached = load_icons(&self.home);
        let state = self.inner.lock().await;
        let live = state.workspaces.get(&workspace);
        servers
            .into_iter()
            .map(|server| {
                let row = live.and_then(|slot| slot.live.get(&server.id));
                let icon = row
                    .and_then(|row| row.icon.clone())
                    .or_else(|| cached.get(&server.id).cloned());
                McpServerStatus {
                    id: server.id,
                    status: row
                        .map(|row| row.status)
                        .unwrap_or("disconnected")
                        .to_owned(),
                    title: row.and_then(|row| row.title.clone()),
                    icon,
                    tool_count: row.map(|row| row.tool_count).unwrap_or(0),
                }
            })
            .collect()
    }

    /// The two JSON files as stored. Secret objects stay unresolved.
    pub fn config_files(&self, root: &Path, stored_hash: Option<&str>) -> McpConfigFiles {
        let user = user_path(&self.home);
        let project = project_path(root);
        let project_bytes = std::fs::read(&project).ok();
        let project_enabled = project_bytes
            .as_ref()
            .is_some_and(|bytes| stored_hash.is_some_and(|hash| hash == file_hash(bytes)));
        McpConfigFiles {
            user_path: user.display().to_string(),
            user_text: std::fs::read_to_string(&user).ok(),
            project_path: project.display().to_string(),
            project_text: project_bytes.and_then(|bytes| String::from_utf8(bytes).ok()),
            project_enabled,
        }
    }

    pub async fn pair(&self, workspace: WorkspaceId, registered: &str) -> Option<(String, String)> {
        let state = self.inner.lock().await;
        state
            .workspaces
            .get(&workspace)
            .and_then(|slot| slot.names.pair(registered))
    }

    async fn secrets(&self) -> BTreeMap<String, String> {
        let path = self.home.join("secrets.toml");
        let Ok(body) = std::fs::read_to_string(&path) else {
            return BTreeMap::new();
        };
        let Ok(table) = toml::from_str::<toml::Table>(&body) else {
            tracing::error!("secrets.toml did not parse; mcp secret references will fail");
            return BTreeMap::new();
        };
        let mut found = BTreeMap::new();
        for (key, value) in table {
            let Some(text) = value.as_str() else {
                continue;
            };
            if self
                .settings
                .get(&key)
                .await
                .ok()
                .flatten()
                .is_some_and(|setting| setting.secret)
            {
                found.insert(key, text.to_owned());
            }
        }
        found
    }
}

async fn connect_server(
    open: &dyn session::SessionOpener,
    server: &ServerConfig,
    root: &Path,
) -> Result<(Arc<dyn McpSession>, Listed), String> {
    let spec = session::OpenSpec {
        server_id: server.id.clone(),
        workspace_root: root.to_path_buf(),
        transport: server.transport.clone(),
    };
    let connected = tokio::time::timeout(std::time::Duration::from_secs(15), open.open(spec))
        .await
        .map_err(|_| "initialize exceeded 15 seconds".to_owned())?
        .map_err(|err| err)?;
    let listed = connected.list_tools().await?;
    if let Some(instructions) = &listed.instructions {
        tracing::info!(
            server = %server.id,
            len = instructions.len(),
            "mcp initialize instructions left out of the prompt"
        );
    }
    Ok((Arc::from(connected), listed))
}

const ICON_CACHE: &str = "mcp-icons.json";
const MAX_CACHED_ICON: usize = 256 * 1024;

fn load_icons(home: &Path) -> BTreeMap<String, String> {
    let Ok(body) = std::fs::read_to_string(home.join(ICON_CACHE)) else {
        return BTreeMap::new();
    };
    serde_json::from_str(&body).unwrap_or_default()
}

fn remember_icon(home: &Path, id: &str, icon: &str) {
    if icon.len() > MAX_CACHED_ICON {
        tracing::warn!(server = %id, "mcp icon is too large to cache");
        return;
    }
    let mut icons = load_icons(home);
    if icons.get(id).map(String::as_str) == Some(icon) {
        return;
    }
    icons.insert(id.to_owned(), icon.to_owned());
    let Ok(body) = serde_json::to_string(&icons) else {
        return;
    };
    if let Err(err) = std::fs::write(home.join(ICON_CACHE), body) {
        tracing::warn!(%err, "could not cache an mcp icon");
    }
}

fn server_target(server: &ServerConfig) -> String {
    match &server.transport {
        config::Transport::Stdio { command, args, .. } => {
            if args.is_empty() {
                command.clone()
            } else {
                format!("{command} {}", args.join(" "))
            }
        }
        config::Transport::Http { url, .. } => url.clone(),
    }
}

fn register_connected(
    registry: &ToolRegistry,
    allows: Arc<dyn AllowList>,
    slot: &mut WorkspaceServers,
    server: &str,
    session: Arc<dyn McpSession>,
    listed: Listed,
) -> LiveServer {
    let previous = slot.names.forget_server(server);
    let title = listed.title.clone();
    let icon = listed.icon.clone();
    let rows = register_list(
        registry,
        server,
        &listed,
        session,
        Arc::new(Mutex::new(())),
        allows,
        &previous,
    );
    slot.names.remember(&rows);
    LiveServer {
        status: "connected",
        title,
        icon,
        tool_count: rows.len() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::memory::MemorySettingsStore;

    #[tokio::test]
    async fn a_configured_server_is_disconnected_until_an_actor_starts_it() {
        let dir = std::env::temp_dir().join(format!("robi-mcp-status-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mcp.json"),
            r#"{"mcpServers":{"linear":{"url":"https://mcp.linear.app/mcp"}}}"#,
        )
        .unwrap();
        let hub = McpHub::new(
            Arc::new(MemorySettingsStore::new()),
            dir.clone(),
            Arc::new(EventBus::new()),
        );
        let rows = hub
            .status(WorkspaceId::new(), Path::new("/tmp"), None)
            .await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "linear");
        assert_eq!(rows[0].status, "disconnected");
        assert!(rows[0].icon.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_cached_icon_is_returned_before_the_server_connects() {
        let dir = std::env::temp_dir().join(format!("robi-mcp-icon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mcp.json"),
            r#"{"mcpServers":{"linear":{"url":"https://mcp.linear.app/mcp"}}}"#,
        )
        .unwrap();
        remember_icon(&dir, "linear", "https://example.com/linear.png");
        let hub = McpHub::new(
            Arc::new(MemorySettingsStore::new()),
            dir.clone(),
            Arc::new(EventBus::new()),
        );
        let rows = hub
            .status(WorkspaceId::new(), Path::new("/tmp"), None)
            .await;
        assert_eq!(
            rows[0].icon.as_deref(),
            Some("https://example.com/linear.png")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
