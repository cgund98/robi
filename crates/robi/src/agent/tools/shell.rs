//! Run a command. Sandboxed calls stay inside the workspace profile.
//! `unsandboxed: true` always waits for approval.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent::sandbox::{
    apply_allow_read, apply_allow_write, apply_deny, command_env, install_path_wrappers, launch,
    push_classified, toolchain_reads, EnvInput, NetworkMode, Profile,
};

use super::context::ToolContext;

pub struct Shell {
    ctx: Arc<ToolContext>,
}

impl Shell {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[derive(Debug, Deserialize)]
struct ShellArgs {
    command: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    unsandboxed: bool,
    #[serde(default)]
    read_paths: Vec<String>,
    #[serde(default)]
    write_paths: Vec<String>,
    #[serde(default)]
    network: Option<String>,
}

#[async_trait]
impl Tool for Shell {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Run a command in the workspace. The command is sandboxed: it can read and write the workspace, and it cannot read the home directory, secret files, or the network. cwd must stay inside the workspace. Set read_paths or write_paths when it needs one or two paths outside the workspace. Set unsandboxed to true when it needs the home directory, a logged-in CLI, or more than a few outside paths. Set network to unrestricted when it needs the network. Those calls wait for approval. If the sandbox blocks a path, call shell again with that path in read_paths or write_paths, or with unsandboxed set to true."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Shell command to run."},
                "cwd": {"type": "string", "description": "Working directory inside the workspace. Defaults to the workspace root."},
                "unsandboxed": {"type": "boolean", "description": "Run without the sandbox. The user must approve the call."},
                "read_paths": {"type": "array", "items": {"type": "string"}, "description": "Extra files or directories to read. The user must approve the call."},
                "write_paths": {"type": "array", "items": {"type": "string"}, "description": "Extra files or directories to write. The user must approve the call."},
                "network": {"type": "string", "enum": ["deny", "unrestricted"], "description": "deny is the default. unrestricted waits for approval and keeps the filesystem sandbox."}
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Exclusive
    }

    async fn requires_approval(&self, args: &Value) -> ApprovalDecision {
        let Some(args) = parse_args(args) else {
            return ApprovalDecision::AllowImmediately;
        };
        if args.command.trim().is_empty() {
            return ApprovalDecision::AllowImmediately;
        }
        if network_mode(&args.network).is_err() {
            return ApprovalDecision::AllowImmediately;
        }
        if args
            .cwd
            .as_deref()
            .is_some_and(|cwd| self.working_dir(cwd).is_err())
        {
            return ApprovalDecision::AllowImmediately;
        }
        if args
            .read_paths
            .iter()
            .chain(&args.write_paths)
            .any(|path| self.ctx.resolve(path).is_err())
        {
            return ApprovalDecision::AllowImmediately;
        }
        if args.unsandboxed
            || !args.read_paths.is_empty()
            || !args.write_paths.is_empty()
            || args.network.as_deref() == Some("unrestricted")
        {
            return ApprovalDecision::NeedsApproval;
        }
        ApprovalDecision::AllowImmediately
    }

    async fn execute(&self, args: Value, run: ToolRun) -> Result<Value, ToolError> {
        if run.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let args = parse_args(&args).ok_or_else(|| {
            ToolError::InvalidArgs(
                "unsandboxed must be a boolean and network must be deny or unrestricted".into(),
            )
        })?;
        let command = args.command.trim();
        if command.is_empty() {
            return Err(ToolError::InvalidArgs("command is required".into()));
        }
        let network = network_mode(&args.network).map_err(ToolError::InvalidArgs)?;
        let cwd = match &args.cwd {
            Some(cwd) => self.working_dir(cwd)?,
            None => self.ctx.root.clone(),
        };
        let home = crate::agent::workspace::user_home()
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let unsandboxed = args.unsandboxed;
        let (home, temp_dir, toolchain) = crate::agent::blocking::call(move || {
            let home = home.canonicalize().unwrap_or(home);
            let temp_dir = private_temp().map_err(|err| err.to_string())?;
            let toolchain = if unsandboxed {
                Vec::new()
            } else {
                toolchain_reads(&home)
            };
            Ok::<_, String>((home, temp_dir, toolchain))
        })
        .await
        .map_err(ToolError::Failed)?
        .map_err(ToolError::Failed)?;
        let temp_guard = TempDir(temp_dir.clone());
        let session = self
            .ctx
            .sessions
            .get_chat_session(self.ctx.session_id)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let mut profile = Profile {
            sandboxed: !args.unsandboxed,
            workspace: self.ctx.root.clone(),
            home: home.clone(),
            temp_dir: temp_dir.clone(),
            cwd,
            network,
            env: Vec::new(),
            toolchain_reads: toolchain,
            wide_reads: Vec::new(),
            wide_writes: Vec::new(),
            protected_read_files: Vec::new(),
            protected_write_files: Vec::new(),
            protected_read_dirs: Vec::new(),
            protected_write_dirs: Vec::new(),
            deny_reads: Vec::new(),
            deny_writes: Vec::new(),
        };
        if profile.sandboxed {
            for pattern in &session.path_rules.allow_read {
                apply_allow_read(pattern, &self.ctx.root, &home, &mut profile)
                    .map_err(|err| ToolError::Failed(err.to_string()))?;
            }
            for pattern in &session.path_rules.deny_read {
                apply_deny(pattern, &self.ctx.root, true, &mut profile)
                    .map_err(|err| ToolError::Failed(err.to_string()))?;
            }
            for pattern in &session.path_rules.deny_write {
                apply_deny(pattern, &self.ctx.root, false, &mut profile)
                    .map_err(|err| ToolError::Failed(err.to_string()))?;
            }
            let mut configured = session.path_rules.clone();
            configured.allow_read.clear();
            configured.allow_write.clear();
            self.ctx.append_configured_allows(&mut configured).await;
            for pattern in &configured.allow_read {
                apply_allow_read(pattern, &self.ctx.root, &home, &mut profile)
                    .map_err(|err| ToolError::Failed(err.to_string()))?;
            }
            for pattern in &configured.allow_write {
                apply_allow_write(pattern, &self.ctx.root, &home, &mut profile)
                    .map_err(|err| ToolError::Failed(err.to_string()))?;
            }
            for path in &args.read_paths {
                let resolved = self.ctx.resolve(path)?;
                push_classified(resolved.absolute, &home, true, &mut profile);
            }
            for path in &args.write_paths {
                let resolved = self.ctx.resolve(path)?;
                push_classified(resolved.absolute, &home, false, &mut profile);
            }
        }
        let extra_path = self
            .ctx
            .setting_value(crate::domain::settings::keys::PATH_ENTRIES)
            .await;
        let path_bin = if profile.sandboxed {
            let temp_dir = temp_dir.clone();
            let home = home.clone();
            let extra_path = extra_path.clone();
            crate::agent::blocking::call(move || {
                install_path_wrappers(&temp_dir, &home, &extra_path)
            })
            .await
            .ok()
            .flatten()
        } else {
            None
        };
        let path_prefix = path_bin
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        profile.env = command_env(&EnvInput {
            home: &home,
            workspace: &self.ctx.root,
            temp_dir: &temp_dir,
            sandboxed: profile.sandboxed,
            parent_path: &std::env::var("PATH").unwrap_or_default(),
            extra_path: &extra_path,
            path_prefix: &path_prefix,
            lang: std::env::var("LANG").ok().as_deref(),
            user: std::env::var("USER").ok().as_deref(),
        });
        let timeout_seconds = self
            .ctx
            .bounded_u32(
                crate::domain::settings::keys::TOOL_TIMEOUT_SECONDS,
                crate::domain::settings::keys::DEFAULT_TIMEOUT_SECONDS
                    .parse()
                    .unwrap_or(120),
                crate::domain::settings::keys::TIMEOUT_LIMIT_SECONDS,
            )
            .await;
        let output = launch(
            &profile,
            command,
            run.cancel,
            std::time::Duration::from_secs(u64::from(timeout_seconds)),
        )
        .await
        .map_err(ToolError::Failed)?;
        drop(temp_guard);
        let mut payload = json!({
            "exit_code": output.exit_code,
            "stdout": output.stdout,
            "stderr": output.stderr,
            "truncated": output.truncated,
            "sandboxed": profile.sandboxed,
        });
        if profile.sandboxed {
            if let Some(hint) = blocked_hint(&output.stderr) {
                payload["hint"] = json!(hint);
            }
        }
        Ok(payload)
    }
}

