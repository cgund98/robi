//! Content search. Ripgrep when `rg` is on `PATH`, otherwise a tree walk.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::Regex;
use robi_core::error::ToolError;
use robi_core::tool::{ApprovalDecision, Concurrency, Tool};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::workspace::{directory_exclusion_globs, PathFilter};

use super::context::{denied, ToolContext};

const MAX_MATCHES: usize = 50;
const MAX_MATCH_BYTES: usize = 32 * 1024;
const MAX_LINE_RUNES: usize = 300;
const BINARY_SNIFF_BYTES: usize = 8 * 1024;
const MAX_RG_STDOUT: usize = 4 * 1024 * 1024;

pub struct Grep {
    ctx: Arc<ToolContext>,
}

impl Grep {
    pub fn new(ctx: Arc<ToolContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for Grep {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents. The pattern is a literal substring unless regex is true. path may be workspace-relative, outside the workspace (../), absolute, or start with ~/. A path outside the workspace is refused until the session grants it. Search uses ripgrep when rg is on PATH, and otherwise walks the tree. Ripgrep's defaults apply, including .gitignore and skipping hidden files. Set hidden to search hidden files. Set no_ignore to search gitignored files. Session path rules still omit denied paths. Results stop at 50 matches or 32 KB. backend is ripgrep or builtin. When truncated is true, narrow the path or pattern."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Substring to search for, or a regular expression when regex is true."},
                "path": {"type": "string", "description": "File or directory to search. Workspace-relative, absolute, or ~/. Defaults to the workspace root."},
                "regex": {"type": "boolean", "description": "Match pattern as a regular expression instead of a literal substring."},
                "hidden": {"type": "boolean", "description": "Search hidden files. Adds ripgrep --hidden."},
                "no_ignore": {"type": "boolean", "description": "Search gitignored files. Adds ripgrep --no-ignore."}
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    fn concurrency(&self) -> Concurrency {
        Concurrency::Concurrent
    }

    async fn requires_approval(&self, _args: &Value) -> ApprovalDecision {
        ApprovalDecision::AllowImmediately
    }

    async fn execute(&self, args: Value, cancel: CancellationToken) -> Result<Value, ToolError> {
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let args: GrepArgs = serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("parse arguments: {err}")))?;
        if args.pattern.is_empty() {
            return Err(ToolError::InvalidArgs("pattern is required".into()));
        }
        if args.regex {
            Regex::new(&args.pattern)
                .map_err(|err| ToolError::InvalidArgs(format!("invalid regex: {err}")))?;
        }
        let session = self
            .ctx
            .sessions
            .get_chat_session(self.ctx.session_id)
            .await
            .map_err(|err| ToolError::Failed(err.to_string()))?;
        let filter = PathFilter::compile(&session.path_rules).map_err(ToolError::Failed)?;
        let resolved = self.ctx.resolve(args.path.as_deref().unwrap_or(""))?;
        if !filter.allows_read(&resolved.relative) {
            return Err(denied(&resolved));
        }
        let query = GrepQuery {
            pattern: args.pattern,
            regex: args.regex,
            hidden: args.hidden,
            no_ignore: args.no_ignore,
            relative: resolved.relative,
        };
        let globs = directory_exclusion_globs(&session.path_rules);
        if let Some(rg) = find_rg() {
            match search_ripgrep(&rg, &self.ctx.root, &filter, &query, &globs, &cancel).await {
                Ok(output) => return Ok(payload(output)),
                Err(RipgrepError::Launch(reason)) => {
                    let mut output = search_builtin(&self.ctx.root, &filter, &query, &cancel)?;
                    output.fallback_reason = Some(reason);
                    return Ok(payload(output));
                }
                Err(RipgrepError::Tool(err)) => return Err(err),
            }
        }
        search_builtin(&self.ctx.root, &filter, &query, &cancel).map(payload)
    }
}

#[derive(Debug, Deserialize)]
struct GrepArgs {
    pattern: String,
    path: Option<String>,
    #[serde(default)]
    regex: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    no_ignore: bool,
}

#[derive(Debug, Clone)]
struct GrepQuery {
    pattern: String,
    regex: bool,
    hidden: bool,
    no_ignore: bool,
    relative: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GrepHit {
    path: String,
    line: u32,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GrepOutput {
    matches: Vec<GrepHit>,
    backend: &'static str,
    truncated: bool,
    fallback_reason: Option<String>,
}

enum RipgrepError {
    Launch(String),
    Tool(ToolError),
}

fn payload(output: GrepOutput) -> Value {
    let matches = output
        .matches
        .into_iter()
        .map(|hit| json!({"path": hit.path, "line": hit.line, "text": hit.text}))
        .collect::<Vec<_>>();
    let mut body = json!({
        "matches": matches,
        "backend": output.backend,
        "truncated": output.truncated,
    });
    if output.truncated {
        body["hint"] = json!(
            "Results stopped at 50 matches or 32 KB. Narrow the path or pattern and call grep again."
        );
    }
    if let Some(reason) = output.fallback_reason {
        body["fallback_reason"] = json!(reason);
    }
    body
}

fn rg_arguments(query: &GrepQuery, globs: &[String]) -> Vec<String> {
    let mut args = vec!["--json".to_owned()];
    if !query.regex {
        args.push("-F".into());
    }
    if query.hidden {
        args.push("--hidden".into());
    }
    if query.no_ignore {
        args.push("--no-ignore".into());
    }
    for glob in globs {
        args.push("--glob".into());
        args.push(glob.clone());
    }
    args.push("-e".into());
    args.push(query.pattern.clone());
    if !query.relative.is_empty() {
        args.push(query.relative.clone());
    }
    args
}

fn find_rg() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join("rg");
        candidate.is_file().then_some(candidate)
    })
}

