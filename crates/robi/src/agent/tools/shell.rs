//! Run a command. Sandboxed calls stay inside the workspace profile.
//! `unsandboxed: true` always waits for approval.
//!
//! A [`ShellAccess::ReadOnly`] instance is the same tool with two policies
//! changed: its schema drops the escalation keys, and its sandbox profile cannot
//! write the workspace or `.git`. The mode decides which instance it gets.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool, ToolRun};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent::sandbox::{
    apply_allow_read, apply_allow_write, apply_deny, command_env, install_path_wrappers, launch,
    push_classified, toolchain_reads, EnvInput, NetworkMode, Profile, WriteAccess,
};

use super::context::ToolContext;

/// What a shell instance may do. One tool, one schema shape, two policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellAccess {
    /// Ask: the workspace is mounted read-only, so a command that writes fails
    /// inside the sandbox. `write_paths` and `unsandboxed` are not in the
    /// schema at all.
    ReadOnly,
    /// Plan: read-only except `.robi/plans`, which is read-write so the mode can
    /// produce a plan. `write_paths` and `unsandboxed` are still absent.
    PlanReadOnly,
    /// Agent: the workspace is writable inside the sandbox.
    ReadWrite,
}

const READ_WRITE_DESCRIPTION: &str = "Run a command in the workspace. The command is sandboxed: it can read and write the workspace, and it cannot read the home directory, secret files, or the network. cwd must stay inside the workspace. Set read_paths or write_paths when it needs one or two paths outside the workspace. Set unsandboxed to true when it needs the home directory, a logged-in CLI, or more than a few outside paths. Set network to unrestricted when it needs the network. Those calls wait for approval. If the sandbox blocks a path, call shell again with that path in read_paths or write_paths, or with unsandboxed set to true.";

const READ_ONLY_DESCRIPTION: &str = "Run a command in the workspace, read-only. The command is sandboxed: it can read the workspace, and it cannot write the workspace or .git, read the home directory, or reach the network. cwd must stay inside the workspace. There is no write_paths and no unsandboxed in this mode: a command that writes a file fails inside the sandbox, so ask the user to switch to Agent if a change is needed. git status, git diff, git log, grep, sed, find, and cat all work. Build and test commands write to target/ and are refused; verify in Agent mode.";

/// Plan mode's shell: read-only except the plans directory, which is the one
/// place a plan may be written.
const PLAN_DESCRIPTION: &str = "Run a command in the workspace, read-only except .robi/plans. The command is sandboxed: it can read the workspace, and the only directory it can write is .robi/plans, where a plan and its TODO.md go. It cannot write the rest of the workspace or .git, read the home directory, or reach the network. cwd must stay inside the workspace. There is no write_paths and no unsandboxed in this mode: a write anywhere but .robi/plans fails inside the sandbox, so ask the user to switch to Agent for any other change. git status, git diff, git log, grep, sed, find, and cat all work. Build and test commands write to target/ and are refused; verify in Agent mode.";

pub struct Shell {
    ctx: Arc<ToolContext>,
    access: ShellAccess,
}

impl Shell {
    pub fn new(ctx: Arc<ToolContext>, access: ShellAccess) -> Self {
        Self { ctx, access }
    }

    fn read_only(&self) -> bool {
        matches!(
            self.access,
            ShellAccess::ReadOnly | ShellAccess::PlanReadOnly
        )
    }

    /// Plan mode: read-only except `.robi/plans`.
    fn plan_read_only(&self) -> bool {
        self.access == ShellAccess::PlanReadOnly
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
        if self.plan_read_only() {
            PLAN_DESCRIPTION
        } else if self.read_only() {
            READ_ONLY_DESCRIPTION
        } else {
            READ_WRITE_DESCRIPTION
        }
    }

