//! System prompt sources.
//!
//! [`PromptAssembler`] concatenates sources from general to specific. The
//! built-in block is code. A user setting, `~/.robi/system.md`,
//! `~/.robi/AGENTS.md`, and the project `AGENTS.md` chain are each a source.
//! A missing file contributes nothing.

mod builtin;
mod files;
mod mode;

use std::path::PathBuf;
use std::sync::Arc;

use robi_core::prompt::{render, PromptBlock};
use robi_core::tool::ToolRegistry;

use crate::domain::chat_session::model::AgentMode;
use crate::tools::plan_file::todos_prompt;

use mode::ModePrefix;

pub use builtin::Builtin;
pub use files::{InstructionFile, ProjectAgents, WorkingDirectory};

/// Default cap for one instruction file. The tail is kept.
pub const DEFAULT_MAX_BYTES: usize = 32 * 1024;

/// One contributor to the system prompt.
///
/// `load` returns `None` when the source has nothing to say. A missing file
/// is that case. A source does not fail the turn because a file could not
/// be read.
pub trait PromptSource {
    fn load(&self) -> Option<PromptBlock>;
}

/// Ordered sources. Later blocks are more specific.
pub struct PromptAssembler {
    sources: Vec<Box<dyn PromptSource + Send + Sync>>,
}

impl PromptAssembler {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }

    pub fn source(mut self, source: impl PromptSource + Send + Sync + 'static) -> Self {
        self.sources.push(Box::new(source));
        self
    }

    pub fn render(&self) -> String {
        let blocks: Vec<PromptBlock> = self
            .sources
            .iter()
            .filter_map(|source| source.load())
            .collect();
        render(&blocks)
    }
}

impl Default for PromptAssembler {
    fn default() -> Self {
        Self::new()
    }
}

/// Text supplied by configuration, not by a file.
pub struct UserPrompt {
    pub text: String,
    pub max_bytes: usize,
}

impl PromptSource for UserPrompt {
    fn load(&self) -> Option<PromptBlock> {
        let text = self.text.trim();
        if text.is_empty() {
            return None;
        }
        Some(PromptBlock {
            tag: Some("user_prompt"),
            body: robi_core::prompt::keep_tail(text, self.max_bytes),
        })
    }
}

/// What one session actor needs to build its default prompt.
pub struct SessionPrompt<'a> {
    pub tools: &'a ToolRegistry,
    pub user_prompt: Option<String>,
    /// `~/.robi`. Global `system.md` and `AGENTS.md` are read from here.
    pub config_dir: Option<PathBuf>,
    pub workspace: Option<PathBuf>,
    pub mode: AgentMode,
    /// Workspace-relative plan whose checklist is re-read for an agent turn.
    pub plan_path: Option<String>,
    pub max_bytes: usize,
}

/// Built-in prompt, then the user setting, then global files, then the
/// project chain, then the working directory.
pub fn assemble_session(input: SessionPrompt<'_>) -> String {
    let max_bytes = if input.max_bytes == 0 {
        DEFAULT_MAX_BYTES
    } else {
        input.max_bytes
    };
    let mut assembler = PromptAssembler::new().source(Builtin {
        tools: Arc::new(registry_snapshot(input.tools)),
    });
    if let Some(text) = input.user_prompt {
        assembler = assembler.source(UserPrompt { text, max_bytes });
    }
    if let Some(dir) = input.config_dir {
        assembler = assembler
            .source(InstructionFile {
                path: dir.join("system.md"),
                tag: "user_prompt",
                max_bytes,
            })
            .source(InstructionFile {
                path: dir.join("AGENTS.md"),
                tag: "user_agents",
                max_bytes,
            });
    }
    if let Some(workspace) = input.workspace.clone() {
        assembler = assembler.source(ProjectAgents {
            workspace: workspace.clone(),
            fallback_files: Vec::new(),
            max_bytes,
        });
        assembler = assembler.source(WorkingDirectory { path: workspace });
    }
    assembler = assembler.source(ModePrefix { mode: input.mode });
    if input.mode == AgentMode::Agent {
        if let Some(path) = input.plan_path.as_deref() {
            if let Some(body) = todos_prompt(path) {
                assembler = assembler.source(PlanChecklist { body });
            }
        }
    }
    assembler.render()
}

