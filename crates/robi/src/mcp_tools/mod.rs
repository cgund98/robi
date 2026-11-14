//! `mcp-tools`: list one configured MCP server's tools.
//!
//! The diagnostic surface for a server entry. It reads the same `mcp.json`,
//! `secrets.toml`, and `path_entries` setting the app does, and connects through
//! the same opener, so what it prints is what a session would see.
//!
//! It opens no database. `prepare` resolves the files, `list` connects once,
//! prints nothing itself, and returns the lines so a test can read them.

pub mod args;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::adapters::settings::TomlSettingsStore;
use crate::agent::mcp::{
    connect_server, load, project_path, registered_name, user_path, ListedTool, ProjectTrust,
    ServerConfig, SessionOpener,
};
use crate::domain::settings::store::SettingsStore;

use args::Options;

/// Everything the run needs before it connects.
pub struct Context {
    pub options: Options,
    pub home: PathBuf,
    pub root: PathBuf,
    pub settings: Arc<dyn SettingsStore>,
}

/// What to print. `lines` goes to stdout, `note` to stderr.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub lines: Vec<String>,
    pub note: Option<String>,
}

/// Resolve the home directory, the settings store, and the workspace root.
pub fn prepare(options: Options) -> Result<Context, String> {
    let home = match &options.home {
        Some(home) => home.clone(),
        None => crate::adapters::settings::home_dir().map_err(|err| err.to_string())?,
    };
    let settings: Arc<dyn SettingsStore> =
        Arc::new(TomlSettingsStore::load(&home).map_err(|err| err.to_string())?);
    let root = match &options.workspace {
        Some(root) => {
            let root = root.clone();
            root.canonicalize().unwrap_or(root)
        }
        None => std::env::current_dir().map_err(|err| format!("no workspace: {err}"))?,
    };
    Ok(Context {
        options,
        home,
        root,
        settings,
    })
}

/// Connect to one server, read its tool list, and render the lines.
pub async fn list(context: &Context, opener: &dyn SessionOpener) -> Result<Report, String> {
    let trust = if context.options.trust {
        ProjectTrust::Always
    } else {
        ProjectTrust::StoredHash(None)
    };
    let loaded = load(
        &context.home,
        &context.root,
        trust,
        context.settings.as_ref(),
    )
    .await;
    let mut notes = Vec::new();
    if loaded.project_present && !loaded.project_trusted {
        notes.push(format!(
            "{} is not trusted; pass --trust to read it",
            project_note(&context.root)
        ));
    }
    for error in &loaded.errors {
        notes.push(format!("skipped {}: {}", error.id, error.message));
    }
    let ids: Vec<String> = loaded
        .servers
        .iter()
        .map(|server| server.id.clone())
        .collect();
    let Some(server) = loaded
        .servers
        .into_iter()
        .find(|server| server.id == context.options.server)
    else {
        return Err(with_notes(
            missing(&context.options.server, &context.home, &ids),
            &notes,
        ));
    };

    let (session, listed) = connect_server(opener, &server, &context.root)
        .await
        .map_err(|err| with_notes(err, &notes))?;
    session.close().await;

    let lines = render(&server, &listed.tools, &context.options, &mut notes);
    Ok(Report {
        lines,
        note: if notes.is_empty() {
            None
        } else {
            Some(notes.join("; "))
        },
    })
}

fn render(
    server: &ServerConfig,
    tools: &[ListedTool],
    options: &Options,
    notes: &mut Vec<String>,
) -> Vec<String> {
    let (kept, unused) = server.tools.visible(&server.id, tools);
    if server.tools.is_filtered() {
        let shown: Vec<&ListedTool> = if options.kept {
            kept
        } else {
            notes.push(format!(
                "tools filter keeps {} of {}; pass --kept to print only those",
                kept.len(),
                tools.len()
            ));
            tools.iter().collect()
        };
        if !unused.is_empty() {
            notes.push(format!("no tool matched: {}", unused.join(", ")));
        }
        return shown
            .into_iter()
            .map(|tool| one_line(server, tool, options))
            .collect();
    }
    tools
        .iter()
        .map(|tool| one_line(server, tool, options))
        .collect()
}