fn parse_args(args: &Value) -> Option<ShellArgs> {
    serde_json::from_value(args.clone()).ok()
}

fn network_mode(network: &Option<String>) -> Result<NetworkMode, String> {
    match network.as_deref() {
        None | Some("deny") => Ok(NetworkMode::Deny),
        Some("unrestricted") => Ok(NetworkMode::Unrestricted),
        Some(other) => Err(format!("network must be deny or unrestricted, got {other}")),
    }
}

fn working_dir_in(root: &std::path::Path, argument: &str) -> Result<PathBuf, ToolError> {
    let home =
        crate::agent::workspace::user_home().map_err(|err| ToolError::Failed(err.to_string()))?;
    let resolved = crate::agent::workspace::resolve_path(root, argument, &home)
        .map_err(|err| ToolError::Failed(format!("{err}: {argument}")))?;
    if resolved.relative.starts_with("..") || PathBuf::from(&resolved.relative).is_absolute() {
        return Err(ToolError::Failed(format!(
            "cwd is outside the workspace: {argument}"
        )));
    }
    Ok(resolved.absolute)
}

impl Shell {
    fn working_dir(&self, argument: &str) -> Result<PathBuf, ToolError> {
        working_dir_in(&self.ctx.root, argument)
    }
}

fn private_temp() -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "robi-shell-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    std::fs::create_dir_all(&dir)?;
    dir.canonicalize()
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let path = std::mem::take(&mut self.0);
        if path.as_os_str().is_empty() {
            return;
        }
        let remove = move || {
            let _ = std::fs::remove_dir_all(&path);
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            // Detach the blocking cleanup task; we cannot await inside `Drop`.
            std::mem::drop(tokio::task::spawn_blocking(remove));
        } else {
            remove();
        }
    }
}

