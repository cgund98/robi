//! Workspace tools.
//!
//! Each tool closes over one chat session. It reloads that session's path
//! rules at the start of every call.

mod args;
#[cfg(feature = "edit-tools")]
pub(crate) mod change;
mod context;
mod delegate;
#[cfg(feature = "edit-tools")]
mod delete_file;
#[cfg(feature = "edit-tools")]
mod edit_file;
#[cfg(feature = "read-tools")]
mod find;
/// `--features edit-tools` only. The docs editor uses the same primitives.
#[cfg(any(feature = "edit-tools", feature = "docs-viewer"))]
pub(crate) mod fs_write;
mod grant;
#[cfg(feature = "read-tools")]
mod grep;
#[cfg(feature = "read-tools")]
mod list_dir;
#[cfg(feature = "edit-tools")]
mod marker;
pub(crate) mod memory_store;
/// One writer at a time per path. The edit tools, the docs editor, and the
/// review baseline reader use it.
#[cfg(any(feature = "edit-tools", feature = "docs-viewer", feature = "review"))]
pub(crate) mod path_lock;
#[cfg(feature = "read-tools")]
mod read_file;
#[cfg(feature = "edit-tools")]
mod replace;
mod retrieve;
pub(crate) mod shell;
mod skill;
mod subagent;
mod web_fetch;
mod web_search;
#[cfg(feature = "edit-tools")]
mod write_file;

#[cfg(all(test, feature = "edit-tools"))]
mod apply_tests;
#[cfg(test)]
mod test_harness;

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::{RegistryError, ToolError};
use robi_core::tool::{Concurrency, Tool, ToolRegistry, ToolRun};
use serde_json::Value;

use crate::agent::web::{HttpFetcher, SearchEngine};
use crate::domain::chat_session::model::AgentMode;

pub use context::ToolContext;
pub use delegate::{ChildModels, Delegate, SessionChildModels, UnavailableChildModels};
pub use shell::ShellAccess;

/// Register `grant`, plus whichever read tools this build has.
///
/// The default build registers `grant` alone: the shell is the read interface.
/// `--features read-tools` adds `read_file`, `list_dir`, `find` and `grep`.
pub fn register_read_tools(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
) -> Result<(), RegistryError> {
    #[cfg(feature = "read-tools")]
    register(
        registry,
        Arc::new(read_file::ReadFile::new(Arc::clone(&ctx))),
    )?;
    #[cfg(feature = "read-tools")]
    register(registry, Arc::new(list_dir::ListDir::new(Arc::clone(&ctx))))?;
    #[cfg(feature = "read-tools")]
    register(registry, Arc::new(find::Find::new(Arc::clone(&ctx))))?;
    #[cfg(feature = "read-tools")]
    register(registry, Arc::new(grep::Grep::new(Arc::clone(&ctx))))?;
    register(registry, Arc::new(grant::Grant::new(Arc::clone(&ctx))))?;
    Ok(())
}

/// Register `write_file`, `edit_file`, and `delete_file`.
///
/// `--features edit-tools` only. The default build edits through the shell.
#[cfg(feature = "edit-tools")]
pub fn register_edit_tools(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
) -> Result<(), RegistryError> {
    register(
        registry,
        Arc::new(write_file::WriteFile::new(Arc::clone(&ctx))),
    )?;
    register(
        registry,
        Arc::new(edit_file::EditFile::new(Arc::clone(&ctx))),
    )?;
    register(registry, Arc::new(delete_file::DeleteFile::new(ctx)))?;
    Ok(())
}

/// Register `shell` and `retrieve`.
///
/// `access` is the mode's policy, not the sandbox's: a read-only shell runs
/// inside a profile that cannot write the workspace, whatever the arguments say.
pub fn register_shell_tool(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
    access: shell::ShellAccess,
) -> Result<(), RegistryError> {
    register(
        registry,
        Arc::new(shell::Shell::new(Arc::clone(&ctx), access)),
    )?;
    register(registry, Arc::new(retrieve::Retrieve::new(ctx)))?;
    Ok(())
}