async fn search_ripgrep(
    rg: &Path,
    root: &Path,
    filter: &PathFilter,
    query: &GrepQuery,
    globs: &[String],
    cancel: &CancellationToken,
) -> Result<GrepOutput, RipgrepError> {
    let child = tokio::process::Command::new(rg)
        .args(rg_arguments(query, globs))
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|err| RipgrepError::Launch(err.to_string()))?;

    let finished = tokio::select! {
        _ = cancel.cancelled() => {
            return Err(RipgrepError::Tool(ToolError::Cancelled));
        }
        finished = child.wait_with_output() => finished,
    };
    let output = finished.map_err(|err| RipgrepError::Launch(err.to_string()))?;
    if !output.status.success() && output.status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RipgrepError::Tool(ToolError::Failed(format!(
            "ripgrep failed: {}",
            stderr.trim()
        ))));
    }
    let truncated_stdout = output.stdout.len() > MAX_RG_STDOUT;
    let stdout = if truncated_stdout {
        &output.stdout[..MAX_RG_STDOUT]
    } else {
        &output.stdout
    };
    let text = String::from_utf8_lossy(stdout);
    let mut parsed = parse_rg_json(&text, root, filter);
    parsed.truncated = parsed.truncated || truncated_stdout;
    Ok(parsed)
}

fn parse_rg_json(stdout: &str, root: &Path, filter: &PathFilter) -> GrepOutput {
    let mut matches = Vec::new();
    let mut bytes = 0;
    let mut truncated = false;
    for line in stdout.lines() {
        if matches.len() >= MAX_MATCHES || bytes >= MAX_MATCH_BYTES {
            truncated = true;
            break;
        }
        let Ok(event) = serde_json::from_str::<RgEvent>(line) else {
            continue;
        };
        if event.kind != "match" {
            continue;
        }
        let Some(data) = event.data else {
            continue;
        };
        let Some(path_text) = data.path.map(|path| path.text) else {
            continue;
        };
        let relative = workspace_relative(root, &path_text);
        if !filter.allows_read(&relative) {
            continue;
        }
        let Some(line_number) = data.line_number else {
            continue;
        };
        let raw = data.lines.map(|lines| lines.text).unwrap_or_default();
        let start = data
            .submatches
            .and_then(|subs| subs.into_iter().next())
            .and_then(|sub| sub.start)
            .unwrap_or(0);
        let text = clip_line(&raw, start, MAX_LINE_RUNES);
        if bytes + text.len() > MAX_MATCH_BYTES && !matches.is_empty() {
            truncated = true;
            break;
        }
        bytes += text.len();
        matches.push(GrepHit {
            path: relative,
            line: line_number,
            text,
        });
    }
    if matches.len() >= MAX_MATCHES {
        truncated = true;
    }
    GrepOutput {
        matches,
        backend: "ripgrep",
        truncated,
        fallback_reason: None,
    }
}

#[derive(Debug, Deserialize)]
struct RgEvent {
    #[serde(rename = "type")]
    kind: String,
    data: Option<RgData>,
}

#[derive(Debug, Deserialize)]
struct RgData {
    path: Option<RgText>,
    lines: Option<RgText>,
    line_number: Option<u32>,
    submatches: Option<Vec<RgSubmatch>>,
}

#[derive(Debug, Deserialize)]
struct RgText {
    text: String,
}

#[derive(Debug, Deserialize)]
struct RgSubmatch {
    start: Option<usize>,
}