fn one_line(server: &ServerConfig, tool: &ListedTool, options: &Options) -> String {
    let name = if options.remote {
        tool.name.clone()
    } else {
        registered_name(&server.id, &tool.name)
    };
    if !options.long {
        return name;
    }
    let summary = tool
        .description
        .as_deref()
        .and_then(|text| text.lines().find(|line| !line.trim().is_empty()))
        .map(|line| line.trim().chars().take(120).collect::<String>())
        .unwrap_or_default();
    if summary.is_empty() {
        name
    } else {
        format!("{name}\t{summary}")
    }
}

/// A message with the run's notes after it, so a failure keeps the reason it
/// failed. An untrusted project file explains a missing id.
fn with_notes(message: String, notes: &[String]) -> String {
    if notes.is_empty() {
        message
    } else {
        format!("{message}; {}", notes.join("; "))
    }
}

fn project_note(root: &Path) -> String {
    project_path(root).display().to_string()
}

fn missing(id: &str, home: &Path, ids: &[String]) -> String {
    let path = user_path(home).display().to_string();
    if ids.is_empty() {
        return format!("no MCP server is configured in {path}");
    }
    format!(
        "no MCP server `{id}` in {path}; configured: {}",
        ids.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::mcp::{Listed, ListedTool, McpSession, OpenSpec};
    use async_trait::async_trait;
    use serde_json::Value;
    use tokio_util::sync::CancellationToken;

    struct Fake {
        tools: Vec<ListedTool>,
    }

    #[async_trait]
    impl McpSession for Fake {
        async fn list_tools(&self) -> Result<Listed, String> {
            Ok(Listed {
                tools: self.tools.clone(),
                instructions: None,
                title: None,
                icon: None,
            })
        }

        async fn call_tool(
            &self,
            _name: &str,
            _args: Value,
            _cancel: &CancellationToken,
        ) -> Result<Value, String> {
            Err("not called".into())
        }
    }

    struct Opens {
        tools: Vec<ListedTool>,
    }

    #[async_trait]
    impl SessionOpener for Opens {
        async fn open(&self, _spec: OpenSpec) -> Result<Box<dyn McpSession>, String> {
            Ok(Box::new(Fake {
                tools: self.tools.clone(),
            }))
        }
    }

    fn tool(name: &str, description: &str) -> ListedTool {
        ListedTool {
            name: name.into(),
            description: Some(description.into()),
            input_schema: None,
            read_only: true,
            destructive: false,
            open_world: false,
        }
    }

    fn tools() -> Vec<ListedTool> {
        vec![
            tool("keep_one", "the first\nsecond line"),
            tool("drop_two", "the second"),
        ]
    }

    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new(name: &str, entry: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("robi-mcp-tools-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("workspace")).unwrap();
            std::fs::write(
                dir.join("mcp.json"),
                format!(r#"{{"mcpServers":{{"srv":{entry}}}}}"#),
            )
            .unwrap();
            Self { dir }
        }

        fn context(&self, extra: &[&str]) -> Context {
            let mut argv = vec![
                "--home".to_string(),
                self.dir.display().to_string(),
                "--workspace".to_string(),
                self.dir.join("workspace").display().to_string(),
                "srv".to_string(),
            ];
            argv.extend(extra.iter().map(|flag| flag.to_string()));
            let args: Vec<std::ffi::OsString> = argv.into_iter().map(Into::into).collect();
            match args::parse(args) {
                args::Outcome::List(options) => prepare(options).unwrap(),
                other => panic!("expected a list, got {other:?}"),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn opener() -> Opens {
        Opens { tools: tools() }
    }

    #[tokio::test]
    async fn an_entry_without_tools_prints_every_registered_name() {
        let fixture = Fixture::new("all", r#"{"command":"fake"}"#);
        let report = list(&fixture.context(&[]), &opener()).await.unwrap();
        assert_eq!(
            report.lines,
            vec![
                "mcp_srv_keep_one".to_string(),
                "mcp_srv_drop_two".to_string()
            ]
        );
        assert_eq!(report.note, None);
    }

    #[tokio::test]
    async fn remote_prints_the_servers_own_names() {
        let fixture = Fixture::new("remote", r#"{"command":"fake"}"#);
        let report = list(&fixture.context(&["--remote"]), &opener())
            .await
            .unwrap();
        assert_eq!(
            report.lines,
            vec!["keep_one".to_string(), "drop_two".to_string()]
        );
    }

    #[tokio::test]
    async fn kept_prints_only_what_the_filter_keeps() {
        let fixture = Fixture::new("kept", r#"{"command":"fake","tools":["keep_*"]}"#);
        let report = list(&fixture.context(&["--kept"]), &opener())
            .await
            .unwrap();
        assert_eq!(report.lines, vec!["mcp_srv_keep_one".to_string()]);
        assert_eq!(report.note, None);
    }

    #[tokio::test]
    async fn without_kept_a_filtered_entry_prints_all_and_explains() {
        let fixture = Fixture::new("filtered", r#"{"command":"fake","tools":["keep_*"]}"#);
        let report = list(&fixture.context(&[]), &opener()).await.unwrap();
        assert_eq!(report.lines.len(), 2);
        let note = report.note.expect("a note about the filter");
        assert!(note.contains("1 of 2"), "{note}");
        assert!(note.contains("--kept"), "{note}");
    }

    #[tokio::test]
    async fn a_pattern_that_matches_nothing_is_named() {
        let fixture = Fixture::new("unused", r#"{"command":"fake","tools":["keep_*","nope"]}"#);
        let report = list(&fixture.context(&["--kept"]), &opener())
            .await
            .unwrap();
        assert_eq!(report.lines, vec!["mcp_srv_keep_one".to_string()]);
        let note = report.note.expect("the unused pattern");
        assert!(note.contains("nope"), "{note}");
    }

    #[tokio::test]
    async fn long_keeps_one_line_per_tool() {
        let fixture = Fixture::new("long", r#"{"command":"fake"}"#);
        let report = list(&fixture.context(&["--long"]), &opener())
            .await
            .unwrap();
        assert_eq!(report.lines.len(), 2);
        assert_eq!(report.lines[0], "mcp_srv_keep_one\tthe first");
    }

    #[tokio::test]
    async fn an_unknown_id_names_the_configured_ones() {
        let fixture = Fixture::new("missing", r#"{"command":"fake"}"#);
        let mut context = fixture.context(&[]);
        context.options.server = "nope".into();
        let err = list(&context, &opener()).await.unwrap_err();
        assert!(err.contains("nope"), "{err}");
        assert!(err.contains("srv"), "{err}");
    }

    #[tokio::test]
    async fn an_untrusted_project_file_is_named_and_ignored() {
        let fixture = Fixture::new("project", r#"{"command":"fake"}"#);
        let project_dir = fixture.dir.join("workspace").join(".robi");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("mcp.json"),
            r#"{"mcpServers":{"extra":{"command":"fake"}}}"#,
        )
        .unwrap();
        let report = list(&fixture.context(&[]), &opener()).await.unwrap();
        let note = report.note.expect("the untrusted note");
        assert!(note.contains("--trust"), "{note}");

        // The same file answers for an id only it defines, and the failure says
        // why: the file is there and untrusted.
        let mut context = fixture.context(&[]);
        context.options.server = "extra".into();
        let err = list(&context, &opener()).await.unwrap_err();
        assert!(err.contains("extra"), "{err}");
        assert!(err.contains("--trust"), "{err}");
    }

    #[tokio::test]
    async fn trust_reads_the_project_file() {
        let fixture = Fixture::new("trusted", r#"{"command":"fake"}"#);
        let project_dir = fixture.dir.join("workspace").join(".robi");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("mcp.json"),
            r#"{"mcpServers":{"extra":{"command":"fake"}}}"#,
        )
        .unwrap();
        let mut context = fixture.context(&["--trust"]);
        context.options.server = "extra".into();
        let report = list(&context, &opener()).await.unwrap();
        assert_eq!(report.lines.len(), 2);
        assert_eq!(report.note, None);
    }
}
