//! The tool trait and the registry that holds tools.

use std::collections::BTreeMap;
use std::sync::{Arc, PoisonError, RwLock};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::error::{RegistryError, ToolError};
use crate::message::SubagentSnapshot;

/// Whether the loop may run a tool alongside another in the same turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Concurrency {
    /// Safe to overlap. The default, because reads dominate a coding agent's
    /// tool set.
    Concurrent,
    /// Nothing else in the turn is in flight at the same time as this call.
    Exclusive,
}

impl Concurrency {
    pub fn is_exclusive(self) -> bool {
        matches!(self, Concurrency::Exclusive)
    }
}

/// Whether a call needs a decision before it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    AllowImmediately,
    NeedsApproval,
}

/// What the loop hands a tool for one call.
pub struct ToolRun {
    pub cancel: CancellationToken,
    /// Writes the child snapshot onto this call while `execute` is still running.
    pub report: Arc<dyn ToolReporter>,
}

impl ToolRun {
    /// A run that records no child progress. Tests and tools that do not delegate
    /// use this.
    pub fn new(cancel: CancellationToken) -> Self {
        Self {
            cancel,
            report: Arc::new(NopReporter),
        }
    }
}

/// Progress a tool can publish before it returns.
///
/// The loop's implementation writes the snapshot onto the parent call and emits
/// `ToolCallUpdated`. A tool that is not a subagent ignores it.
#[async_trait]
pub trait ToolReporter: Send + Sync {
    async fn subagent(&self, snapshot: SubagentSnapshot);
}

/// Drops every snapshot.
#[derive(Debug, Default, Clone, Copy)]
pub struct NopReporter;

#[async_trait]
impl ToolReporter for NopReporter {
    async fn subagent(&self, _snapshot: SubagentSnapshot) {}
}

/// One capability the model can call.
///
/// The description and `parameters` schema are the model's only guide to using
/// the tool, so they are product copy rather than comments.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;

    fn description(&self) -> &str;

    /// A JSON Schema object describing the arguments.
    fn parameters(&self) -> serde_json::Value;

    /// Whether the loop may overlap this tool with another.
    ///
    /// Side-effect free. `Concurrent` is a claim that `execute` tolerates being
    /// called from several tasks at once, which `Send + Sync` alone does not
    /// establish.
    fn concurrency(&self) -> Concurrency {
        Concurrency::Concurrent
    }

    /// Whether this call needs the user's decision.
    ///
    /// Side-effect free, decided from this call's arguments alone. That property
    /// is what lets the loop settle a whole turn's approvals before running any
    /// of it.
    async fn requires_approval(&self, args: &serde_json::Value) -> ApprovalDecision;

    /// Run the call.
    ///
    /// Must be safe to call concurrently when `concurrency` returns
    /// `Concurrency::Concurrent`: the loop may call it from several tasks.
    async fn execute(
        &self,
        args: serde_json::Value,
        run: ToolRun,
    ) -> Result<serde_json::Value, ToolError>;
}

/// The tools a turn can call, keyed by name.
///
/// Held behind a shared handle so a tool registered after the agent is built is
/// still visible to it.
#[derive(Default)]
pub struct ToolRegistry {
    tools: RwLock<BTreeMap<String, Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a tool. A duplicate name is refused rather than replacing.
    pub fn register(&self, tool: Arc<dyn Tool>) -> Result<(), RegistryError> {
        let name = tool.name().to_owned();
        let mut tools = self.write();
        if tools.contains_key(&name) {
            return Err(RegistryError::DuplicateName(name));
        }
        tools.insert(name, tool);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.read().get(name).cloned()
    }

    /// Every tool, sorted by name, so prompt assembly is byte-stable across runs.
    pub fn tools(&self) -> Vec<Arc<dyn Tool>> {
        self.read().values().cloned().collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.read().keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether this unfinished call is waiting on a person.
    ///
    /// `Pending` is the status a call is born with, including one the tool would
    /// run immediately. The turn pauses only when this returns true.
    pub async fn awaits_user_decision(&self, call: &crate::message::ToolCall) -> bool {
        if !call.is_pending_approval() || !call.needs_execution() || call.args_error.is_some() {
            return false;
        }
        let Some(tool) = self.get(&call.name) else {
            return false;
        };
        tool.requires_approval(&call.args).await == ApprovalDecision::NeedsApproval
    }

    /// Every tool's declared concurrency, for the registry snapshot test.
    ///
    /// The test asserts this map exactly, so adding a tool fails until its author
    /// states a setting. `Concurrent` being the default is what makes the setting
    /// easy to omit, and this is what keeps an omission visible.
    pub fn concurrency_snapshot(&self) -> BTreeMap<String, Concurrency> {
        self.read()
            .iter()
            .map(|(name, tool)| (name.clone(), tool.concurrency()))
            .collect()
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, BTreeMap<String, Arc<dyn Tool>>> {
        self.tools.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, BTreeMap<String, Arc<dyn Tool>>> {
        self.tools.write().unwrap_or_else(PoisonError::into_inner)
    }
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("tools", &self.names())
            .finish()
    }
}