fn search_builtin(
    root: &Path,
    filter: &PathFilter,
    query: &GrepQuery,
    cancel: &CancellationToken,
) -> Result<GrepOutput, ToolError> {
    let regex = if query.regex {
        Some(
            Regex::new(&query.pattern)
                .map_err(|err| ToolError::InvalidArgs(format!("invalid regex: {err}")))?,
        )
    } else {
        None
    };
    let start = if query.relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(&query.relative)
    };
    let mut builder = WalkBuilder::new(&start);
    builder
        .hidden(!query.hidden)
        .follow_links(false)
        .git_ignore(!query.no_ignore)
        .git_global(!query.no_ignore)
        .git_exclude(!query.no_ignore)
        .ignore(!query.no_ignore)
        .parents(!query.no_ignore);
    let root_owned = root.to_path_buf();
    let filter_owned = filter.clone();
    builder.filter_entry(move |entry| {
        let relative = workspace_relative(&root_owned, &entry.path().to_string_lossy());
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            return !filter_owned.skip_dir(&relative);
        }
        filter_owned.allows_read(&relative)
    });

    let mut matches = Vec::new();
    let mut bytes = 0;
    let mut truncated = false;
    for entry in builder.build() {
        if cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        if matches.len() >= MAX_MATCHES || bytes >= MAX_MATCH_BYTES {
            truncated = true;
            break;
        }
        let Ok(entry) = entry else {
            continue;
        };
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            continue;
        }
        let path = entry.path();
        if is_binary(path).unwrap_or(false) {
            continue;
        }
        let relative = workspace_relative(root, &path.to_string_lossy());
        let text = std::fs::read_to_string(path).unwrap_or_default();
        for (index, line) in text.lines().enumerate() {
            if matches.len() >= MAX_MATCHES || bytes >= MAX_MATCH_BYTES {
                truncated = true;
                break;
            }
            let Some(start) = match_at(line, &query.pattern, regex.as_ref()) else {
                continue;
            };
            let clipped = clip_line(line, start, MAX_LINE_RUNES);
            if bytes + clipped.len() > MAX_MATCH_BYTES && !matches.is_empty() {
                truncated = true;
                break;
            }
            bytes += clipped.len();
            matches.push(GrepHit {
                path: relative.clone(),
                line: index as u32 + 1,
                text: clipped,
            });
        }
    }
    if matches.len() >= MAX_MATCHES {
        truncated = true;
    }
    Ok(GrepOutput {
        matches,
        backend: "builtin",
        truncated,
        fallback_reason: None,
    })
}

fn match_at(line: &str, pattern: &str, regex: Option<&Regex>) -> Option<usize> {
    match regex {
        Some(regex) => regex.find(line).map(|found| found.start()),
        None => line.find(pattern),
    }
}

fn is_binary(path: &Path) -> std::io::Result<bool> {
    let mut file = File::open(path)?;
    let mut buffer = [0_u8; BINARY_SNIFF_BYTES];
    let read = file.read(&mut buffer)?;
    Ok(buffer[..read].contains(&0))
}

fn workspace_relative(root: &Path, path: &str) -> String {
    crate::workspace::workspace_relative(root, Path::new(path))
}

fn clip_line(line: &str, byte_index: usize, max_runes: usize) -> String {
    let line = line.trim_end_matches(['\n', '\r']);
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= max_runes {
        return chars.into_iter().collect();
    }
    let char_index = line
        .get(..byte_index.min(line.len()))
        .map(|prefix| prefix.chars().count())
        .unwrap_or(0);
    let half = max_runes / 2;
    let mut start = char_index.saturating_sub(half);
    let end = (start + max_runes).min(chars.len());
    if end - start < max_runes {
        start = end.saturating_sub(max_runes);
    }
    let mut text: String = chars[start..end].iter().collect();
    if start > 0 {
        text.insert(0, '…');
    }
    if end < chars.len() {
        text.push('…');
    }
    text
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::domain::chat_session::model::PathRules;

    fn query(pattern: &str) -> GrepQuery {
        GrepQuery {
            pattern: pattern.to_owned(),
            regex: false,
            hidden: false,
            no_ignore: false,
            relative: String::new(),
        }
    }

    fn workspace() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "robi-grep-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "notes.txt\n").unwrap();
        fs::write(root.join("notes.txt"), "resume in a gitignored file\n").unwrap();
        fs::write(root.join(".env"), "TOKEN=super-secret\n").unwrap();
        fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        root
    }

    #[test]
    fn default_rg_arguments_omit_the_optional_flags() {
        let args = rg_arguments(&query("fn main"), &[]);
        assert!(args.contains(&"--json".to_owned()));
        assert!(!args
            .iter()
            .any(|arg| arg == "--hidden" || arg == "--no-ignore"));
        let mut with_flags = query("fn main");
        with_flags.hidden = true;
        with_flags.no_ignore = true;
        let args = rg_arguments(&with_flags, &[]);
        assert!(args.iter().any(|arg| arg == "--hidden"));
        assert!(args.iter().any(|arg| arg == "--no-ignore"));
    }

    #[test]
    fn walker_skips_gitignored_files_unless_no_ignore_is_set() {
        let root = workspace();
        let filter = PathFilter::compile(&PathRules::default()).unwrap();
        let found =
            search_builtin(&root, &filter, &query("resume"), &CancellationToken::new()).unwrap();
        assert!(found.matches.is_empty(), "{found:?}");

        let mut include = query("resume");
        include.no_ignore = true;
        let found = search_builtin(&root, &filter, &include, &CancellationToken::new()).unwrap();
        assert_eq!(found.matches.len(), 1);
        assert_eq!(found.matches[0].path, "notes.txt");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn walker_skips_env_even_when_no_ignore_is_set() {
        let root = workspace();
        let filter = PathFilter::compile(&PathRules::default()).unwrap();
        let mut include = query("TOKEN");
        include.no_ignore = true;
        include.hidden = true;
        let found = search_builtin(&root, &filter, &include, &CancellationToken::new()).unwrap();
        assert!(
            found.matches.iter().all(|hit| hit.path != ".env"),
            "{found:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
