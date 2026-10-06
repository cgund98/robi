//! Workspace tools.
//!
//! Each tool closes over one chat session. It reloads that session's path
//! rules at the start of every call.

pub(crate) mod change;
mod context;
mod delegate;
mod delete_file;
mod edit_file;
mod find;
mod grant;
mod grep;
mod list_dir;
mod lsp;
mod marker;
pub(crate) mod memory_store;
pub(crate) mod plan_file;
mod read_code;
mod read_file;
mod replace;
mod retrieve;
mod semantic_search;
mod shell;
mod skill;
mod subagent;
mod todos;
mod web_fetch;
mod web_search;
mod write_file;
mod write_plan;

#[cfg(test)]
mod apply_tests;

use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::{RegistryError, ToolError};
use robi_core::tool::{Concurrency, Tool, ToolRegistry, ToolRun};
use serde_json::Value;

use crate::agent::web::{HttpFetcher, SearchEngine};
use crate::domain::chat_session::model::AgentMode;

pub use context::ToolContext;
pub use delegate::{ChildModels, Delegate, SessionChildModels, UnavailableChildModels};

/// Register `read_file`, `list_dir`, `find`, `grep`, and `grant`.
pub fn register_read_tools(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
) -> Result<(), RegistryError> {
    register(
        registry,
        Arc::new(read_file::ReadFile::new(Arc::clone(&ctx))),
    )?;
    register(
        registry,
        Arc::new(read_code::ReadCode::new(Arc::clone(&ctx))),
    )?;
    register(registry, Arc::new(list_dir::ListDir::new(Arc::clone(&ctx))))?;
    register(registry, Arc::new(find::Find::new(Arc::clone(&ctx))))?;
    register(registry, Arc::new(grep::Grep::new(Arc::clone(&ctx))))?;
    register(
        registry,
        Arc::new(semantic_search::SemanticSearch::new(Arc::clone(&ctx))),
    )?;
    register(registry, Arc::new(grant::Grant::new(Arc::clone(&ctx))))?;
    if ctx.lsp_enabled {
        register(registry, Arc::new(lsp::Diagnostics::new(Arc::clone(&ctx))))?;
        register(registry, Arc::new(lsp::Definition::new(Arc::clone(&ctx))))?;
        register(registry, Arc::new(lsp::References::new(Arc::clone(&ctx))))?;
        register(registry, Arc::new(lsp::Hover::new(Arc::clone(&ctx))))?;
        register(registry, Arc::new(lsp::WorkspaceSymbol::new(ctx)))?;
    }
    Ok(())
}

/// Register `write_file`, `edit_file`, and `delete_file`.
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

/// Register `shell`.
pub fn register_shell_tool(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
) -> Result<(), RegistryError> {
    register(registry, Arc::new(shell::Shell::new(Arc::clone(&ctx))))?;
    register(registry, Arc::new(retrieve::Retrieve::new(ctx)))?;
    Ok(())
}

/// Register `write_plan`.
///
/// `create` is true in plan mode, which may mint a new file. Agent mode passes
/// false and only overwrites a plan that already exists.
pub fn register_plan_tool(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
    create: bool,
) -> Result<(), RegistryError> {
    register(registry, Arc::new(write_plan::WritePlan::new(ctx, create)))?;
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
    match mode {
        AgentMode::Ask => {}
        AgentMode::Plan => {
            register_shell_tool(registry, Arc::clone(&ctx))?;
            register_plan_tool(registry, ctx, true)?;
        }
        AgentMode::Agent => {
            register_edit_tools(registry, Arc::clone(&ctx))?;
            register_shell_tool(registry, Arc::clone(&ctx))?;
            register_plan_tool(registry, Arc::clone(&ctx), false)?;
            register(registry, Arc::new(todos::Todos::new(Arc::clone(&ctx))))?;
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

    #[tokio::test]
    async fn each_mode_registers_its_own_tools() {
        let harness = apply_tests::harness().await;
        let ask = names(AgentMode::Ask, &harness).await;
        let plan = names(AgentMode::Plan, &harness).await;
        let agent = names(AgentMode::Agent, &harness).await;
        assert_eq!(
            ask,
            vec![
                "definition",
                "diagnostics",
                "find",
                "grant",
                "grep",
                "hover",
                "list_dir",
                "read_code",
                "read_file",
                "references",
                "semantic_search",
                "skill",
                "web_fetch",
                "web_search",
                "workspace_symbol"
            ]
        );
        assert_eq!(
            plan,
            vec![
                "definition",
                "diagnostics",
                "find",
                "grant",
                "grep",
                "hover",
                "list_dir",
                "read_code",
                "read_file",
                "references",
                "retrieve",
                "semantic_search",
                "shell",
                "skill",
                "web_fetch",
                "web_search",
                "workspace_symbol",
                "write_plan"
            ]
        );
        assert_eq!(
            agent,
            vec![
                "definition",
                "delegate",
                "delete_file",
                "diagnostics",
                "edit_file",
                "find",
                "grant",
                "grep",
                "hover",
                "list_dir",
                "read_code",
                "read_file",
                "references",
                "retrieve",
                "semantic_search",
                "shell",
                "skill",
                "todos",
                "web_fetch",
                "web_search",
                "workspace_symbol",
                "write_file",
                "write_plan"
            ]
        );
    }

    async fn names(mode: AgentMode, harness: &apply_tests::Harness) -> Vec<String> {
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
    async fn lsp_off_omits_the_language_server_tools() {
        let harness = apply_tests::harness().await;
        let ctx = Arc::new(ToolContext {
            session_id: harness.ctx.session_id,
            workspace_id: harness.ctx.workspace_id,
            root: harness.ctx.root.clone(),
            sessions: Arc::clone(&harness.ctx.sessions),
            file_changes: Arc::clone(&harness.ctx.file_changes),
            index: None,
            lsp: crate::agent::lsp::LspHub::new(),
            lsp_enabled: false,
            originals: None,
            settings: None,
            events: None,
        });
        let registry = ToolRegistry::new();
        register_tools_for_mode(
            &registry,
            Arc::clone(&ctx),
            AgentMode::Agent,
            Arc::new(UnavailableChildModels),
            Arc::new(IdleSearch),
            true,
            true,
        )
        .unwrap();
        let names = registry.names();
        for tool in [
            "diagnostics",
            "definition",
            "references",
            "hover",
            "workspace_symbol",
        ] {
            assert!(!names.iter().any(|name| name == tool), "{names:?}");
        }
        assert!(names.iter().any(|name| name == "read_file"));

        let child = subagent::child_tool_names(robi_core::message::SubagentMode::Explore, ctx);
        assert!(!child.iter().any(|name| name == "diagnostics"));
        assert!(child.iter().any(|name| name == "grep"));
    }

    #[tokio::test]
    async fn an_edit_tool_publishes_a_file_changed_frame() {
        use crate::domain::events::{EventBus, FILE_CHANGED};
        use robi_core::tool::Tool;
        use serde_json::json;

        let harness = apply_tests::harness().await;
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
            index: None,
            lsp: crate::agent::lsp::LspHub::new(),
            lsp_enabled: false,
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
