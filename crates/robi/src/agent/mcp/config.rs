//! `~/.robi/mcp.json` and `<workspace>/.robi/mcp.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::filter::ToolFilter;
use super::names::sha256_hex;
use crate::domain::settings::store::SettingsStore;

const ID: &str = r"^[A-Za-z0-9_-]{1,32}$";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub id: String,
    pub transport: Transport,
    pub timeout_seconds: u64,
    /// Which of this server's tools reach the session. `all()` unless the entry
    /// sets `tools`.
    pub tools: ToolFilter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
    },
    Http {
        url: String,
        headers: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPreview {
    pub id: String,
    pub command: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LoadError {
    pub id: String,
    pub message: String,
}

/// Whether the project file's servers may start.
#[derive(Debug, Clone, Copy)]
pub enum ProjectTrust<'a> {
    /// Compare the file's SHA-256 with the hash stored on the workspace row.
    StoredHash(Option<&'a str>),
    /// The caller is looking at the bytes: the diagnostic CLI's `--trust`.
    Always,
}

/// Both files read, merged, and resolved. What a caller needs to start servers.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub servers: Vec<ServerConfig>,
    pub errors: Vec<LoadError>,
    /// Whether a project file exists, trusted or not.
    pub project_present: bool,
    pub project_trusted: bool,
}

/// Read `~/.robi/mcp.json` and `<root>/.robi/mcp.json`, then merge them.
///
/// The one read path: the hub starts servers from it, reports status from it,
/// and the `mcp-tools` binary lists from it. An untrusted project file is read
/// and reported but contributes no server.
pub async fn load(
    home: &Path,
    root: &Path,
    trust: ProjectTrust<'_>,
    settings: &dyn SettingsStore,
) -> Loaded {
    let settings_home = home.to_path_buf();
    let read_home = settings_home.clone();
    let project_file = project_path(root);
    let (user, project_bytes) = crate::agent::blocking::call(move || {
        (
            read_file(&user_path(&read_home)),
            std::fs::read(&project_file).unwrap_or_default(),
        )
    })
    .await
    .unwrap_or_default();
    let project_present = !project_bytes.is_empty();
    let project_trusted = match trust {
        ProjectTrust::Always => true,
        ProjectTrust::StoredHash(stored) => {
            stored.is_some_and(|hash| hash == file_hash(&project_bytes))
        }
    };
    let project = String::from_utf8_lossy(&project_bytes).into_owned();
    let resolved = secrets(&settings_home, settings).await;
    let (servers, errors) = merge(&user, &project, project_trusted, &resolved);
    Loaded {
        servers,
        errors,
        project_present,
        project_trusted: project_trusted && project_present,
    }
}

/// Secret values a config entry may reference.
///
/// A key in `secrets.toml` counts only when the store has it marked secret.
/// The values are never logged.
pub async fn secrets(home: &Path, settings: &dyn SettingsStore) -> BTreeMap<String, String> {
    let path = home.join("secrets.toml");
    let Ok(body) = crate::agent::blocking::call(move || std::fs::read_to_string(&path))
        .await
        .unwrap_or(Err(std::io::Error::other("filesystem read failed")))
    else {
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
        if settings
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

/// User servers, then project servers when `project_trusted` is true.
///
/// The project entry replaces the user entry with the same id.
pub fn merge(
    user: &str,
    project: &str,
    project_trusted: bool,
    secrets: &BTreeMap<String, String>,
) -> (Vec<ServerConfig>, Vec<LoadError>) {
    let (mut servers, mut errors) = parse(user, secrets);
    if !project_trusted {
        return (servers, errors);
    }
    let (project_servers, project_errors) = parse(project, secrets);
    errors.extend(project_errors);
    for server in project_servers {
        servers.retain(|existing| existing.id != server.id);
        servers.push(server);
    }
    (servers, errors)
}

pub fn parse(
    text: &str,
    secrets: &BTreeMap<String, String>,
) -> (Vec<ServerConfig>, Vec<LoadError>) {
    if text.trim().is_empty() {
        return (Vec::new(), Vec::new());
    }
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(err) => {
            return (
                Vec::new(),
                vec![LoadError {
                    id: "*".into(),
                    message: format!("mcp.json did not parse: {err}"),
                }],
            );
        }
    };
    let Some(map) = value.get("mcpServers").and_then(Value::as_object) else {
        return (Vec::new(), Vec::new());
    };
    let id_pattern = regex::Regex::new(ID).expect("server id pattern");
    let mut servers = Vec::new();
    let mut errors = Vec::new();
    for (id, body) in map {
        if !id_pattern.is_match(id) {
            errors.push(LoadError {
                id: id.clone(),
                message: "server id is not ^[A-Za-z0-9_-]{1,32}$".into(),
            });
            continue;
        }
        match one(id, body, secrets) {
            Ok(Some(server)) => servers.push(server),
            Ok(None) => {}
            Err(message) => errors.push(LoadError {
                id: id.clone(),
                message,
            }),
        }
    }
    (servers, errors)
}

/// Servers the trust dialog lists. Env and header values stay out.
pub fn preview(text: &str) -> Vec<ServerPreview> {
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let Some(map) = value.get("mcpServers").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for (id, body) in map {
        let command = body.get("command").and_then(Value::as_str).map(|command| {
            let args = body
                .get("args")
                .and_then(Value::as_array)
                .map(|args| {
                    args.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            if args.is_empty() {
                command.to_owned()
            } else {
                format!("{command} {args}")
            }
        });
        rows.push(ServerPreview {
            id: id.clone(),
            command,
            url: body.get("url").and_then(Value::as_str).map(str::to_owned),
        });
    }
    rows
}

pub fn read_file(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

pub fn user_path(home: &Path) -> PathBuf {
    home.join("mcp.json")
}

pub fn project_path(root: &Path) -> PathBuf {
    root.join(".robi").join("mcp.json")
}

fn one(
    id: &str,
    body: &Value,
    secrets: &BTreeMap<String, String>,
) -> Result<Option<ServerConfig>, String> {
    if body.get("enabled").and_then(Value::as_bool) == Some(false) {
        return Ok(None);
    }
    let command = body.get("command").and_then(Value::as_str);
    let url = body.get("url").and_then(Value::as_str);
    let timeout = timeout_seconds(body)?;
    let tools = tool_filter(body)?;
    match (command, url) {
        (Some(_), Some(_)) => Err("server has both command and url".into()),
        (None, None) => Err("server has neither command nor url".into()),
        (Some(command), None) => {
            if command.contains('/') && !Path::new(command).is_absolute() {
                return Err("command must be a name or an absolute path".into());
            }
            if body.get("url").is_some() {
                return Err("stdio server has url".into());
            }
            Ok(Some(ServerConfig {
                id: id.to_owned(),
                timeout_seconds: timeout,
                tools,
                transport: Transport::Stdio {
                    command: command.to_owned(),
                    args: string_list(body, "args")?,
                    env: string_map(body, "env", secrets)?,
                },
            }))
        }
        (None, Some(url)) => {
            check_url(url)?;
            Ok(Some(ServerConfig {
                id: id.to_owned(),
                timeout_seconds: timeout,
                tools,
                transport: Transport::Http {
                    url: url.to_owned(),
                    headers: string_map(body, "headers", secrets)?,
                },
            }))
        }
    }
}

/// Absent or `null` loads every tool. An array filters, and `[]` loads none.
fn tool_filter(body: &Value) -> Result<ToolFilter, String> {
    match body.get("tools") {
        None | Some(Value::Null) => Ok(ToolFilter::all()),
        Some(_) => ToolFilter::new(&string_list(body, "tools")?),
    }
}

fn timeout_seconds(body: &Value) -> Result<u64, String> {
    let Some(value) = body.get("timeout_seconds") else {
        return Ok(60);
    };
    let seconds = value
        .as_u64()
        .ok_or_else(|| "timeout_seconds must be a whole number".to_owned())?;
    if seconds > 300 {
        return Err("timeout_seconds above 300".into());
    }
    Ok(seconds)
}

fn string_list(body: &Value, field: &str) -> Result<Vec<String>, String> {
    let Some(value) = body.get(field) else {
        return Ok(Vec::new());
    };
    let list = value
        .as_array()
        .ok_or_else(|| format!("{field} must be an array of strings"))?;
    list.iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{field} must be an array of strings"))
        })
        .collect()
}

fn string_map(
    body: &Value,
    field: &str,
    secrets: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    let Some(value) = body.get(field) else {
        return Ok(BTreeMap::new());
    };
    let map = value
        .as_object()
        .ok_or_else(|| format!("{field} must be an object"))?;
    let mut resolved = BTreeMap::new();
    for (key, value) in map {
        resolved.insert(key.clone(), resolve_value(value, secrets)?);
    }
    Ok(resolved)
}

fn resolve_value(value: &Value, secrets: &BTreeMap<String, String>) -> Result<String, String> {
    if let Some(text) = value.as_str() {
        return Ok(text.to_owned());
    }
    if let Some(name) = value.get("secret").and_then(Value::as_str) {
        return secrets
            .get(name)
            .cloned()
            .ok_or_else(|| format!("missing secret {name}"));
    }
    Err("env or header value must be a string or a secret object".into())
}

pub fn check_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| format!("url is not valid: {url}"))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" => {
            let host = parsed.host_str().unwrap_or("");
            if host == "127.0.0.1" || host == "localhost" {
                Ok(())
            } else {
                Err("http is only allowed for 127.0.0.1 and localhost".into())
            }
        }
        _ => Err("url must be https, or http on loopback".into()),
    }
}

