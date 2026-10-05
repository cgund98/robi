//! Outbound MCP connections. This process does not listen for MCP clients.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use rmcp::model::{
    CallToolRequestParam, CallToolResult, CancelledNotificationParam, ClientCapabilities,
    ClientInfo, Implementation, ListRootsResult, Root,
};
use rmcp::service::{RoleClient, RunningService, RxJsonRpcMessage, ServiceExt, TxJsonRpcMessage};
use rmcp::transport::Transport as RmcpTransport;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{ClientHandler, ServiceError};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{ChildStderr, Command};
use tokio_util::sync::CancellationToken;

use crate::domain::settings::store::SettingsStore;

use super::config::{redirect_allowed, Transport};
use super::env::{child_env, login_path, resolve_command};
use super::logs::ServerLog;
use super::session::{Listed, ListedTool, McpSession, OpenSpec, SessionOpener};

struct Handler {
    root: String,
}

impl ClientHandler for Handler {
    fn get_info(&self) -> ClientInfo {
        ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::builder().enable_roots().build(),
            client_info: Implementation {
                name: "robi".into(),
                title: None,
                version: env!("CARGO_PKG_VERSION").into(),
                icons: None,
                website_url: None,
            },
        }
    }

    async fn list_roots(
        &self,
        _context: rmcp::service::RequestContext<RoleClient>,
    ) -> Result<ListRootsResult, rmcp::ErrorData> {
        Ok(ListRootsResult {
            roots: vec![Root {
                uri: format!("file://{}", self.root),
                name: Some("workspace".into()),
            }],
        })
    }
}

pub struct RmcpSession {
    peer: rmcp::service::Peer<RoleClient>,
    service: tokio::sync::Mutex<Option<RunningService<RoleClient, Handler>>>,
}

#[async_trait]
impl McpSession for RmcpSession {
    async fn list_tools(&self) -> Result<Listed, String> {
        let info = self.peer.peer_info();
        let instructions = info.as_ref().and_then(|info| info.instructions.clone());
        let title = info
            .as_ref()
            .and_then(|info| info.server_info.title.clone());
        let icon = info
            .as_ref()
            .and_then(|info| icon_src(info.server_info.icons.as_deref()));
        let tools = self
            .peer
            .list_all_tools()
            .await
            .map_err(|err| err.to_string())?;
        Ok(Listed {
            instructions,
            title,
            icon,
            tools: tools
                .into_iter()
                .map(|tool| ListedTool {
                    name: tool.name.to_string(),
                    description: tool.description.map(|text| text.to_string()),
                    input_schema: serde_json::to_value(tool.input_schema).ok(),
                    read_only: tool
                        .annotations
                        .as_ref()
                        .and_then(|hints| hints.read_only_hint)
                        .unwrap_or(false),
                    destructive: tool
                        .annotations
                        .as_ref()
                        .and_then(|hints| hints.destructive_hint)
                        .unwrap_or(false),
                    open_world: tool
                        .annotations
                        .as_ref()
                        .and_then(|hints| hints.open_world_hint)
                        .unwrap_or(false),
                })
                .collect(),
        })
    }

    async fn call_tool(
        &self,
        name: &str,
        args: Value,
        cancel: &CancellationToken,
    ) -> Result<Value, String> {
        if cancel.is_cancelled() {
            return Err("cancelled".into());
        }
        let arguments = match args {
            Value::Object(map) => Some(map),
            _ => None,
        };
        let call = self.peer.call_tool(CallToolRequestParam {
            name: name.to_owned().into(),
            arguments,
        });
        tokio::select! {
            result = call => result
                .map(result_value)
                .map_err(call_error),
            _ = cancel.cancelled() => {
                let _ = self.peer.notify_cancelled(CancelledNotificationParam {
                    request_id: rmcp::model::NumberOrString::Number(0),
                    reason: Some("cancelled".into()),
                }).await;
                Err("cancelled".into())
            }
        }
    }

    async fn close(&self) {
        self.service.lock().await.take();
    }
}

fn icon_src(icons: Option<&[rmcp::model::Icon]>) -> Option<String> {
    icons.and_then(|icons| {
        icons.iter().find_map(|icon| {
            let src = icon.src.trim();
            if src.starts_with("https://") || src.starts_with("data:image/") {
                Some(src.to_owned())
            } else {
                None
            }
        })
    })
}

