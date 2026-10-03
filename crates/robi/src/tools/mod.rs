//! Workspace tools.
//!
//! Each tool closes over one chat session. It reloads that session's path
//! rules at the start of every call.

mod change;
mod context;
mod delegate;
mod delete_file;
mod edit_file;
mod find;
mod grant;
mod grep;
mod list_dir;
mod memory_store;
pub(crate) mod plan_file;
mod read_file;
mod replace;
mod semantic_search;
mod shell;
mod subagent;
mod todos;
mod web_fetch;
mod web_search;
mod write_file;
mod write_plan;

#[cfg(test)]
mod apply_tests;

use std::sync::Arc;

use robi_core::error::RegistryError;
use robi_core::tool::ToolRegistry;

use crate::domain::chat_session::model::AgentMode;
use crate::web::{HttpFetcher, SearchEngine};

pub use context::ToolContext;
pub use delegate::{ChildModels, Delegate, SessionChildModels, UnavailableChildModels};

/// Register `read_file`, `list_dir`, `find`, `grep`, and `grant`.
pub fn register_read_tools(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
) -> Result<(), RegistryError> {
    registry.register(Arc::new(read_file::ReadFile::new(Arc::clone(&ctx))))?;
    registry.register(Arc::new(list_dir::ListDir::new(Arc::clone(&ctx))))?;
    registry.register(Arc::new(find::Find::new(Arc::clone(&ctx))))?;
    registry.register(Arc::new(grep::Grep::new(Arc::clone(&ctx))))?;
    registry.register(Arc::new(semantic_search::SemanticSearch::new(Arc::clone(
        &ctx,
    ))))?;
    registry.register(Arc::new(grant::Grant::new(ctx)))?;
    Ok(())
}

/// Register `write_file`, `edit_file`, and `delete_file`.
pub fn register_edit_tools(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
) -> Result<(), RegistryError> {
    registry.register(Arc::new(write_file::WriteFile::new(Arc::clone(&ctx))))?;
    registry.register(Arc::new(edit_file::EditFile::new(Arc::clone(&ctx))))?;
    registry.register(Arc::new(delete_file::DeleteFile::new(ctx)))?;
    Ok(())
}

/// Register `shell`.
pub fn register_shell_tool(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
) -> Result<(), RegistryError> {
    registry.register(Arc::new(shell::Shell::new(ctx)))?;
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
    registry.register(Arc::new(write_plan::WritePlan::new(ctx, create)))?;
    Ok(())
}

/// Register `web_search` and `web_fetch`.
pub fn register_web_tools(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
    search: Arc<dyn SearchEngine>,
) -> Result<(), RegistryError> {
    registry.register(Arc::new(web_search::WebSearch::new(search)))?;
    registry.register(Arc::new(web_fetch::WebFetch::new(
        ctx,
        Arc::new(HttpFetcher),
    )))?;
    Ok(())
}

/// Register the tools one mode is allowed to call.
pub fn register_tools_for_mode(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
    mode: AgentMode,
    models: Arc<dyn ChildModels>,
    search: Arc<dyn SearchEngine>,
) -> Result<(), RegistryError> {
    register_read_tools(registry, Arc::clone(&ctx))?;
    register_web_tools(registry, Arc::clone(&ctx), search)?;
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
            registry.register(Arc::new(todos::Todos::new(Arc::clone(&ctx))))?;
            registry.register(Arc::new(Delegate::new(ctx, models)))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod registry_tests {
    use async_trait::async_trait;

    use super::*;
    use crate::web::{SearchError, SearchHit};

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
                "find",
                "grant",
                "grep",
                "list_dir",
                "read_file",
                "semantic_search",
                "web_fetch",
                "web_search"
            ]
        );
        assert_eq!(
            plan,
            vec![
                "find",
                "grant",
                "grep",
                "list_dir",
                "read_file",
                "semantic_search",
                "shell",
                "web_fetch",
                "web_search",
                "write_plan"
            ]
        );
        assert_eq!(
            agent,
            vec![
                "delegate",
                "delete_file",
                "edit_file",
                "find",
                "grant",
                "grep",
                "list_dir",
                "read_file",
                "semantic_search",
                "shell",
                "todos",
                "web_fetch",
                "web_search",
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
        )
        .unwrap();
        registry
            .tools()
            .into_iter()
            .map(|tool| tool.name().to_owned())
            .collect()
    }
}
