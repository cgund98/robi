//! Outbound MCP connections. This process does not listen for MCP clients.

use std::collections::BTreeMap;

use async_trait::async_trait;
use rmcp::model::{
    CallToolRequestParam, CallToolResult, CancelledNotificationParam, ClientCapabilities,
    ClientInfo, Implementation, ListRootsResult, Root,
};
use rmcp::service::{RoleClient, RunningService, ServiceExt};
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{ClientHandler, ServiceError};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{ChildStderr, Command};
use tokio_util::sync::CancellationToken;

use super::config::{redirect_allowed, Transport};
use super::env::{child_env, resolve_command};
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
    _service: RunningService<RoleClient, Handler>,
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

pub struct RmcpOpener;

#[async_trait]
impl SessionOpener for RmcpOpener {
    async fn open(&self, spec: OpenSpec) -> Result<Box<dyn McpSession>, String> {
        let handler = Handler {
            root: spec.workspace_root.display().to_string(),
        };
        let service = match spec.transport {
            Transport::Stdio { command, args, env } => {
                open_stdio(handler, &spec.workspace_root, &command, &args, &env).await?
            }
            Transport::Http { url, headers } => open_http(handler, &url, &headers).await?,
        };
        let peer = service.peer().clone();
        Ok(Box::new(RmcpSession {
            peer,
            _service: service,
        }))
    }
}

async fn open_stdio(
    handler: Handler,
    root: &std::path::Path,
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
) -> Result<RunningService<RoleClient, Handler>, String> {
    let child_env = child_env(root, env);
    let resolved = resolve_command(command, &child_env)
        .ok_or_else(|| format!("{command} is not on the constructed PATH"))?;
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
        log_stderr(resolved.display().to_string(), stderr);
    }
    handler
        .serve(transport)
        .await
        .map_err(|err| format!("initialize {}: {err}", resolved.display()))
}

fn log_stderr(command: String, stderr: ChildStderr) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut logged = 0usize;
        const CAP: usize = 8 * 1024;
        while let Ok(Some(line)) = lines.next_line().await {
            if logged >= CAP {
                tracing::warn!(%command, "mcp stderr truncated");
                break;
            }
            logged += line.len();
            tracing::warn!(%command, stderr = %line, "mcp stderr");
        }
    });
}

async fn open_http(
    handler: Handler,
    url: &str,
    headers: &BTreeMap<String, String>,
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
    tracing::info!(%url, authorization, "mcp http connect");
    handler
        .serve(transport)
        .await
        .map_err(|err| format!("initialize {url}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::bearer_token;

    #[test]
    fn a_bearer_prefix_is_not_sent_twice() {
        assert_eq!(bearer_token("Bearer lin_api_x"), "lin_api_x");
        assert_eq!(bearer_token("lin_api_x"), "lin_api_x");
    }
}