/// `rmcp` sends this value with `Authorization: Bearer`. A stored `Bearer ` prefix is removed so it is not doubled.
fn bearer_token(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
        .unwrap_or(value)
        .trim()
        .to_owned()
}

fn result_value(result: CallToolResult) -> Value {
    serde_json::to_value(result).unwrap_or(Value::Null)
}

fn call_error(err: ServiceError) -> String {
    let text = err.to_string();
    if text.to_ascii_lowercase().contains("timeout") {
        "timeout".into()
    } else {
        text
    }
}

/// Opens one configured server over `rmcp`.
pub struct RmcpOpener {
    home: PathBuf,
    settings: Arc<dyn SettingsStore>,
}

impl RmcpOpener {
    pub fn new(home: PathBuf, settings: Arc<dyn SettingsStore>) -> Self {
        Self { home, settings }
    }

    /// The `path_entries` setting, appended to the child `PATH`.
    async fn path_entries(&self) -> String {
        self.settings
            .get(crate::domain::settings::keys::PATH_ENTRIES)
            .await
            .ok()
            .flatten()
            .map(|setting| setting.value)
            .unwrap_or_default()
    }
}

#[async_trait]
impl SessionOpener for RmcpOpener {
    async fn open(&self, spec: OpenSpec) -> Result<Box<dyn McpSession>, String> {
        let home = self.home.clone();
        let server_id = spec.server_id.clone();
        let log = crate::agent::blocking::call(move || ServerLog::open(&home, &server_id))
            .await
            .unwrap_or_else(|_| ServerLog::disabled());
        let handler = Handler {
            root: spec.workspace_root.display().to_string(),
        };
        let service = match spec.transport {
            Transport::Stdio { command, args, env } => {
                let base_path = crate::agent::blocking::call(login_path)
                    .await
                    .unwrap_or_else(|_| std::env::var("PATH").unwrap_or_default());
                let extra_path = self.path_entries().await;
                open_stdio(
                    handler,
                    &spec.workspace_root,
                    &command,
                    &args,
                    &env,
                    &base_path,
                    &extra_path,
                    &log,
                )
                .await?
            }
            Transport::Http { url, headers } => open_http(handler, &url, &headers, &log).await?,
        };
        let peer = service.peer().clone();
        Ok(Box::new(RmcpSession {
            peer,
            service: tokio::sync::Mutex::new(Some(service)),
        }))
    }
}

/// Wraps a transport so every JSON-RPC message is written to the server log.
///
/// `send` and `receive` are the one point both stdio and streamable HTTP pass
/// through, so the log captures `initialize`, `tools/list`, `tools/call`, the
/// notifications, and the errors identically for both.
struct LoggingTransport<T> {
    inner: T,
    log: ServerLog,
}

impl<T> RmcpTransport<RoleClient> for LoggingTransport<T>
where
    T: RmcpTransport<RoleClient>,
{
    type Error = T::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.log.protocol_out(&item);
        self.inner.send(item)
    }

    fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleClient>>> + Send {
        let log = self.log.clone();
        let fut = self.inner.receive();
        async move {
            let message = fut.await;
            if let Some(message) = &message {
                log.protocol_in(message);
            }
            message
        }
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.inner.close()
    }
}

#[allow(clippy::too_many_arguments)]
async fn open_stdio(
    handler: Handler,
    root: &std::path::Path,
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    base_path: &str,
    extra_path: &str,
    log: &ServerLog,
) -> Result<RunningService<RoleClient, Handler>, String> {
    let child_env = child_env(root, env, base_path, extra_path);
    let resolved = resolve_command(command, &child_env)
        .ok_or_else(|| format!("{command} is not on the constructed PATH"))?;
    log.line(
        "INFO",
        &format!(
            "stdio command={} args={} cwd={}",
            resolved.display(),
            args.join(" "),
            root.display()
        ),
    );
    tracing::info!(
        command = %resolved.display(),
        args = %args.join(" "),
        cwd = %root.display(),
        "mcp stdio spawn"
    );
    let mut cmd = Command::new(&resolved);
    cmd.args(args)
        .current_dir(root)
        .env_clear()
        .envs(child_env)
        .kill_on_drop(true);
    let (transport, stderr) = TokioChildProcess::builder(cmd)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("spawn {}: {err}", resolved.display()))?;
    if let Some(stderr) = stderr {
        log_stderr(log.clone(), resolved.display().to_string(), stderr);
    }
    handler
        .serve(LoggingTransport {
            inner: transport,
            log: log.clone(),
        })
        .await
        .map_err(|err| format!("initialize {}: {err}", resolved.display()))
}

