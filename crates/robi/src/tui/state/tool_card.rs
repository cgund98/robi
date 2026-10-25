//! One tool call as a card: a summary, a heading, and a bounded body.
//!
//! The terminal's peer of `src/features/chat/toolCallView.ts`. It produces plain
//! data — no ratatui types and no I/O — so every variant is a unit test, and
//! `state::transcript` is the only place that turns a line into a row.
//!
//! The GUI shows six variants because it has the room for a collapsed row, a
//! diff panel and a separate answer panel. A terminal card is one band, so there
//! are two forms: collapsed (the short diff, six output lines) and expanded (the
//! long diff, the delegate's steps and its answer).

use robi_core::message::{
    ApprovalStatus, ExecutionStatus, SubagentMode, SubagentStepStatus, ToolCall,
};
use serde_json::Value;

/// Diff lines an edit card shows before the rest is counted.
pub const EDIT_VISIBLE_LINES: usize = 4;

/// Diff lines an opened edit card shows before it stops.
pub const EDIT_PREVIEW_LINES: usize = 24;

/// Output lines a collapsed card shows.
pub const MAX_TOOL_OUTPUT_LINES: usize = 6;

/// What a body says while the call is waiting on the user.
///
/// A constant because the view animates the dots that follow it, and both sides
/// have to agree on the phrase.
pub const WAITING_LABEL: &str = "waiting for approval";

/// Output lines an opened card shows.
pub const MAX_EXPANDED_TOOL_LINES: usize = 40;

/// A replacement longer than this is shown as a full replace rather than a
/// line diff, the same rule the GUI uses.
const DIFF_SNIPPET_LIMIT: usize = 400;

/// What one line of a card's body is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardLineKind {
    /// Plain output.
    Body,
    /// A line the change adds.
    DiffAdd,
    /// A line the change removes.
    DiffDel,
    /// The `… (+N lines)` count that ends a capped body.
    More,
    /// Why the call failed.
    Error,
}

/// One line of a card's body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardLine {
    pub kind: CardLineKind,
    pub text: String,
}

impl CardLine {
    /// A plain body line. The transcript reuses this for a fetched original.
    pub fn body(text: impl Into<String>) -> Self {
        Self {
            kind: CardLineKind::Body,
            text: text.into(),
        }
    }
}

/// The verb and target a card, the approval list, and the transcript cursor all
/// show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub verb: String,
    pub target: String,
    /// An inclusive line window, such as `L240-299`, when the call names one.
    pub range: Option<String>,
}

/// One call, ready to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub summary: Summary,
    /// What the tool was for: its `description` argument, else its name.
    pub heading: String,
    /// The line under the heading, and the marker that introduces it.
    pub subject: Option<String>,
    pub marker: char,
    pub lines: Vec<CardLine>,
    /// Lines the cap removed from `lines`.
    pub hidden: usize,
}

impl Card {
    /// The heading as the transcript writes it.
    pub fn heading_line(&self) -> String {
        format!("# {}", self.heading)
    }
}