fn blocked_hint(stderr: &str) -> Option<String> {
    let mut paths = Vec::new();
    for line in stderr.lines() {
        if !(line.contains("Operation not permitted") || line.contains("Permission denied")) {
            continue;
        }
        if let Some(path) = path_in_denial(line) {
            if !paths.iter().any(|existing| existing == &path) {
                paths.push(path);
            }
        }
    }
    if paths.is_empty()
        && !stderr.contains("Operation not permitted")
        && !stderr.contains("Permission denied")
    {
        return None;
    }
    let retry = "Call shell again with that path in read_paths or write_paths, or set unsandboxed to true. Those calls wait for approval.";
    if paths.is_empty() {
        return Some(format!("The sandbox blocked a path. {retry}"));
    }
    Some(format!("The sandbox blocked {}. {retry}", paths.join(", ")))
}

fn path_in_denial(line: &str) -> Option<String> {
    for word in line.split_whitespace() {
        let word = word.trim_matches(|ch| matches!(ch, ':' | ',' | '\'' | '"'));
        if word.starts_with('/') {
            return Some(word.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::apply_tests::harness;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn a_plain_command_does_not_ask_and_an_unsandboxed_one_does() {
        let harness = harness().await;
        let tool = Shell::new(Arc::clone(&harness.ctx));
        assert_eq!(
            tool.requires_approval(&json!({"command": "echo hi"})).await,
            ApprovalDecision::AllowImmediately
        );
        assert_eq!(
            tool.requires_approval(&json!({"command": "echo hi", "unsandboxed": true}))
                .await,
            ApprovalDecision::NeedsApproval
        );
        assert_eq!(
            tool.requires_approval(
                &json!({"command": "curl example.com", "network": "unrestricted"})
            )
            .await,
            ApprovalDecision::NeedsApproval
        );
        assert_eq!(
            tool.requires_approval(&json!({"command": "echo hi", "read_paths": ["/etc/hosts"]}))
                .await,
            ApprovalDecision::NeedsApproval
        );
    }

    #[tokio::test]
    async fn a_bad_argument_does_not_ask() {
        let harness = harness().await;
        let tool = Shell::new(Arc::clone(&harness.ctx));
        assert_eq!(
            tool.requires_approval(&json!({"command": ""})).await,
            ApprovalDecision::AllowImmediately
        );
        assert_eq!(
            tool.requires_approval(&json!({"command": "echo", "network": "allow"}))
                .await,
            ApprovalDecision::AllowImmediately
        );
        assert_eq!(
            tool.requires_approval(&json!({"command": "echo", "cwd": "/etc"}))
                .await,
            ApprovalDecision::AllowImmediately
        );
    }

    #[tokio::test]
    async fn echo_runs_in_the_sandbox() {
        let harness = harness().await;
        let tool = Shell::new(Arc::clone(&harness.ctx));
        let result = tool
            .execute(
                json!({"command": "echo hi"}),
                ToolRun::new(CancellationToken::new()),
            )
            .await
            .unwrap();
        assert_eq!(result["stdout"], "hi\n");
        assert_eq!(result["exit_code"], 0);
        assert_eq!(result["sandboxed"], true);
        assert_eq!(result["truncated"], false);
    }

    #[tokio::test]
    async fn a_cwd_outside_the_workspace_is_refused() {
        let harness = harness().await;
        let tool = Shell::new(Arc::clone(&harness.ctx));
        let err = tool
            .execute(
                json!({"command": "echo hi", "cwd": "/etc"}),
                ToolRun::new(CancellationToken::new()),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("outside the workspace"), "{err}");
    }

    #[tokio::test]
    async fn the_sandbox_hides_the_home_directory() {
        let harness = harness().await;
        let tool = Shell::new(Arc::clone(&harness.ctx));
        let home = crate::agent::workspace::user_home().unwrap();
        let result = tool
            .execute(
                json!({"command": format!("ls {}", home.display())}),
                ToolRun::new(CancellationToken::new()),
            )
            .await
            .unwrap();
        assert_ne!(result["exit_code"], 0);
        assert_eq!(result["sandboxed"], true);
    }
}
