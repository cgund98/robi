//! MCP host. Robi connects to servers. It does not accept MCP connections.

mod config;
mod connect;
mod content;
mod env;
mod filter;
mod host;
pub(crate) mod logs;
mod names;
mod session;
mod tool;

pub(crate) use config::load;
pub use config::{
    file_hash, preview, project_path, read_file, user_path, ProjectTrust, ServerConfig,
    ServerPreview,
};
pub use connect::RmcpOpener;
pub use host::{register_list, NameIndex, Registered};
pub(crate) use names::registered_name;
pub use session::{Listed, ListedTool, McpSession, OpenSpec, SessionOpener};
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

/// The SHA-256 of `<root>/.robi/mcp.json`, or `None` when the file cannot be read.
///
/// This is what the workspace row stores to trust a project file. The bytes are
/// hashed as they are on disk: one edit breaks the match, which is the point.
pub async fn project_hash(root: &Path) -> Option<String> {
    let path = project_path(root);
    let bytes = crate::agent::blocking::call(move || std::fs::read(&path))
        .await
        .ok()?
        .ok()?;
    Some(file_hash(&bytes))
}

/// Trust or distrust this workspace's project MCP file.
///
/// Trust stores the hash of the file's current bytes. Distrust clears the
/// column, which stops the project servers on the next focus or turn. Both
/// surfaces go through here so the hash rule has one implementation.
pub async fn set_project_trust(
    workspaces: &crate::domain::workspace::service::WorkspaceService,
    workspace: WorkspaceId,
    enable: bool,
) -> Result<(), String> {
    let row = workspaces
        .get_workspace(workspace)
        .await
        .map_err(|err| err.to_string())?;
    let root = PathBuf::from(&row.root);
    let root = root.canonicalize().unwrap_or(root);
    let hash = if enable {
        let path = project_path(&root);
        Some(
            project_hash(&root)
                .await
                .ok_or_else(|| format!("there is no {} to trust", path.display()))?,
        )
    } else {
        None
    };
    workspaces
        .set_mcp_project_sha256(workspace, hash)
        .await
        .map_err(|err| err.to_string())
}