/// Whether a redirect target is still an allowed MCP URL.
pub fn redirect_allowed(next: &str) -> bool {
    check_url(next).is_ok()
}

pub fn file_hash(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mcp::session::ListedTool;

    fn secrets() -> BTreeMap<String, String> {
        BTreeMap::from([("github_token".into(), "gh".into())])
    }

    #[test]
    fn a_missing_secret_skips_that_server() {
        let text =
            r#"{"mcpServers":{"github":{"command":"npx","env":{"TOKEN":{"secret":"nope"}}}}}"#;
        let (servers, errors) = parse(text, &secrets());
        assert!(servers.is_empty());
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn a_project_file_is_ignored_until_the_hash_matches() {
        let user = r#"{"mcpServers":{"git":{"command":"git-mcp"}}}"#;
        let project =
            r#"{"mcpServers":{"git":{"command":"other"},"extra":{"url":"https://mcp.example"}}}"#;
        let (off, _) = merge(user, project, false, &secrets());
        assert_eq!(off.len(), 1);
        match &off[0].transport {
            Transport::Stdio { command, .. } => assert_eq!(command, "git-mcp"),
            Transport::Http { .. } => panic!("user server"),
        }
        let (on, _) = merge(user, project, true, &secrets());
        assert_eq!(on.len(), 2);
        assert!(on.iter().any(|server| server.id == "git"
            && matches!(&server.transport, Transport::Stdio { command, .. } if command == "other")));
    }

    #[test]
    fn a_one_byte_change_breaks_the_stored_hash() {
        let original = br#"{"mcpServers":{"git":{"command":"git-mcp"}}}"#;
        let changed = br#"{"mcpServers":{"git":{"command":"git-mcp"}} }"#;
        assert_ne!(file_hash(original), file_hash(changed));
    }

    #[tokio::test]
    async fn an_untrusted_project_file_contributes_no_server() {
        let dir = temp_home("untrusted");
        let root = dir.join("workspace");
        std::fs::create_dir_all(root.join(".robi")).unwrap();
        std::fs::write(
            dir.join("mcp.json"),
            r#"{"mcpServers":{"git":{"command":"git-mcp"}}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join(".robi").join("mcp.json"),
            r#"{"mcpServers":{"extra":{"command":"other"}}}"#,
        )
        .unwrap();
        let store = crate::domain::settings::memory::MemorySettingsStore::new();
        let loaded = load(&dir, &root, ProjectTrust::StoredHash(None), &store).await;
        assert_eq!(loaded.servers.len(), 1);
        assert_eq!(loaded.servers[0].id, "git");
        assert!(loaded.project_present);
        assert!(!loaded.project_trusted);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn always_trusts_the_project_file_and_lets_it_replace_an_id() {
        let dir = temp_home("always");
        let root = dir.join("workspace");
        std::fs::create_dir_all(root.join(".robi")).unwrap();
        std::fs::write(
            dir.join("mcp.json"),
            r#"{"mcpServers":{"git":{"command":"git-mcp"}}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join(".robi").join("mcp.json"),
            r#"{"mcpServers":{"git":{"command":"other"}}}"#,
        )
        .unwrap();
        let store = crate::domain::settings::memory::MemorySettingsStore::new();
        let loaded = load(&dir, &root, ProjectTrust::Always, &store).await;
        assert!(loaded.project_trusted);
        assert_eq!(loaded.servers.len(), 1);
        match &loaded.servers[0].transport {
            Transport::Stdio { command, .. } => assert_eq!(command, "other"),
            Transport::Http { .. } => panic!("expected stdio"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_stored_hash_that_matches_trusts_the_file() {
        let dir = temp_home("hash");
        let root = dir.join("workspace");
        std::fs::create_dir_all(root.join(".robi")).unwrap();
        let bytes = br#"{"mcpServers":{"extra":{"command":"other"}}}"#;
        std::fs::write(root.join(".robi").join("mcp.json"), bytes).unwrap();
        let hash = file_hash(bytes);
        let store = crate::domain::settings::memory::MemorySettingsStore::new();
        let loaded = load(&dir, &root, ProjectTrust::StoredHash(Some(&hash)), &store).await;
        assert!(loaded.project_trusted);
        assert_eq!(loaded.servers.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_secret_counts_only_when_the_store_marks_it_secret() {
        use crate::domain::settings::store::SettingsStore;

        let dir = temp_home("secrets");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("secrets.toml"),
            "github_token = \"gh\"\nplain = \"visible\"\n",
        )
        .unwrap();
        let store = crate::domain::settings::memory::MemorySettingsStore::new();
        store.set("github_token", "gh".into(), true).await.unwrap();
        store.set("plain", "visible".into(), false).await.unwrap();
        let found = super::secrets(&dir, &store).await;
        assert_eq!(found.get("github_token").map(String::as_str), Some("gh"));
        assert!(!found.contains_key("plain"));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn temp_home(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("robi-mcp-config-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn http_to_a_public_host_is_rejected() {
        assert!(check_url("http://example.com/mcp").is_err());
        assert!(check_url("http://127.0.0.1:9/mcp").is_ok());
        assert!(check_url("https://mcp.linear.app/mcp").is_ok());
    }

    #[test]
    fn a_server_without_tools_keeps_every_tool() {
        let text = r#"{"mcpServers":{"linear":{"url":"https://mcp.linear.app/mcp"}}}"#;
        let (servers, errors) = parse(text, &secrets());
        assert!(errors.is_empty());
        assert!(!servers[0].tools.is_filtered());
    }

    #[test]
    fn a_tools_list_becomes_a_filter() {
        let text = r#"{"mcpServers":{"linear":{"url":"https://mcp.linear.app/mcp","tools":["get_*","re:^search_"]}}}"#;
        let (servers, errors) = parse(text, &secrets());
        assert!(errors.is_empty());
        assert!(servers[0].tools.is_filtered());
    }

    #[test]
    fn an_empty_tools_list_keeps_nothing() {
        let text = r#"{"mcpServers":{"linear":{"url":"https://mcp.linear.app/mcp","tools":[]}}}"#;
        let (servers, errors) = parse(text, &secrets());
        assert!(errors.is_empty());
        let listed = [ListedTool {
            name: "get_issue".into(),
            description: None,
            input_schema: None,
            read_only: true,
            destructive: false,
            open_world: false,
        }];
        assert!(servers[0].tools.visible("linear", &listed).0.is_empty());
    }

    #[test]
    fn a_null_tools_field_keeps_every_tool() {
        let text = r#"{"mcpServers":{"linear":{"url":"https://mcp.linear.app/mcp","tools":null}}}"#;
        let (servers, errors) = parse(text, &secrets());
        assert!(errors.is_empty());
        assert!(!servers[0].tools.is_filtered());
    }

    #[test]
    fn a_bad_tools_shape_skips_that_server() {
        for tools in [r#""get_*""#, r#"[1]"#, r#"["re:("]"#, r#"["*["]"#] {
            let text = format!(
                r#"{{"mcpServers":{{"linear":{{"url":"https://mcp.linear.app/mcp","tools":{tools}}}}}}}"#
            );
            let (servers, errors) = parse(&text, &secrets());
            assert!(servers.is_empty(), "accepted {tools}");
            assert_eq!(errors.len(), 1, "one error for {tools}");
        }
    }
}
