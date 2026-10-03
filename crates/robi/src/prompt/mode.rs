//! The instruction that tells the model which mode it is in.

use robi_core::prompt::PromptBlock;

use crate::domain::chat_session::model::AgentMode;

use super::PromptSource;

/// Appended last, after the working directory, so it outranks earlier rules
/// about what the assistant may do.
pub struct ModePrefix {
    pub mode: AgentMode,
}

impl PromptSource for ModePrefix {
    fn load(&self) -> Option<PromptBlock> {
        Some(PromptBlock {
            tag: Some("mode"),
            body: body(self.mode).to_owned(),
        })
    }
}

fn body(mode: AgentMode) -> &'static str {
    match mode {
        AgentMode::Ask => {
            "You are in Ask mode. Answer questions about the workspace. You have read-only tools. Do not edit files or run commands."
        }
        AgentMode::Plan => {
            "You are in Plan mode. Explore with read tools and a sandboxed shell, then save the plan with write_plan. Do not edit project files. Pass todos for each implementation step, with an id, content, and status of pending, in_progress, completed, or canceled. The body is the markdown plan and does not include the todo list. Pass path when you are revising a plan under .robi/plans. Omit path to create .robi/plans/<plan_name>-<uuid>.md. Repeat the plan in your reply. Saving a plan adds .robi/plans to the workspace-root .gitignore when that file exists. Saving a plan does not apply it. The user applies it by switching to Agent mode."
        }
        AgentMode::Agent => {
            "You are in Agent mode. You may read, edit, and run commands. Apply requested changes with the edit tools instead of only showing the code in your reply. When you need to find, map, or answer how something works across more than a couple of files, call delegate with mode explore. Do that instead of paging through those files yourself. A single known file is still read_file. Use delegate with mode general only when the task needs a command. When the user asks you to plan, do not start the work. Ask them to switch to Plan mode. Plan mode writes a new plan. To revise a plan already under .robi/plans, call write_plan with that path, the full body, and the todos."
        }
    }
}