/// The verb and target for a call, mirroring `toolSummary` in the GUI.
pub fn summary(call: &ToolCall) -> Summary {
    let args = record(Some(&call.args));
    let path = string_field(args, "path");
    let pattern = string_field(args, "pattern");
    let target = |fallback: &str| {
        if path.is_empty() {
            fallback.to_owned()
        } else {
            path.clone()
        }
    };
    let simple = |verb: &str, target: String| Summary {
        verb: verb.to_owned(),
        target,
        range: None,
    };

    match call.name.as_str() {
        "read_file" | "read_code" => {
            let range = read_line_range(args);
            Summary {
                verb: "Read".to_owned(),
                target: target("file"),
                range,
            }
        }
        "grep" => simple(
            "Grepped",
            if pattern.is_empty() {
                target("workspace")
            } else {
                pattern.clone()
            },
        ),
        "semantic_search" => simple(
            "Searched",
            non_empty(string_field(args, "query"), "workspace"),
        ),
        "web_search" => simple("Search", non_empty(string_field(args, "query"), "the web")),
        "web_fetch" => simple("Fetch", non_empty(string_field(args, "url"), "a page")),
        "find" => simple(
            "Found",
            if pattern.is_empty() {
                target("files")
            } else {
                pattern.clone()
            },
        ),
        "list_dir" => simple("Listed", non_empty(path.clone(), ".")),
        "grant" => simple("Grant", target("path")),
        "write_file" => simple("Write", target("file")),
        "edit_file" => simple("Edit", target("file")),
        "delete_file" => simple("Delete", target("file")),
        "shell" => {
            let unsandboxed = record(Some(&call.args))
                .and_then(|args| args.get("unsandboxed"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Summary {
                verb: if unsandboxed {
                    "Run unsandboxed".to_owned()
                } else {
                    "Run".to_owned()
                },
                target: shell_program(&string_field(args, "command")),
                range: None,
            }
        }
        "retrieve" => retrieve_summary(call),
        "delegate" => simple(
            "Delegate",
            non_empty(
                string_field(args, "description"),
                &string_field(args, "task"),
            ),
        ),
        name if name.starts_with("mcp_") => {
            simple("MCP call", name.trim_start_matches("mcp_").to_owned())
        }
        name => Summary {
            verb: name.to_owned(),
            target: if path.is_empty() { pattern } else { path },
            range: None,
        },
    }
}

/// One call's card, in its collapsed or expanded form.
pub fn card(call: &ToolCall, expanded: bool) -> Card {
    let summary = summary(call);
    let marker = if call.name == "shell" { '$' } else { '>' };
    let mut card = Card {
        summary,
        heading: heading(call),
        subject: subject(call),
        marker,
        lines: Vec::new(),
        hidden: 0,
    };
    card.lines = body(call, Form::card(expanded), &mut card.hidden);
    card
}

/// Every line of a call's body, with no cap.
///
/// What the detail view shows. A card truncates because the transcript has to
/// stay scrollable; a view of its own does not.
pub fn full_body(call: &ToolCall) -> Vec<CardLine> {
    let mut hidden = 0;
    body(call, Form::uncapped(true), &mut hidden)
}

/// Which form a body is cut for. The one thing that decides a body's length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Form {
    /// The long form: an opened card, or a card the detail view will show.
    expanded: bool,
    /// No cut at all. The detail view.
    uncapped: bool,
}

impl Form {
    fn card(expanded: bool) -> Self {
        Self {
            expanded,
            uncapped: false,
        }
    }

    fn uncapped(expanded: bool) -> Self {
        Self {
            expanded,
            uncapped: true,
        }
    }

    /// The line cap, or `None` for the whole body.
    ///
    /// A diff and a body cap differently: the GUI shows four changed lines before
    /// the counts and twenty-four when opened, and six output lines before the
    /// tail.
    fn limit(self, diff: bool) -> Option<usize> {
        if self.uncapped {
            return None;
        }
        Some(match (self.expanded, diff) {
            (true, true) => EDIT_PREVIEW_LINES,
            (true, false) => MAX_EXPANDED_TOOL_LINES,
            (false, true) => EDIT_VISIBLE_LINES,
            (false, false) => MAX_TOOL_OUTPUT_LINES,
        })
    }
}

/// The stored bytes behind a compressed result, as text.
///
/// The two shapes are the ones `agent::compress` writes: shell output under
/// `stdout` / `stderr`, MCP text under `text` with `kind: "mcp"`.
pub fn original_text(body: &Value) -> String {
    let map = record(Some(body));
    if let Some(text) = map.and_then(|map| map.get("kind")).and_then(Value::as_str) {
        if text == "mcp" {
            return string_field(map, "text");
        }
    }
    let stdout = string_field(map, "stdout");
    let stderr = string_field(map, "stderr");
    join_non_empty(&[stdout, stderr])
}

fn heading(call: &ToolCall) -> String {
    let description = string_field(record(Some(&call.args)), "description");
    if !description.is_empty() {
        return description;
    }
    if call.execution_status == ExecutionStatus::Succeeded {
        return call.name.clone();
    }
    format!(
        "{} \u{b7} {}",
        call.name,
        status_label(call.execution_status)
    )
}

fn subject(call: &ToolCall) -> Option<String> {
    let args = record(Some(&call.args));
    // Retrieve names a log, not a path: the id and the stream are its subject.
    if call.name == "retrieve" {
        let id = short_id(&string_field(args, "id"));
        let stream = string_field(args, "stream");
        let parts: Vec<&str> = [id.as_str(), stream.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty() && *part != "both")
            .collect();
        return Some(if parts.is_empty() {
            "log".to_owned()
        } else {
            parts.join(" ")
        });
    }
    for key in TARGET_ARGS {
        let value = string_field(args, key);
        if !value.is_empty() {
            return Some(value);
        }
    }
    let compact = serde_json::to_string(&call.args).unwrap_or_default();
    if compact.is_empty() || compact == "null" {
        return None;
    }
    Some(compact)
}

/// The arguments a card shows as its subject, in priority order.
const TARGET_ARGS: &[&str] = &["command", "path", "pattern", "query", "url", "name"];

/// The result fields that hold a readable body, in priority order.
const BODY_FIELDS: &[&str] = &["output", "stdout", "content", "text", "result", "body"];

fn body(call: &ToolCall, form: Form, hidden: &mut usize) -> Vec<CardLine> {
    if let Some(error) = &call.error {
        return error
            .lines()
            .map(|line| CardLine {
                kind: CardLineKind::Error,
                text: line.to_owned(),
            })
            .collect();
    }

    if call.name.starts_with("mcp_") {
        return capped(mcp_lines(call), form, false, hidden);
    }
    if call.name == "delegate" {
        return capped(delegate_lines(call, form.expanded), form, false, hidden);
    }
    if is_edit(&call.name) {
        let lines = edit_lines(call);
        if !lines.is_empty() {
            return capped(lines, form, true, hidden);
        }
    }
    if call.execution_status == ExecutionStatus::Running {
        return vec![CardLine::body("running\u{2026}")];
    }
    if call.approval_status == ApprovalStatus::Pending
        && call.execution_status == ExecutionStatus::NotStarted
        && call.result.is_none()
    {
        return vec![CardLine::body(WAITING_LABEL)];
    }

    let Some(result) = &call.result else {
        return Vec::new();
    };
    let text = match result {
        Value::String(text) => text.clone(),
        Value::Object(map) => BODY_FIELDS
            .iter()
            .find_map(|field| map.get(*field))
            .map(render_value)
            .unwrap_or_else(|| render_value(result)),
        other => render_value(other),
    };
    let lines: Vec<CardLine> = text.lines().map(CardLine::body).collect();
    capped(lines, form, false, hidden)
}

fn is_edit(name: &str) -> bool {
    matches!(name, "write_file" | "edit_file" | "delete_file")
}

/// An edit's diff: the pending replacement, or the result's patch.
fn edit_lines(call: &ToolCall) -> Vec<CardLine> {
    let args = record(Some(&call.args));
    // A paused `edit_file` is the one call whose change is knowable before it
    // runs: the replacement is in its arguments.
    if call.name == "edit_file" && call.result.is_none() {
        let before = string_field(args, "old_string");
        let after = string_field(args, "new_string");
        if !before.is_empty() || !after.is_empty() {
            return diff_lines(&diff_snippet(&before, &after));
        }
    }
    let Some(result) = record(call.result.as_ref()) else {
        return Vec::new();
    };
    let patch = string_field(Some(result), "patch");
    let parsed = parse_unified(&patch);
    let diff = if parsed.is_empty() {
        lines_from_hunks(result)
    } else {
        parsed
    };
    diff_lines(&diff)
}

/// One line of a diff, with the line number it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffLine {
    kind: CardLineKind,
    text: String,
}

