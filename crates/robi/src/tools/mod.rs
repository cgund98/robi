//! Read-only workspace tools.
//!
//! Each tool closes over one chat session. It reloads that session's path
//! rules at the start of every call.

mod context;
mod find;
mod grant;
mod grep;
mod list_dir;
mod read_file;

use std::sync::Arc;

use robi_core::error::RegistryError;
use robi_core::tool::ToolRegistry;

pub use context::ToolContext;

/// Register `read_file`, `list_dir`, `find`, `grep`, and `grant`.
pub fn register_read_tools(
    registry: &ToolRegistry,
    ctx: Arc<ToolContext>,
) -> Result<(), RegistryError> {
    registry.register(Arc::new(read_file::ReadFile::new(Arc::clone(&ctx))))?;
    registry.register(Arc::new(list_dir::ListDir::new(Arc::clone(&ctx))))?;
    registry.register(Arc::new(find::Find::new(Arc::clone(&ctx))))?;
    registry.register(Arc::new(grep::Grep::new(Arc::clone(&ctx))))?;
    registry.register(Arc::new(grant::Grant::new(ctx)))?;
    Ok(())
}