/// One supervisor for the process. Sessions of a workspace share its servers.
pub struct McpHub {
    inner: Mutex<HubState>,
    /// Serializes connect and close so a switch cannot race a start.
    gate: Mutex<()>,
    /// Workspace the shell has open. `None` until the first focus.
    open: Mutex<Option<WorkspaceId>>,
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

struct LiveServer {
    status: &'static str,
    title: Option<String>,
    icon: Option<String>,
    session: Option<Arc<dyn McpSession>>,
    listed: Option<Listed>,
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
            gate: Mutex::new(()),
            open: Mutex::new(None),
            settings,
            home,
            bus,
        }
    }

    /// The connection opener this hub uses.
    ///
    /// Carries the hub's home and settings, so a server's log lands under
    /// `~/.robi/logs/mcp/<id>/` and its child `PATH` includes `path_entries`.
    pub fn opener(&self) -> RmcpOpener {
        RmcpOpener::new(self.home.clone(), Arc::clone(&self.settings))
    }

    /// Start this workspace's servers and close every other workspace's.
    ///
    /// The shell calls this when the open workspace changes. Connections stay
    /// up for that workspace until the next switch, a delete, or process exit.
    pub async fn focus(
        &self,
        workspace: WorkspaceId,
        root: &Path,
        stored_hash: Option<&str>,
        open: &dyn session::SessionOpener,
    ) {
        let _gate = self.gate.lock().await;
        *self.open.lock().await = Some(workspace);
        self.close_except(workspace).await;
        self.ensure(workspace, root, stored_hash, open).await;
    }

    /// Drop this workspace's connections. In-flight tool calls see a closed session.
    pub async fn close(&self, workspace: WorkspaceId) {
        let _gate = self.gate.lock().await;
        if self.open.lock().await.as_ref() == Some(&workspace) {
            *self.open.lock().await = None;
        }
        self.close_except_keeping(Some(workspace), true).await;
    }

    /// Read both JSON files and register tools for the servers that connect.
    ///
    /// Agent mode calls this before the first model request. Servers already
    /// open for the workspace are reused. A server that is still down is
    /// omitted. The project file is used only when `stored_hash` equals the
    /// file's SHA-256.
    pub async fn attach(
        &self,
        workspace: WorkspaceId,
        root: &Path,
        stored_hash: Option<&str>,
        registry: &ToolRegistry,
        allows: Arc<dyn AllowList>,
        open: &dyn session::SessionOpener,
    ) {
        let _gate = self.gate.lock().await;
        let servers = self.ensure(workspace, root, stored_hash, open).await;
        let filters: std::collections::HashMap<String, filter::ToolFilter> = servers
            .into_iter()
            .map(|server| (server.id, server.tools))
            .collect();
        let state = self.inner.lock().await;
        let Some(slot) = state.workspaces.get(&workspace) else {
            return;
        };
        let ready: Vec<_> = slot
            .live
            .iter()
            .filter_map(|(id, live)| {
                Some((id.clone(), live.session.clone()?, live.listed.clone()?))
            })
            .collect();
        drop(state);
        let mut state = self.inner.lock().await;
        let Some(slot) = state.workspaces.get_mut(&workspace) else {
            return;
        };
        for (id, session, listed) in ready {
            let filter = filters.get(&id).cloned().unwrap_or_default();
            let row = register_connected(
                registry,
                allows.clone(),
                slot,
                &id,
                session,
                listed,
                &filter,
            );
            slot.live.insert(id, row);
        }
    }

    /// Read both files, start any server that is not up, and return the parsed
    /// entries so a caller can apply the current tool filters.
    async fn ensure(
        &self,
        workspace: WorkspaceId,
        root: &Path,
        stored_hash: Option<&str>,
        open: &dyn session::SessionOpener,
    ) -> Vec<ServerConfig> {
        if self.open.lock().await.is_some_and(|id| id != workspace) {
            return Vec::new();
        }
        let loaded = config::load(
            &self.home,
            root,
            ProjectTrust::StoredHash(stored_hash),
            self.settings.as_ref(),
        )
        .await;
        tracing::info!(
            %workspace,
            servers = loaded.servers.len(),
            project_trusted = loaded.project_trusted,
            "mcp workspace"
        );
        if !loaded.project_trusted && loaded.project_present {
            tracing::info!(
                %workspace,
                "project mcp.json is not enabled for this workspace"
            );
        }
        for error in loaded.errors {
            tracing::error!(server = %error.id, reason = %error.message, "skipped mcp server");
        }
        let servers = loaded.servers;
        let mut pending = Vec::new();
        {
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
            let configured: std::collections::HashSet<_> =
                servers.iter().map(|server| server.id.clone()).collect();
            let stale: Vec<_> = slot
                .live
                .keys()
                .filter(|id| !configured.contains(*id))
                .cloned()
                .collect();
            let mut closed = Vec::new();
            for id in stale {
                slot.names.forget_server(&id);
                if let Some(row) = slot.live.remove(&id) {
                    if let Some(session) = row.session {
                        closed.push(session);
                    }
                }
            }
            drop(state);
            for session in closed {
                session.close().await;
            }
            let mut state = self.inner.lock().await;
            let Some(slot) = state.workspaces.get_mut(&workspace) else {
                return Vec::new();
            };
            for server in &servers {
                if slot
                    .live
                    .get(&server.id)
                    .is_some_and(|row| row.session.is_some())
                {
                    continue;
                }
                slot.live.insert(
                    server.id.clone(),
                    LiveServer {
                        status: "starting",
                        title: None,
                        icon: None,
                        session: None,
                        listed: None,
                    },
                );
                pending.push(server.clone());
            }
        }
        self.publish_status(workspace);
        for server in pending {
            tracing::info!(
                server = %server.id,
                target = %server_target(&server),
                "mcp server starting"
            );
            let opened = connect_server(open, &server, root).await;
            let mut state = self.inner.lock().await;
            let Some(slot) = state.workspaces.get_mut(&workspace) else {
                if let Ok((session, _)) = opened {
                    session.close().await;
                }
                continue;
            };
            match opened {
                Ok((session, listed)) => {
                    let (visible, unused) = server.tools.visible(&server.id, &listed.tools);
                    if server.tools.is_filtered() {
                        tracing::info!(
                            server = %server.id,
                            listed = listed.tools.len(),
                            registered = visible.len(),
                            "mcp tool filter applied"
                        );
                    }
                    tracing::info!(
                        server = %server.id,
                        tools = listed.tools.len(),
                        "mcp server connected"
                    );
                    for pattern in &unused {
                        tracing::info!(
                            server = %server.id,
                            pattern = %pattern,
                            "mcp tool filter matched nothing"
                        );
                    }
                    if let Some(icon) = listed.icon.clone() {
                        remember_icon(&self.home, &server.id, &icon);
                    }
                    slot.live.insert(
                        server.id,
                        LiveServer {
                            status: "connected",
                            title: listed.title.clone(),
                            icon: listed.icon.clone(),
                            session: Some(session),
                            listed: Some(listed),
                        },
                    );
                }
                Err(err) => {
                    tracing::error!(server = %server.id, reason = %err, "mcp server failed");
                    self.bus.publish(EventEnvelope::user_error(format!(
                        "MCP server {} failed to start: {err}",
                        server.id
                    )));
                    let title = slot.live.get(&server.id).and_then(|row| row.title.clone());
                    slot.names.forget_server(&server.id);
                    slot.live.insert(
                        server.id,
                        LiveServer {
                            status: "failed",
                            title,
                            icon: None,
                            session: None,
                            listed: None,
                        },
                    );
                }
            }
            drop(state);
            self.publish_status(workspace);
        }
        servers
    }

    async fn close_except(&self, keep: WorkspaceId) {
        self.close_except_keeping(Some(keep), false).await;
    }

    async fn close_except_keeping(&self, target: Option<WorkspaceId>, only_target: bool) {
        let mut state = self.inner.lock().await;
        let ids: Vec<_> = state
            .workspaces
            .keys()
            .copied()
            .filter(|id| {
                if only_target {
                    Some(*id) == target
                } else {
                    Some(*id) != target
                }
            })
            .collect();
        let mut sessions = Vec::new();
        for id in ids {
            if let Some(slot) = state.workspaces.remove(&id) {
                tracing::info!(workspace = %id, "mcp workspace closed");
                for row in slot.live.into_values() {
                    if let Some(session) = row.session {
                        sessions.push(session);
                    }
                }
            }
        }
        drop(state);
        for session in sessions {
            session.close().await;
        }
    }

    /// Tell subscribers this workspace's server list changed.
    pub fn publish_status(&self, workspace: WorkspaceId) {
        self.bus
            .publish(EventEnvelope::mcp_status(&workspace.to_string()));
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
        let loaded = config::load(
            &self.home,
            root,
            ProjectTrust::StoredHash(stored_hash),
            self.settings.as_ref(),
        )
        .await;
        let servers = loaded.servers;
        let home = self.home.clone();
        let cached = crate::agent::blocking::call(move || load_icons(&home))
            .await
            .unwrap_or_default();
        let state = self.inner.lock().await;
        let live = state.workspaces.get(&workspace);
        servers
            .into_iter()
            .map(|server| {
                let row = live.and_then(|slot| slot.live.get(&server.id));
                let icon = row
                    .and_then(|row| row.icon.clone())
                    .or_else(|| cached.get(&server.id).cloned());
                let tool_count = row
                    .and_then(|row| row.listed.as_ref())
                    .map(|listed| server.tools.visible(&server.id, &listed.tools).0.len() as u32)
                    .unwrap_or(0);
                McpServerStatus {
                    id: server.id,
                    status: row
                        .map(|row| row.status)
                        .unwrap_or("disconnected")
                        .to_owned(),
                    title: row.and_then(|row| row.title.clone()),
                    icon,
                    tool_count,
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
}

pub(crate) async fn connect_server(
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
        .map_err(|_| "initialize exceeded 15 seconds".to_owned())??;
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
    filter: &filter::ToolFilter,
) -> LiveServer {
    let previous = slot.names.forget_server(server);
    let (visible, _unused) = filter.visible(server, &listed.tools);
    let title = listed.title.clone();
    let icon = listed.icon.clone();
    let rows = register_list(
        registry,
        server,
        &visible,
        Arc::clone(&session),
        Arc::new(Mutex::new(())),
        allows,
        &previous,
    );
    slot.names.remember(&rows);
    LiveServer {
        status: "connected",
        title,
        icon,
        session: Some(session),
        listed: Some(listed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::memory::MemorySettingsStore;

    #[tokio::test]
    async fn a_configured_server_is_disconnected_until_the_workspace_is_focused() {
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

    fn listed_tool(name: &str) -> ListedTool {
        ListedTool {
            name: name.into(),
            description: Some("do a thing".into()),
            input_schema: None,
            read_only: true,
            destructive: false,
            open_world: false,
        }
    }

    struct FilteredServer;

    #[async_trait::async_trait]
    impl McpSession for FilteredServer {
        async fn list_tools(&self) -> Result<Listed, String> {
            Ok(Listed {
                tools: vec![listed_tool("keep_one"), listed_tool("drop_two")],
                instructions: None,
                title: None,
                icon: None,
            })
        }

        async fn call_tool(
            &self,
            _name: &str,
            _args: serde_json::Value,
            _cancel: &tokio_util::sync::CancellationToken,
        ) -> Result<serde_json::Value, String> {
            Err("not called".into())
        }
    }

    struct FilteredOpener;

    #[async_trait::async_trait]
    impl session::SessionOpener for FilteredOpener {
        async fn open(&self, _spec: session::OpenSpec) -> Result<Box<dyn McpSession>, String> {
            Ok(Box::new(FilteredServer))
        }
    }

    struct Never;

    #[async_trait::async_trait]
    impl AllowList for Never {
        async fn allows(&self, _server: &str, _tool: &str) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn a_tools_list_keeps_the_other_tools_out_of_the_registry() {
        let dir = std::env::temp_dir().join(format!("robi-mcp-filter-{}", std::process::id()));
        let root = dir.join("workspace");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            dir.join("mcp.json"),
            r#"{"mcpServers":{"srv":{"command":"fake","tools":["keep_*"]}}}"#,
        )
        .unwrap();
        let hub = McpHub::new(
            Arc::new(MemorySettingsStore::new()),
            dir.clone(),
            Arc::new(EventBus::new()),
        );
        let workspace = WorkspaceId::new();
        let registry = ToolRegistry::new();
        hub.attach(
            workspace,
            &root,
            None,
            &registry,
            Arc::new(Never),
            &FilteredOpener,
        )
        .await;
        assert!(registry.get("mcp_srv_keep_one").is_some());
        assert!(registry.get("mcp_srv_drop_two").is_none());
        let rows = hub.status(workspace, &root, None).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "connected");
        assert_eq!(rows[0].tool_count, 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod trust_tests {
    use super::*;
    use crate::domain::workspace::model::Workspace;
    use crate::domain::workspace::repo::WorkspaceRepository;
    use crate::domain::workspace::service::WorkspaceService;
    use async_trait::async_trait;

    /// One workspace whose row records what the trust path stored.
    struct Recording {
        root: String,
        stored: std::sync::Mutex<Vec<Option<String>>>,
    }

    impl Recording {
        fn new(root: &Path) -> Self {
            Self {
                root: root.display().to_string(),
                stored: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn writes(&self) -> Vec<Option<String>> {
            self.stored.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl WorkspaceRepository for Recording {
        async fn canonicalize_root(
            &self,
            root: &str,
        ) -> Result<String, crate::domain::error::ServiceError> {
            Ok(root.to_owned())
        }

        async fn get_workspace(
            &self,
            id: WorkspaceId,
        ) -> Result<Option<Workspace>, crate::domain::error::ServiceError> {
            Ok(Some(Workspace {
                id,
                name: "recording".to_owned(),
                root: self.root.clone(),
                mcp_project_sha256: None,
                created_at: chrono::Utc::now(),
            }))
        }

        async fn get_workspace_by_root(
            &self,
            _root: &str,
        ) -> Result<Option<Workspace>, crate::domain::error::ServiceError> {
            Ok(None)
        }

        async fn insert_workspace(
            &self,
            root: &str,
            name: &str,
        ) -> Result<Workspace, crate::domain::error::ServiceError> {
            Ok(Workspace {
                id: WorkspaceId::new(),
                name: name.to_owned(),
                root: root.to_owned(),
                mcp_project_sha256: None,
                created_at: chrono::Utc::now(),
            })
        }

        async fn list_workspaces(
            &self,
        ) -> Result<Vec<Workspace>, crate::domain::error::ServiceError> {
            Ok(Vec::new())
        }

        async fn delete_workspace(
            &self,
            id: WorkspaceId,
        ) -> Result<(), crate::domain::error::ServiceError> {
            Err(crate::domain::error::ServiceError::NotFound(id.to_string()))
        }

        async fn set_mcp_project_sha256(
            &self,
            _id: WorkspaceId,
            hash: Option<String>,
        ) -> Result<(), crate::domain::error::ServiceError> {
            self.stored.lock().unwrap().push(hash);
            Ok(())
        }
    }

    fn workspace_with_project_file(bytes: &[u8]) -> (std::path::PathBuf, Arc<Recording>) {
        let dir = std::env::temp_dir().join(format!(
            "robi-mcp-trust-{}-{}",
            std::process::id(),
            file_hash(bytes)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".robi")).unwrap();
        std::fs::write(dir.join(".robi").join("mcp.json"), bytes).unwrap();
        let root = dir.canonicalize().unwrap();
        (root.clone(), Arc::new(Recording::new(&root)))
    }

    #[tokio::test]
    async fn trusting_stores_the_hash_of_the_bytes_on_disk() {
        let bytes = br#"{"mcpServers":{"git":{"command":"git-mcp"}}}"#;
        let (root, repo) = workspace_with_project_file(bytes);
        let service = WorkspaceService {
            repository: repo.clone(),
            asset_cleaner: None,
        };
        set_project_trust(&service, WorkspaceId::new(), true)
            .await
            .unwrap();
        assert_eq!(repo.writes(), vec![Some(file_hash(bytes))]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn distrusting_clears_the_column_without_reading_the_file() {
        let (root, repo) = workspace_with_project_file(b"{}");
        std::fs::remove_file(root.join(".robi").join("mcp.json")).unwrap();
        let service = WorkspaceService {
            repository: repo.clone(),
            asset_cleaner: None,
        };
        set_project_trust(&service, WorkspaceId::new(), false)
            .await
            .unwrap();
        assert_eq!(repo.writes(), vec![None]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn trusting_a_workspace_with_no_project_file_names_the_path() {
        let (root, repo) = workspace_with_project_file(b"{}");
        std::fs::remove_file(root.join(".robi").join("mcp.json")).unwrap();
        let service = WorkspaceService {
            repository: repo.clone(),
            asset_cleaner: None,
        };
        let err = set_project_trust(&service, WorkspaceId::new(), true)
            .await
            .unwrap_err();
        assert!(err.contains("mcp.json"), "{err}");
        assert!(repo.writes().is_empty(), "nothing was stored");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn one_byte_of_edit_changes_the_hash_the_surface_would_store() {
        let first = br#"{"mcpServers":{"git":{"command":"git-mcp"}}}"#;
        let (root, _) = workspace_with_project_file(first);
        let before = project_hash(&root).await.expect("a hash");
        std::fs::write(
            root.join(".robi").join("mcp.json"),
            br#"{"mcpServers":{"git":{"command":"git-mcp"}} }"#,
        )
        .unwrap();
        let after = project_hash(&root).await.expect("a hash");
        assert_ne!(before, after);
        let _ = std::fs::remove_dir_all(root);
    }
}