/// Copy the child's stderr into the per-server log. The per-line bound on the
/// log, and the weekly prune, keep the file from growing without limit.
fn log_stderr(log: ServerLog, command: String, stderr: ChildStderr) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log.stderr(&command, &line);
        }
    });
}

async fn open_http(
    handler: Handler,
    url: &str,
    headers: &BTreeMap<String, String>,
    log: &ServerLog,
) -> Result<RunningService<RoleClient, Handler>, String> {
    let mut config =
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url);
    let mut request = reqwest::header::HeaderMap::new();
    let mut authorization = false;
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("authorization") {
            let token = bearer_token(value);
            if token.is_empty() {
                return Err("authorization header is empty".into());
            }
            config = config.auth_header(token);
            authorization = true;
            continue;
        }
        let header_name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("header name {name}"))?;
        let header_value =
            reqwest::header::HeaderValue::from_str(value).map_err(|_| "header value".to_owned())?;
        request.insert(header_name, header_value);
    }
    if !authorization {
        tracing::warn!(%url, "mcp http server has no authorization header");
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if redirect_allowed(attempt.url().as_str()) {
                attempt.follow()
            } else {
                attempt.error("redirect left the allowed url")
            }
        }))
        .default_headers(request)
        .build()
        .map_err(|err| format!("http client: {err}"))?;
    let transport = StreamableHttpClientTransport::with_client(client, config);
    // The target and whether auth is present, never the header values.
    log.line(
        "INFO",
        &format!("http url={url} authorization={authorization}"),
    );
    tracing::info!(%url, authorization, "mcp http connect");
    handler
        .serve(LoggingTransport {
            inner: transport,
            log: log.clone(),
        })
        .await
        .map_err(|err| format!("initialize {url}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{ClientRequest, JsonRpcMessage, PingRequest, RequestId, ServerRequest};

    #[test]
    fn a_bearer_prefix_is_not_sent_twice() {
        assert_eq!(bearer_token("Bearer lin_api_x"), "lin_api_x");
        assert_eq!(bearer_token("lin_api_x"), "lin_api_x");
    }

    /// A transport that records what it sent and yields one queued inbound
    /// message, so the logging wrapper can be checked without a server.
    struct FakeTransport {
        inbound: Option<RxJsonRpcMessage<RoleClient>>,
        sent: Vec<TxJsonRpcMessage<RoleClient>>,
    }

    impl RmcpTransport<RoleClient> for FakeTransport {
        type Error = std::io::Error;

        fn send(
            &mut self,
            item: TxJsonRpcMessage<RoleClient>,
        ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
            self.sent.push(item);
            async { Ok(()) }
        }

        fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleClient>>> + Send {
            let message = self.inbound.take();
            async move { message }
        }

        async fn close(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn the_logging_transport_writes_both_directions() {
        let home = std::env::temp_dir().join(format!("robi-mcp-transport-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let path = home.join("mcp-now.log");
        let log = ServerLog::to_file(std::fs::File::create(&path).unwrap());

        let inbound: RxJsonRpcMessage<RoleClient> = JsonRpcMessage::request(
            ServerRequest::PingRequest(PingRequest::default()),
            RequestId::Number(2),
        );
        let mut transport = LoggingTransport {
            inner: FakeTransport {
                inbound: Some(inbound),
                sent: Vec::new(),
            },
            log,
        };
        transport
            .send(JsonRpcMessage::request(
                ClientRequest::PingRequest(PingRequest::default()),
                RequestId::Number(1),
            ))
            .await
            .unwrap();
        let received = transport.receive().await;
        assert!(received.is_some());

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("-> "), "{text}");
        assert!(text.contains("<- "), "{text}");
        assert_eq!(transport.inner.sent.len(), 1);
        let _ = std::fs::remove_dir_all(&home);
    }
}