impl DiffLine {
    fn context(text: &str) -> Self {
        Self {
            kind: CardLineKind::Body,
            text: text.to_owned(),
        }
    }

    fn add(text: &str) -> Self {
        Self {
            kind: CardLineKind::DiffAdd,
            text: text.to_owned(),
        }
    }

    fn del(text: &str) -> Self {
        Self {
            kind: CardLineKind::DiffDel,
            text: text.to_owned(),
        }
    }
}

fn diff_lines(lines: &[DiffLine]) -> Vec<CardLine> {
    lines
        .iter()
        .map(|line| {
            let sigil = match line.kind {
                CardLineKind::DiffAdd => '+',
                CardLineKind::DiffDel => '-',
                _ => ' ',
            };
            CardLine {
                kind: line.kind,
                text: format!("{sigil} {}", line.text),
            }
        })
        .collect()
}

/// A line diff of one replacement. Long snippets are shown as a full replace.
fn diff_snippet(before: &str, after: &str) -> Vec<DiffLine> {
    let old = split_lines(before);
    let new = split_lines(after);
    if old.len() > DIFF_SNIPPET_LIMIT || new.len() > DIFF_SNIPPET_LIMIT {
        let mut lines: Vec<DiffLine> = old.iter().map(|line| DiffLine::del(line)).collect();
        lines.extend(new.iter().map(|line| DiffLine::add(line)));
        return lines;
    }
    // The longest common subsequence, scored from the end so the walk forward
    // can read the answer in one pass.
    let mut score = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            score[i][j] = if old[i] == new[j] {
                score[i + 1][j + 1] + 1
            } else {
                score[i + 1][j].max(score[i][j + 1])
            };
        }
    }
    let mut lines = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            lines.push(DiffLine::context(&old[i]));
            i += 1;
            j += 1;
        } else if score[i + 1][j] >= score[i][j + 1] {
            lines.push(DiffLine::del(&old[i]));
            i += 1;
        } else {
            lines.push(DiffLine::add(&new[j]));
            j += 1;
        }
    }
    lines.extend(old[i..].iter().map(|line| DiffLine::del(line)));
    lines.extend(new[j..].iter().map(|line| DiffLine::add(line)));
    lines
}