/// Register `web_search` and `web_fetch`.
pub fn register_web_tools(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
    search: Arc<dyn SearchEngine>,
    search_approval: bool,
    fetch_approval: bool,
) -> Result<(), RegistryError> {
    register(
        registry,
        Arc::new(web_search::WebSearch::new(search, search_approval)),
    )?;
    register(
        registry,
        Arc::new(web_fetch::WebFetch::new(
            ctx,
            Arc::new(HttpFetcher),
            fetch_approval,
        )),
    )?;
    Ok(())
}

/// Register the tools one mode is allowed to call.
///
/// Ask and Plan get a read-only shell: the mode policy lives in the tool
/// instance, not in which tools were registered. See `ShellAccess`.
pub fn register_tools_for_mode(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
    mode: AgentMode,
    models: Arc<dyn ChildModels>,
    search: Arc<dyn SearchEngine>,
    search_approval: bool,
    fetch_approval: bool,
) -> Result<(), RegistryError> {
    register_read_tools(registry, Arc::clone(&ctx))?;
    register(registry, Arc::new(skill::Skill::new(Arc::clone(&ctx))))?;
    register_web_tools(
        registry,
        Arc::clone(&ctx),
        search,
        search_approval,
        fetch_approval,
    )?;
    let access = match mode {
        AgentMode::Ask => ShellAccess::ReadOnly,
        AgentMode::Plan => ShellAccess::PlanReadOnly,
        AgentMode::Agent => ShellAccess::ReadWrite,
    };
    register_shell_tool(registry, Arc::clone(&ctx), access)?;
    match mode {
        AgentMode::Ask => {}
        AgentMode::Plan => {}
        AgentMode::Agent => {
            #[cfg(feature = "edit-tools")]
            register_edit_tools(registry, Arc::clone(&ctx))?;
            register(registry, Arc::new(Delegate::new(ctx, models)))?;
        }
    }
    Ok(())
}

fn register(registry: &ToolRegistry, tool: Arc<dyn Tool>) -> Result<(), RegistryError> {
    registry.register(Arc::new(LoggingTool { inner: tool }))
}

/// Logs the start and the result of one call. Arguments stay out of the log.
struct LoggingTool {
    inner: Arc<dyn Tool>,
}

#[async_trait]
impl Tool for LoggingTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> Value {
        self.inner.parameters()
    }

    fn concurrency(&self) -> Concurrency {
        self.inner.concurrency()
    }

    async fn requires_approval(&self, args: &Value) -> robi_core::tool::ApprovalDecision {
        self.inner.requires_approval(args).await
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        let tool = self.inner.name();
        tracing::info!(tool, "tool started");
        let result = self.inner.execute(args, run).await;
        log_tool_result(tool, &result);
        result
    }
}

fn log_tool_result(tool: &str, result: &Result<Value, ToolError>) {
    match result {
        Ok(_) => tracing::info!(tool, "tool finished"),
        Err(ToolError::Cancelled) => tracing::info!(tool, "tool cancelled"),
        Err(ToolError::Panicked) => tracing::error!(tool, "tool panicked"),
        Err(err) => tracing::warn!(tool, %err, "tool failed"),
    }
}

#[cfg(test)]
mod registry_tests {
    use async_trait::async_trait;

    use super::*;
    use crate::agent::web::{SearchError, SearchHit};

    struct IdleSearch;

    #[async_trait]
    impl SearchEngine for IdleSearch {
        async fn search(&self, _query: &str) -> Result<Vec<SearchHit>, SearchError> {
            Err(SearchError("search is not configured".into()))
        }
    }