    fn parameters(&self) -> Value {
        if self.read_only() {
            return json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Shell command to run."},
                    "cwd": {"type": "string", "description": "Working directory inside the workspace. Defaults to the workspace root."},
                    "read_paths": {"type": "array", "items": {"type": "string"}, "description": "Extra files or directories to read. The user must approve the call."},
                },
                "required": ["command"],
                "additionalProperties": false
            });
        }
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
        let Some(args) = super::args::parse_args::<ShellArgs>(args.clone(), &["command"]).ok()
        else {
            return ApprovalDecision::AllowImmediately;
        };
        if args.command.trim().is_empty() {
            return ApprovalDecision::AllowImmediately;
        }
        if network_mode(&args.network).is_err() {
            return ApprovalDecision::AllowImmediately;
        }
        if self.read_only() && args.network.as_deref() == Some("unrestricted") {
            // `execute` refuses it; asking first would be a wasted turn.
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
        if self.read_only() {
            // Neither `write_paths` nor `unsandboxed` is in the schema, and
            // `execute` refuses both. A read_path is a read, so the remaining
            // question is the same one the writable shell asks.
            return if args.read_paths.is_empty() {
                ApprovalDecision::AllowImmediately
            } else {
                ApprovalDecision::NeedsApproval
            };
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
        let args = super::args::parse_args::<ShellArgs>(args, &["command"])?;
        let command = args.command.trim();
        if command.is_empty() {
            return Err(ToolError::InvalidArgs("command is required".into()));
        }
        if self.read_only() {
            // Refuse the escalation keys outright. The OS profile is the real
            // boundary, but a property the tool accepts and then ignores is a
            // wasted turn.
            if args.unsandboxed || !args.write_paths.is_empty() {
                return Err(ToolError::InvalidArgs(
                    "this mode is read-only: write_paths and unsandboxed are not available. \
                     Ask the user to switch to Agent mode for a write."
                        .into(),
                ));
            }
            if args.network.as_deref() == Some("unrestricted") {
                return Err(ToolError::InvalidArgs(
                    "this mode is read-only: network is restricted to deny".into(),
                ));
            }
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
        // Plan mode may write one directory. Create it before the profile is
        // built: bubblewrap cannot bind a path that does not exist, and the model
        // is told to `mkdir -p` only below this root.
        let extra_writes = if self.plan_read_only() {
            let plans = self.ctx.root.join(".robi").join("plans");
            let create = plans.clone();
            let resolved = crate::agent::blocking::call(move || -> Result<PathBuf, String> {
                std::fs::create_dir_all(&create)
                    .map_err(|err| format!("create {}: {err}", create.display()))?;
                Ok(create.canonicalize().unwrap_or(create))
            })
            .await
            .map_err(ToolError::Failed)?
            .map_err(ToolError::Failed)?;
            vec![resolved]
        } else {
            Vec::new()
        };
        let session = self
            .ctx
            .sessions
            .get_chat_session(self.ctx.session_id)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let mut profile = Profile {
            sandboxed: !args.unsandboxed,
            write: if self.read_only() {
                WriteAccess::ReadOnly
            } else {
                WriteAccess::ReadWrite
            },
            workspace: self.ctx.root.clone(),
            home: home.clone(),
            temp_dir: temp_dir.clone(),
            cwd,
            network,
            env: Vec::new(),
            toolchain_reads: toolchain,
            wide_reads: Vec::new(),
            wide_writes: Vec::new(),
            extra_writes,
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
        let timeout = std::time::Duration::from_secs(u64::from(timeout_seconds));
        // The mechanism is the only sandbox setting the dispatch needs, and it is
        // read here so the container path is skipped entirely when it is `os`.
        let request = crate::agent::sandbox::Launch::new(
            &profile,
            command,
            run.cancel,
            timeout,
            self.ctx.session_id,
        );
        #[cfg(feature = "container-sandbox")]
        let request = {
            let settings = self.ctx.settings.clone();
            let mechanism =
                crate::agent::sandbox::container::config::mechanism(settings.as_deref()).await;
            // A project file is opt-in: `sandbox_project_files` must be on before a
            // checked-in `sandbox.toml` can name an image or widen egress.
            let project_files_on = self
                .ctx
                .setting_value(crate::domain::settings::keys::SANDBOX_PROJECT_FILES)
                .await
                == crate::domain::settings::keys::PROJECT_FILES_ON;
            request
                .with_settings(self.ctx.settings.as_deref(), project_files_on)
                .with_mechanism(mechanism)
                .with_registry(crate::agent::sandbox::container::Registry::global())
        };
        let output = launch(request).await.map_err(ToolError::Failed)?;
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
                let hint = if self.plan_read_only() {
                    format!(
                        "{hint} This mode is read-only except `.robi/plans`: only a plan file may be written there. Ask the user to switch to Agent for any other change."
                    )
                } else if self.read_only() {
                    format!(
                        "{hint} This mode is read-only: ask the user to switch to Agent for a change."
                    )
                } else {
                    hint
                };
                payload["hint"] = json!(hint);
            }
        }
        Ok(payload)
    }
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
    use crate::agent::tools::test_harness::harness;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn a_plain_command_does_not_ask_and_an_unsandboxed_one_does() {
        let harness = harness().await;
        let tool = Shell::new(Arc::clone(&harness.ctx), ShellAccess::ReadWrite);
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
        let tool = Shell::new(Arc::clone(&harness.ctx), ShellAccess::ReadWrite);
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
        let tool = Shell::new(Arc::clone(&harness.ctx), ShellAccess::ReadWrite);
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
        let tool = Shell::new(Arc::clone(&harness.ctx), ShellAccess::ReadWrite);
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
        let tool = Shell::new(Arc::clone(&harness.ctx), ShellAccess::ReadWrite);
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

    /// S9: Plan mode's shell writes `.robi/plans` and nothing else.
    #[tokio::test]
    async fn plan_mode_writes_a_plan_directory_and_nothing_else() {
        let harness = harness().await;
        let tool = Shell::new(Arc::clone(&harness.ctx), ShellAccess::PlanReadOnly);
        let write = "mkdir -p .robi/plans/ship-mode-registry && \
             printf '*\\n' > .robi/plans/.gitignore && \
             printf '# Ship\\n' > .robi/plans/ship-mode-registry/PLAN.md && \
             printf '# TODO\\n\\n- [ ] One\\n' > .robi/plans/ship-mode-registry/TODO.md";
        let result = tool
            .execute(
                json!({ "command": write }),
                ToolRun::new(CancellationToken::new()),
            )
            .await
            .unwrap();
        assert_eq!(result["exit_code"], 0, "{result}");
        assert!(harness
            .root
            .join(".robi/plans/ship-mode-registry/TODO.md")
            .is_file());
        assert!(harness.root.join(".robi/plans/.gitignore").is_file());

        // A second, differently named plan directory: the allow is the whole
        // directory, not one file, and it is recursive.
        let second = tool
            .execute(
                json!({"command": "mkdir -p .robi/plans/drop-plan-column/notes && printf 'x\\n' > .robi/plans/drop-plan-column/notes/draft.md"}),
                ToolRun::new(CancellationToken::new()),
            )
            .await
            .unwrap();
        assert_eq!(second["exit_code"], 0, "{second}");

        for command in [
            "printf 'x\\n' > src/robi-leak.rs",
            "printf 'x\\n' > notes.txt",
            "printf 'x\\n' > .git/robi-leak",
            "mkdir -p .robi/plans-evil && printf 'x\\n' > .robi/plans-evil/x.md",
        ] {
            let refused = tool
                .execute(
                    json!({ "command": command }),
                    ToolRun::new(CancellationToken::new()),
                )
                .await
                .unwrap();
            assert_ne!(refused["exit_code"], 0, "{command} should be refused");
        }
        assert!(!harness.root.join("src/robi-leak.rs").exists());
        assert!(!harness.root.join("notes.txt").exists());
        assert!(!harness.root.join(".git/robi-leak").exists());
        assert!(!harness.root.join(".robi/plans-evil").exists());

        let hint = tool
            .execute(
                json!({"command": "printf 'x\\n' > notes.txt"}),
                ToolRun::new(CancellationToken::new()),
            )
            .await
            .unwrap();
        let hint = hint["hint"].as_str().unwrap_or_default();
        assert!(hint.contains(".robi/plans"), "{hint}");
    }

    /// S9: Agent mode edits plan files like any other workspace file.
    #[tokio::test]
    async fn agent_mode_writes_plan_files() {
        let harness = harness().await;
        let tool = Shell::new(Arc::clone(&harness.ctx), ShellAccess::ReadWrite);
        let result = tool
            .execute(
                json!({"command": "mkdir -p .robi/plans/ship-mode-registry && printf '*\\n' > .robi/plans/.gitignore && printf -- '- [ ] One\\n' > .robi/plans/ship-mode-registry/TODO.md && printf -- '- [x] One\\n' > .robi/plans/ship-mode-registry/TODO.md"}),
                ToolRun::new(CancellationToken::new()),
            )
            .await
            .unwrap();
        assert_eq!(result["exit_code"], 0, "{result}");
        let todo =
            std::fs::read_to_string(harness.root.join(".robi/plans/ship-mode-registry/TODO.md"))
                .unwrap();
        assert_eq!(todo, "- [x] One\n");
    }

    #[test]
    fn the_plan_description_names_the_one_writable_directory() {
        let descriptions: Vec<&str> = [
            READ_ONLY_DESCRIPTION,
            PLAN_DESCRIPTION,
            READ_WRITE_DESCRIPTION,
        ]
        .into_iter()
        .collect();
        assert!(descriptions[1].contains(".robi/plans"));
        assert!(descriptions[1].contains("no write_paths"));
        assert!(!descriptions[0].contains(".robi/plans"));
    }
}
