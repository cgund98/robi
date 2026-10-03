//! Instruction files. A missing file is skipped.

use std::fs;
use std::path::{Path, PathBuf};

use robi_core::prompt::{keep_tail, PromptBlock};

use super::PromptSource;

/// One optional file, wrapped in `tag` when it exists and is non-empty.
pub struct InstructionFile {
    pub path: PathBuf,
    pub tag: &'static str,
    pub max_bytes: usize,
}

impl PromptSource for InstructionFile {
    fn load(&self) -> Option<PromptBlock> {
        let body = read_optional(&self.path)?;
        if body.trim().is_empty() {
            return None;
        }
        Some(PromptBlock {
            tag: Some(self.tag),
            body: keep_tail(body.trim(), self.max_bytes),
        })
    }
}

/// `AGENTS.md` from the git root down to the workspace, root first.
///
/// `AGENTS.override.md` in a directory replaces `AGENTS.md` in that directory
/// only. `fallback_files` are extra names read beside those two.
pub struct ProjectAgents {
    pub workspace: PathBuf,
    pub fallback_files: Vec<String>,
    pub max_bytes: usize,
}

impl PromptSource for ProjectAgents {
    fn load(&self) -> Option<PromptBlock> {
        let mut blocks = Vec::new();
        for dir in instruction_dirs(&self.workspace) {
            let Some((name, body)) = directory_instructions(&dir, &self.fallback_files) else {
                continue;
            };
            if body.trim().is_empty() {
                continue;
            }
            blocks.push(format!("# {}\n{}", dir.join(name).display(), body.trim()));
        }
        if blocks.is_empty() {
            return None;
        }
        Some(PromptBlock {
            tag: Some("project_agents"),
            body: keep_tail(&blocks.join("\n\n"), self.max_bytes),
        })
    }
}

/// The workspace the tools are rooted at.
pub struct WorkingDirectory {
    pub path: PathBuf,
}

impl PromptSource for WorkingDirectory {
    fn load(&self) -> Option<PromptBlock> {
        Some(PromptBlock {
            tag: Some("cwd"),
            body: self.path.display().to_string(),
        })
    }
}

fn instruction_dirs(workspace: &Path) -> Vec<PathBuf> {
    let root = git_root(workspace);
    let Ok(relative) = workspace.strip_prefix(&root) else {
        return vec![workspace.to_path_buf()];
    };
    if relative.as_os_str().is_empty() {
        return vec![root];
    }
    let mut dirs = vec![root.clone()];
    let mut current = root;
    for part in relative.components() {
        current.push(part);
        dirs.push(current.clone());
    }
    dirs
}

fn git_root(dir: &Path) -> PathBuf {
    let mut current = dir.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return current;
        }
        if !current.pop() {
            return dir.to_path_buf();
        }
    }
}

fn directory_instructions(dir: &Path, fallback: &[String]) -> Option<(String, String)> {
    let mut blocks = Vec::new();
    let mut name = "AGENTS.md".to_owned();
    if let Some(body) = read_optional(&dir.join("AGENTS.override.md")) {
        if !body.trim().is_empty() {
            name = "AGENTS.override.md".to_owned();
            blocks.push(body);
        }
    } else if let Some(body) = read_optional(&dir.join("AGENTS.md")) {
        if !body.trim().is_empty() {
            blocks.push(body);
        }
    }
    for extra in fallback {
        let extra = extra.trim();
        if extra.is_empty() || extra == "AGENTS.md" || extra == "AGENTS.override.md" {
            continue;
        }
        if let Some(body) = read_optional(&dir.join(extra)) {
            if !body.trim().is_empty() {
                blocks.push(body);
            }
        }
    }
    if blocks.is_empty() {
        return None;
    }
    Some((name, blocks.join("\n\n")))
}

fn read_optional(path: &Path) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(text) => Some(text.trim_end().to_owned()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "skipped instruction file");
            None
        }
    }
}