    /// The names this build registers, sorted.
    ///
    /// Composed from `cfg!` rather than hardcoded: the same test then covers the
    /// default build and `--all-features`, which is the only place a wrong
    /// feature gate shows up.
    fn expected(mode: AgentMode) -> Vec<&'static str> {
        let mut names = vec![
            "grant",
            "shell",
            "skill",
            "web_fetch",
            "web_search",
            "retrieve",
        ];
        if cfg!(feature = "read-tools") {
            names.extend(["find", "grep", "list_dir", "read_file"]);
        }
        match mode {
            AgentMode::Ask => {}
            AgentMode::Plan => {}
            AgentMode::Agent => {
                names.push("delegate");
                if cfg!(feature = "edit-tools") {
                    names.extend(["delete_file", "edit_file", "write_file"]);
                }
            }
        }
        names.sort_unstable();
        names.dedup();
        names
    }

    #[tokio::test]
    async fn each_mode_registers_its_own_tools() {
        let harness = test_harness::harness().await;
        for mode in [AgentMode::Ask, AgentMode::Plan, AgentMode::Agent] {
            let mut got = names(mode, &harness).await;
            got.sort_unstable();
            assert_eq!(got, expected(mode), "{mode:?}");
        }
    }

    /// S9: the plan is a workspace file the shell writes, so no mode offers a
    /// plan or todo tool.
    #[tokio::test]
    async fn no_mode_offers_a_plan_tool() {
        let harness = test_harness::harness().await;
        for mode in [AgentMode::Ask, AgentMode::Plan, AgentMode::Agent] {
            let registered = names(mode, &harness).await;
            for forbidden in ["write_plan", "todos"] {
                assert!(
                    !registered.iter().any(|tool| tool == forbidden),
                    "{forbidden} in {mode:?}"
                );
            }
        }
    }

    async fn names(mode: AgentMode, harness: &test_harness::Harness) -> Vec<String> {
        let registry = ToolRegistry::new();
        register_tools_for_mode(
            &registry,
            std::sync::Arc::clone(&harness.ctx),
            mode,
            std::sync::Arc::new(UnavailableChildModels),
            std::sync::Arc::new(IdleSearch),
            true,
            true,
        )
        .unwrap();
        registry
            .tools()
            .into_iter()
            .map(|tool| tool.name().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn no_mode_offers_an_escalation_key_it_cannot_honour() {
        let harness = test_harness::harness().await;
        for mode in [AgentMode::Ask, AgentMode::Plan] {
            let registry = ToolRegistry::new();
            register_tools_for_mode(
                &registry,
                std::sync::Arc::clone(&harness.ctx),
                mode,
                std::sync::Arc::new(UnavailableChildModels),
                std::sync::Arc::new(IdleSearch),
                true,
                true,
            )
            .unwrap();
            let shell = registry
                .tools()
                .into_iter()
                .find(|tool| tool.name() == "shell")
                .expect("every mode has a shell");
            let properties = &shell.parameters()["properties"];
            assert!(properties.get("write_paths").is_none(), "{mode:?}");
            assert!(properties.get("unsandboxed").is_none(), "{mode:?}");
        }
    }

    #[cfg(feature = "edit-tools")]
    #[tokio::test]
    async fn an_edit_tool_publishes_a_file_changed_frame() {
        use crate::domain::events::{EventBus, FILE_CHANGED};
        use robi_core::tool::Tool;
        use serde_json::json;

        let harness = test_harness::harness().await;
        std::fs::write(harness.root.join("note.md"), "one\n").unwrap();
        let session = harness.ctx.session_id;
        let workspace = harness.ctx.workspace_id;
        let bus = Arc::new(EventBus::new());
        let mut subscription = bus.subscribe();
        let ctx = Arc::new(ToolContext {
            session_id: session,
            workspace_id: workspace,
            root: harness.ctx.root.clone(),
            sessions: Arc::clone(&harness.ctx.sessions),
            file_changes: Arc::clone(&harness.ctx.file_changes),
            originals: None,
            settings: None,
            events: Some(Arc::clone(&bus)),
        });
        super::write_file::WriteFile::new(ctx)
            .execute(
                json!({ "path": "note.md", "content": "two\n" }),
                robi_core::tool::ToolRun::new(tokio_util::sync::CancellationToken::new()),
            )
            .await
            .unwrap();

        let envelope = tokio::time::timeout(std::time::Duration::from_secs(1), subscription.recv())
            .await
            .expect("a frame")
            .expect("a frame");
        assert_eq!(envelope.event_type, FILE_CHANGED);
        assert_eq!(envelope.subject, workspace.to_string());
        assert_eq!(envelope.data["path"], "note.md");
        assert_eq!(envelope.data["source"], "agent");
        assert_eq!(envelope.data["session_id"], session.to_string());
    }
}