/// The open checklist, already wrapped, so the path can sit on the tag.
struct PlanChecklist {
    body: String,
}

impl PromptSource for PlanChecklist {
    fn load(&self) -> Option<PromptBlock> {
        Some(PromptBlock {
            tag: None,
            body: self.body.clone(),
        })
    }
}

/// Names and descriptions, so a source can render without holding the registry lock.
fn registry_snapshot(tools: &ToolRegistry) -> Vec<(String, String)> {
    tools
        .tools()
        .into_iter()
        .map(|tool| (tool.name().to_owned(), tool.description().to_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use async_trait::async_trait;
    use robi_core::error::ToolError;
    use robi_core::tool::{ApprovalDecision, Tool, ToolRegistry, ToolRun};
    use serde_json::{json, Value};

    use super::*;

    struct NamedTool {
        name: &'static str,
        description: &'static str,
    }

    #[async_trait]
    impl Tool for NamedTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            self.description
        }

        fn parameters(&self) -> Value {
            json!({"type": "object"})
        }

        async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
            ApprovalDecision::AllowImmediately
        }

        async fn execute(&self, _args: Value, _run: ToolRun) -> Result<Value, ToolError> {
            Ok(json!({}))
        }
    }

    fn temp_dir(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "robi-prompt-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn the_default_chain_orders_sources_from_general_to_local() {
        let home = temp_dir("home");
        let repo = temp_dir("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(repo.join("pkg")).unwrap();
        fs::write(home.join("system.md"), "user preamble").unwrap();
        fs::write(home.join("AGENTS.md"), "global rules").unwrap();
        fs::write(repo.join("AGENTS.md"), "root rules").unwrap();
        fs::write(repo.join("pkg/AGENTS.md"), "package rules").unwrap();

        let tools = ToolRegistry::new();
        tools
            .register(std::sync::Arc::new(NamedTool {
                name: "read_file",
                description: "Read one file.",
            }))
            .unwrap();

        let prompt = assemble_session(SessionPrompt {
            tools: &tools,
            user_prompt: Some("from settings".into()),
            config_dir: Some(home.clone()),
            workspace: Some(repo.join("pkg")),
            mode: AgentMode::Ask,
            plan_path: None,
            max_bytes: DEFAULT_MAX_BYTES,
        });

        let settings = prompt.find("from settings").unwrap();
        let preamble = prompt.find("user preamble").unwrap();
        let global = prompt.find("global rules").unwrap();
        let root = prompt.find("root rules").unwrap();
        let package = prompt.find("package rules").unwrap();
        let cwd = prompt.find("<cwd>").unwrap();
        let mode = prompt.find("<mode>").unwrap();
        assert!(prompt.find("- read_file: Read one file.").unwrap() < settings);
        assert!(settings < preamble);
        assert!(preamble < global);
        assert!(global < root);
        assert!(root < package);
        assert!(package < cwd);
        assert!(cwd < mode);
        assert!(prompt.contains("Ask mode"));
        assert!(prompt.contains("<user_agents>"));
        assert!(prompt.contains("<project_agents>"));

        let _ = fs::remove_dir_all(home);
        let _ = fs::remove_dir_all(repo);
    }

    #[test]
    fn an_override_file_replaces_agents_md_in_that_directory() {
        let repo = temp_dir("override");
        fs::write(repo.join("AGENTS.md"), "ignored").unwrap();
        fs::write(repo.join("AGENTS.override.md"), "used").unwrap();

        let prompt = assemble_session(SessionPrompt {
            tools: &ToolRegistry::new(),
            user_prompt: None,
            config_dir: None,
            workspace: Some(repo.clone()),
            mode: AgentMode::Agent,
            plan_path: None,
            max_bytes: DEFAULT_MAX_BYTES,
        });
        assert!(prompt.contains("used"));
        assert!(!prompt.contains("ignored"));
        let _ = fs::remove_dir_all(repo);
    }

    #[test]
    fn the_mode_block_lists_only_the_tools_that_mode_registered() {
        let ask = prompt_for(AgentMode::Ask, &[("read_file", "Read one file.")]);
        assert!(ask.contains("<mode>"));
        assert!(ask.contains("Ask mode"));
        assert!(ask.contains("- read_file: Read one file."));
        assert!(!ask.contains("- edit_file:"));
        assert!(!ask.contains("- shell:"));
        assert!(!ask.contains("mode explore"));

        let plan = prompt_for(
            AgentMode::Plan,
            &[
                ("read_file", "Read one file."),
                ("shell", "Run a command."),
                ("write_plan", "Save a plan."),
            ],
        );
        assert!(plan.contains("Plan mode"));
        assert!(plan.contains("- shell: Run a command."));
        assert!(plan.contains("- write_plan: Save a plan."));
        assert!(!plan.contains("- edit_file:"));

        let agent = prompt_for(
            AgentMode::Agent,
            &[
                ("delegate", "Hand a task to a subagent."),
                ("edit_file", "Edit a file."),
                ("write_plan", "Update a plan."),
            ],
        );
        assert!(agent.contains("Agent mode"));
        assert!(agent.contains("delegate with mode explore"));
        assert!(agent.contains("call delegate with mode explore instead of reading"));
        assert!(agent.contains("- edit_file: Edit a file."));
        let mode = agent.find("<mode>").unwrap();
        assert!(agent.find("- edit_file:").unwrap() < mode);
        assert!(agent.contains("Call todos with that path"));
    }

    #[test]
    fn an_agent_prompt_includes_the_plan_checklist() {
        let repo = temp_dir("plan");
        let path = repo.join("ship.md");
        fs::write(
            &path,
            "---\ntodos:\n  - id: \"modes\"\n    content: \"Add the mode registry\"\n    status: \"pending\"\n  - id: \"wire\"\n    content: \"Register the tool\"\n    status: \"in_progress\"\n---\n# Ship\n",
        )
        .unwrap();
        let plan_path = path.to_string_lossy().into_owned();

        let prompt = assemble_session(SessionPrompt {
            tools: &ToolRegistry::new(),
            user_prompt: None,
            config_dir: None,
            workspace: Some(repo.clone()),
            mode: AgentMode::Agent,
            plan_path: Some(plan_path.clone()),
            max_bytes: DEFAULT_MAX_BYTES,
        });
        let mode = prompt.find("<mode>").unwrap();
        let todos = prompt
            .find(&format!("<todos path=\"{plan_path}\">"))
            .unwrap();
        assert!(mode < todos);
        assert!(prompt.contains("- modes (pending) Add the mode registry"));
        assert!(prompt.contains("- wire (in_progress) Register the tool"));
        assert!(!prompt.contains("todos:"));

        let ask = assemble_session(SessionPrompt {
            tools: &ToolRegistry::new(),
            user_prompt: None,
            config_dir: None,
            workspace: Some(repo.clone()),
            mode: AgentMode::Ask,
            plan_path: Some(plan_path),
            max_bytes: DEFAULT_MAX_BYTES,
        });
        assert!(!ask.contains("<todos"));
        let _ = fs::remove_dir_all(repo);
    }

    fn prompt_for(mode: AgentMode, tools: &[(&'static str, &'static str)]) -> String {
        let registry = ToolRegistry::new();
        for (name, description) in tools {
            registry
                .register(std::sync::Arc::new(NamedTool { name, description }))
                .unwrap();
        }
        assemble_session(SessionPrompt {
            tools: &registry,
            user_prompt: None,
            config_dir: None,
            workspace: None,
            mode,
            plan_path: None,
            max_bytes: DEFAULT_MAX_BYTES,
        })
    }
}