fn render_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    }
}

fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect();
    // `"a\nb\n"` is two lines, not three: the trailing newline ends the second.
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// The lines of a unified patch, skipping the file and hunk headers.
fn parse_unified(patch: &str) -> Vec<DiffLine> {
    if patch.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for line in patch.lines() {
        if line.starts_with("@@") || line.starts_with("--- ") || line.starts_with("+++ ") {
            continue;
        }
        if let Some(rest) = line.strip_prefix('+') {
            lines.push(DiffLine::add(rest));
        } else if let Some(rest) = line.strip_prefix('-') {
            lines.push(DiffLine::del(rest));
        } else if let Some(rest) = line.strip_prefix(' ') {
            lines.push(DiffLine::context(rest));
        }
    }
    lines
}

fn lines_from_hunks(result: &serde_json::Map<String, Value>) -> Vec<DiffLine> {
    let Some(hunks) = result.get("hunks").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for hunk in hunks {
        let Some(hunk) = record(Some(hunk)) else {
            continue;
        };
        for text in string_list(hunk.get("old")) {
            lines.push(DiffLine::del(&text));
        }
        for text in string_list(hunk.get("new")) {
            lines.push(DiffLine::add(&text));
        }
    }
    lines
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn mcp_lines(call: &ToolCall) -> Vec<CardLine> {
    let args = record(Some(&call.args));
    let text = match &call.result {
        Some(Value::String(text)) => text.clone(),
        Some(other) => render_value(other),
        None => String::new(),
    };
    let mut lines: Vec<CardLine> = Vec::new();
    if let Some(args) = args {
        if !args.is_empty() {
            let pretty =
                serde_json::to_string_pretty(&Value::Object(args.clone())).unwrap_or_default();
            lines.extend(pretty.lines().map(CardLine::body));
        }
    }
    if !text.is_empty() {
        if !lines.is_empty() {
            lines.push(CardLine::body(""));
        }
        lines.extend(text.lines().map(CardLine::body));
    }
    lines
}

fn delegate_lines(call: &ToolCall, expanded: bool) -> Vec<CardLine> {
    let Some(snapshot) = &call.subagent else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    if !snapshot.description.is_empty() {
        lines.push(CardLine::body(snapshot.description.clone()));
    }
    lines.push(CardLine::body(match snapshot.mode {
        SubagentMode::Explore => format!("explore \u{b7} {} steps", snapshot.steps.len()),
        SubagentMode::General => format!("general \u{b7} {} steps", snapshot.steps.len()),
    }));
    for step in &snapshot.steps {
        lines.push(CardLine::body(format!(
            "{} {} {}",
            step_mark(step.status),
            step.name,
            step.target
        )));
    }
    if expanded {
        if let Some(answer) = record(call.result.as_ref())
            .and_then(|result| result.get("answer"))
            .and_then(Value::as_str)
        {
            if !answer.is_empty() {
                lines.push(CardLine::body(""));
                lines.extend(answer.lines().map(CardLine::body));
            }
        }
    }
    lines
}

fn step_mark(status: SubagentStepStatus) -> char {
    match status {
        SubagentStepStatus::Running => '\u{25b8}',
        SubagentStepStatus::Ok => '\u{2713}',
        SubagentStepStatus::Denied => '\u{2717}',
        SubagentStepStatus::Failed => '\u{2717}',
    }
}

/// Cut `lines` to the form's cap and count what was lost.
fn capped(mut lines: Vec<CardLine>, form: Form, diff: bool, hidden: &mut usize) -> Vec<CardLine> {
    let Some(limit) = form.limit(diff) else {
        return lines;
    };
    if lines.len() <= limit {
        return lines;
    }
    *hidden = lines.len() - limit;
    lines.truncate(limit);
    lines
}

fn retrieve_summary(call: &ToolCall) -> Summary {
    let args = record(Some(&call.args));
    let result = record(call.result.as_ref());
    let start = result
        .and_then(|result| result.get("start_line"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .max(1);
    let end = result
        .and_then(|result| result.get("end_line"))
        .and_then(Value::as_u64)
        .map(|end| end as usize);
    let range = match end {
        Some(end) if end as u64 >= start => {
            if end as u64 == start {
                Some(format!("L{start}"))
            } else {
                Some(format!("L{start}-{end}"))
            }
        }
        _ => None,
    };
    let id = short_id(&string_field(args, "id"));
    let stream = string_field(args, "stream");
    let parts: Vec<&str> = [id.as_str(), stream.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty() && *part != "both")
        .collect();
    Summary {
        verb: "Retrieved".to_owned(),
        target: if parts.is_empty() {
            "log".to_owned()
        } else {
            parts.join(" ")
        },
        range,
    }
}

fn read_line_range(args: Option<&serde_json::Map<String, Value>>) -> Option<String> {
    let offset = positive_int(args, "offset");
    let limit = positive_int(args, "limit");
    match (offset, limit) {
        (Some(offset), Some(limit)) => Some(format!("L{offset}-{}", offset + limit - 1)),
        (None, Some(limit)) => Some(format!("L1-{limit}")),
        (Some(offset), None) => Some(format!("L{offset}")),
        (None, None) => None,
    }
}

fn positive_int(args: Option<&serde_json::Map<String, Value>>, key: &str) -> Option<u64> {
    args.and_then(|args| args.get(key))
        .and_then(Value::as_u64)
        .filter(|value| *value >= 1)
}

fn short_id(id: &str) -> String {
    let hex: String = id.chars().filter(|c| *c != '-').collect();
    if hex.len() >= 8 {
        hex[..8].to_owned()
    } else {
        id.to_owned()
    }
}

fn shell_program(command: &str) -> String {
    let token = command.split_whitespace().next().unwrap_or("");
    let name = token.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        "command".to_owned()
    } else {
        name.to_owned()
    }
}

fn status_label(status: ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::NotStarted => "not started",
        ExecutionStatus::Running => "running",
        ExecutionStatus::Succeeded => "succeeded",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::Cancelled => "cancelled",
        ExecutionStatus::TimedOut => "timed out",
    }
}

fn record(value: Option<&Value>) -> Option<&serde_json::Map<String, Value>> {
    value.and_then(Value::as_object)
}

fn string_field(map: Option<&serde_json::Map<String, Value>>, key: &str) -> String {
    map.and_then(|map| map.get(key))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn non_empty(value: String, fallback: &str) -> String {
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value
    }
}

fn join_non_empty(parts: &[String]) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use robi_core::ids::ToolCallId;
    use robi_core::message::{ApprovalStatus, ExecutionStatus, SubagentSnapshot, SubagentStep};

    use super::*;

    fn call(name: &str, args: Value, result: Value) -> ToolCall {
        ToolCall {
            id: ToolCallId::new(),
            name: name.to_owned(),
            args,
            args_error: None,
            approval_status: ApprovalStatus::Approved,
            execution_status: ExecutionStatus::Succeeded,
            result: Some(result),
            error: None,
            truncation: None,
            provider_call_id: None,
            subagent: None,
            original_id: None,
        }
    }

    fn texts(lines: &[CardLine]) -> Vec<&str> {
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    #[test]
    fn a_shell_card_shows_the_program_and_the_output() {
        let call = call(
            "shell",
            serde_json::json!({ "command": "cargo test", "description": "Run the suite" }),
            serde_json::json!({ "stdout": "ok", "stderr": "", "exit_code": 0 }),
        );
        let summary = summary(&call);
        assert_eq!(summary.verb, "Run");
        assert_eq!(summary.target, "cargo");

        let card = card(&call, false);
        assert_eq!(card.heading, "Run the suite");
        assert_eq!(card.heading_line(), "# Run the suite");
        assert_eq!(card.subject.as_deref(), Some("cargo test"));
        assert_eq!(card.marker, '$');
        assert_eq!(texts(&card.lines), vec!["ok"]);
    }

    #[test]
    fn a_report_summary_names_its_line_window() {
        let call = call(
            "read_file",
            serde_json::json!({ "path": "a.rs", "offset": 240, "limit": 60 }),
            serde_json::json!({ "content": "x" }),
        );
        let summary = summary(&call);
        assert_eq!(summary.verb, "Read");
        assert_eq!(summary.target, "a.rs");
        assert_eq!(summary.range.as_deref(), Some("L240-299"));
    }

    #[test]
    fn a_pending_edit_shows_the_change_it_would_make() {
        let mut call = call(
            "edit_file",
            serde_json::json!({
                "path": "a.rs",
                "old_string": "let a = 1;",
                "new_string": "let a = 2;",
            }),
            Value::Null,
        );
        call.result = None;
        call.approval_status = ApprovalStatus::Pending;
        call.execution_status = ExecutionStatus::NotStarted;

        let card = card(&call, false);
        assert_eq!(card.summary.verb, "Edit");
        let kinds: Vec<CardLineKind> = card.lines.iter().map(|line| line.kind).collect();
        assert_eq!(
            kinds,
            vec![CardLineKind::DiffDel, CardLineKind::DiffAdd],
            "the change is a removed line and an added one: {:?}",
            texts(&card.lines)
        );
        assert_eq!(texts(&card.lines), vec!["- let a = 1;", "+ let a = 2;"]);
    }

    #[test]
    fn a_result_patch_parses_into_a_diff_without_its_headers() {
        let call = call(
            "edit_file",
            serde_json::json!({ "path": "a.rs" }),
            serde_json::json!({
                "path": "a.rs",
                "patch": "@@ -1,2 +1,2 @@\n--- a/a.rs\n+++ b/a.rs\n keep\n-old\n+new\n",
                "additions": 1,
                "deletions": 1,
            }),
        );
        let card = card(&call, false);
        assert_eq!(
            texts(&card.lines),
            vec!["  keep", "- old", "+ new"],
            "the `---` and `+++` headers must not read as removed and added lines"
        );
    }

    #[test]
    fn a_collapsed_diff_stops_at_four_lines_and_counts_the_rest() {
        let old: String = (0..10).map(|i| format!("old {i}\n")).collect();
        let new: String = (0..10).map(|i| format!("new {i}\n")).collect();
        let mut call = call(
            "edit_file",
            serde_json::json!({ "path": "a.rs", "old_string": old, "new_string": new }),
            Value::Null,
        );
        call.result = None;
        let collapsed = card(&call, false);
        assert_eq!(collapsed.lines.len(), EDIT_VISIBLE_LINES);
        assert_eq!(collapsed.hidden, 16);

        // The whole change is 20 lines, under the opened cap, so nothing is
        // hidden and the count disappears.
        let expanded = card(&call, true);
        assert_eq!(expanded.lines.len(), 20);
        assert_eq!(expanded.hidden, 0);
    }

    #[test]
    fn a_long_output_is_capped_at_six_lines_and_forty_when_opened() {
        let output: String = (0..60).map(|i| format!("line {i}\n")).collect();
        let call = call(
            "shell",
            serde_json::json!({ "command": "seq 60" }),
            serde_json::json!({ "stdout": output }),
        );
        assert_eq!(card(&call, false).lines.len(), MAX_TOOL_OUTPUT_LINES);
        assert_eq!(card(&call, false).hidden, 54);
        assert_eq!(card(&call, true).lines.len(), MAX_EXPANDED_TOOL_LINES);
        assert_eq!(card(&call, true).hidden, 20);
    }

    #[test]
    fn a_failed_call_shows_its_error_instead_of_a_body() {
        let mut call = call("shell", serde_json::json!({ "command": "x" }), Value::Null);
        call.result = None;
        call.execution_status = ExecutionStatus::Failed;
        call.error = Some("it broke".to_owned());
        let card = card(&call, false);
        assert_eq!(card.lines.len(), 1);
        assert_eq!(card.lines[0].kind, CardLineKind::Error);
        assert_eq!(card.lines[0].text, "it broke");
    }

    #[test]
    fn a_paused_call_says_what_it_is_waiting_for() {
        let mut call = call(
            "shell",
            serde_json::json!({ "command": "rm -rf x" }),
            Value::Null,
        );
        call.result = None;
        call.approval_status = ApprovalStatus::Pending;
        call.execution_status = ExecutionStatus::NotStarted;
        call.execution_status = ExecutionStatus::NotStarted;
        let card = card(&call, false);
        assert_eq!(texts(&card.lines), vec![WAITING_LABEL]);
    }

    #[test]
    fn an_mcp_result_is_a_string_not_an_object() {
        let call = call(
            "mcp_github_search",
            serde_json::json!({ "query": "x" }),
            Value::String("found 2".to_owned()),
        );
        assert_eq!(summary(&call).verb, "MCP call");
        assert_eq!(summary(&call).target, "github_search");
        let card = card(&call, false);
        let joined = texts(&card.lines).join("\n");
        assert!(joined.contains("\"query\": \"x\""), "{joined}");
        assert!(joined.contains("found 2"), "{joined}");
    }

    #[test]
    fn a_retrieve_counts_as_a_log_with_a_window() {
        let call = call(
            "retrieve",
            serde_json::json!({ "id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b", "stream": "stdout" }),
            serde_json::json!({
                "stdout": "the log",
                "start_line": 10,
                "end_line": 20,
                "total_lines": 300,
            }),
        );
        let summary = summary(&call);
        assert_eq!(summary.verb, "Retrieved");
        assert_eq!(summary.target, "0193a1b2 stdout");
        assert_eq!(summary.range.as_deref(), Some("L10-20"));
        assert_eq!(
            card(&call, false).subject.as_deref(),
            Some("0193a1b2 stdout")
        );
    }

    #[test]
    fn a_delegate_shows_its_steps_and_only_opens_its_answer() {
        let mut call = call(
            "delegate",
            serde_json::json!({ "description": "Find the parser" }),
            serde_json::json!({ "answer": "it lives in parse.rs" }),
        );
        call.subagent = Some(SubagentSnapshot {
            mode: SubagentMode::Explore,
            description: "Find the parser".to_owned(),
            started_ms: 0,
            steps: vec![
                SubagentStep {
                    name: "shell".to_owned(),
                    target: "rg parser".to_owned(),
                    status: SubagentStepStatus::Ok,
                },
                SubagentStep {
                    name: "read_file".to_owned(),
                    target: "parse.rs".to_owned(),
                    status: SubagentStepStatus::Running,
                },
            ],
        });

        let collapsed = card(&call, false);
        assert_eq!(collapsed.heading, "Find the parser");
        assert!(
            !texts(&collapsed.lines)
                .iter()
                .any(|text| text.contains("it lives")),
            "the answer stays closed: {:?}",
            texts(&collapsed.lines)
        );
        assert!(texts(&collapsed.lines)
            .iter()
            .any(|text| text.contains("parse.rs")));

        let expanded = card(&call, true);
        assert!(
            texts(&expanded.lines)
                .iter()
                .any(|text| text.contains("it lives")),
            "{:?}",
            texts(&expanded.lines)
        );
    }

    #[test]
    fn the_stored_shell_and_mcp_shapes_both_read_as_text() {
        let shell = serde_json::json!({ "stdout": "out", "stderr": "err", "exit_code": 1 });
        assert_eq!(original_text(&shell), "out\nerr");

        let mcp = serde_json::json!({ "kind": "mcp", "text": "the server said" });
        assert_eq!(original_text(&mcp), "the server said");

        assert_eq!(original_text(&serde_json::json!({})), "");
    }

    #[test]
    fn an_unknown_tool_still_gets_a_summary() {
        let call = call(
            "mystery",
            serde_json::json!({ "path": "a.rs" }),
            serde_json::json!({ "nested": { "a": 1 } }),
        );
        assert_eq!(summary(&call).verb, "mystery");
        assert_eq!(summary(&call).target, "a.rs");
        let card = card(&call, false);
        assert!(texts(&card.lines).join("").contains("nested"));
    }
}
