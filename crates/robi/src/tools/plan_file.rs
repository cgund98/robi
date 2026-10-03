//! Todo frontmatter on a plan file.
//!
//! `write_plan` writes the list with the markdown body. `todos` patches the
//! list and leaves the body as it was.

use std::path::{Path, PathBuf};

use robi_core::error::ToolError;
use robi_core::ids::SessionId;
use serde::Deserialize;

pub(crate) const MAX_TODOS: usize = 20;
pub(crate) const MAX_TODO_CONTENT: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Todo {
    pub(crate) id: String,
    pub(crate) content: String,
    #[serde(default)]
    pub(crate) status: String,
}

/// `~/.robi/plans/<session_id>`, after `home` is canonicalized.
pub(crate) fn session_plan_directory(home: &Path, session_id: SessionId) -> PathBuf {
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    home.join(".robi")
        .join("plans")
        .join(session_id.to_string())
}

/// The path the model passes and the session stores.
pub(crate) fn plan_argument(session_id: SessionId, file_name: &str) -> String {
    format!("~/.robi/plans/{session_id}/{file_name}")
}

/// A markdown file directly inside this session's plan directory.
pub(crate) fn is_session_plan_file(absolute: &Path, home: &Path, session_id: SessionId) -> bool {
    let Some(name) = absolute.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name == ".md" || !name.ends_with(".md") {
        return false;
    }
    let Some(parent) = absolute.parent() else {
        return false;
    };
    parent == session_plan_directory(home, session_id)
}

pub(crate) fn normalize_todos(todos: &[Todo]) -> Result<Vec<Todo>, ToolError> {
    if todos.len() > MAX_TODOS {
        return Err(ToolError::InvalidArgs(format!(
            "task list is limited to {MAX_TODOS} items"
        )));
    }
    let mut seen = Vec::new();
    let mut in_progress = 0;
    let mut out = Vec::with_capacity(todos.len());
    for todo in todos {
        let id = todo.id.trim();
        let content = todo.content.trim();
        let mut status = todo.status.trim().to_owned();
        if id.is_empty() || id.chars().any(char::is_whitespace) || id.chars().count() > 64 {
            return Err(ToolError::InvalidArgs(
                "task id must be a short slug".into(),
            ));
        }
        if content.is_empty() {
            return Err(ToolError::InvalidArgs(format!(
                "task {id} content is required"
            )));
        }
        if content.chars().count() > MAX_TODO_CONTENT {
            return Err(ToolError::InvalidArgs(format!(
                "task {id} content is limited to {MAX_TODO_CONTENT} characters"
            )));
        }
        if status.is_empty() {
            status = "pending".to_owned();
        }
        if !matches!(
            status.as_str(),
            "pending" | "in_progress" | "completed" | "canceled"
        ) {
            return Err(ToolError::InvalidArgs(
                "task status must be pending, in_progress, completed, or canceled".into(),
            ));
        }
        if seen.iter().any(|seen_id: &String| seen_id == id) {
            return Err(ToolError::InvalidArgs(format!("duplicate task {id}")));
        }
        if status == "in_progress" {
            in_progress += 1;
        }
        seen.push(id.to_owned());
        out.push(Todo {
            id: id.to_owned(),
            content: content.to_owned(),
            status,
        });
    }
    if in_progress > 1 {
        return Err(ToolError::InvalidArgs(
            "only one task can be in progress".into(),
        ));
    }
    Ok(out)
}

/// Writes todos as YAML frontmatter above `body`. An empty list returns `body`.
pub(crate) fn format_plan(todos: &[Todo], body: &str) -> String {
    if todos.is_empty() {
        return body.to_owned();
    }
    let mut text = String::from("---\ntodos:\n");
    for todo in todos {
        text.push_str("  - id: ");
        text.push_str(&yaml_quote(&todo.id));
        text.push_str("\n    content: ");
        text.push_str(&yaml_quote(&todo.content));
        text.push_str("\n    status: ");
        text.push_str(&yaml_quote(&todo.status));
        text.push('\n');
    }
    text.push_str("---\n");
    text.push_str(body);
    text
}

/// Separates todo frontmatter from the markdown body.
///
/// A file without frontmatter returns the text as the body.
pub(crate) fn split_plan(text: &str) -> Result<(Vec<Todo>, String), ToolError> {
    if !text.starts_with("---\n") {
        return Ok((Vec::new(), text.to_owned()));
    }
    let rest = &text["---\n".len()..];
    let Some(end) = rest.find("\n---\n") else {
        return Err(ToolError::Failed("plan frontmatter is not closed".into()));
    };
    let items = parse_todo_front(&rest[..end])?;
    let body = rest[end + "\n---\n".len()..].to_owned();
    Ok((items, body))
}

/// The `<todos>` block for an agent prompt, or nothing when the file is
/// missing or empty. `plan_path` is `~/...` or absolute.
pub(crate) fn todos_prompt(plan_path: &str) -> Option<String> {
    let path = expand_plan_path(plan_path)?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "skipped plan checklist");
            return None;
        }
    };
    let items = match split_plan(&text) {
        Ok((items, _)) => items,
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "skipped plan checklist");
            return None;
        }
    };
    if items.is_empty() {
        return None;
    }
    let mut block = format!("<todos path=\"{}\">\n", attr_escape(plan_path));
    for item in items {
        let content = item.content.replace(['\n', '\r'], " ");
        block.push_str(&format!("- {} ({}) {content}\n", item.id, item.status));
    }
    block.push_str("</todos>");
    Some(block)
}

fn expand_plan_path(plan_path: &str) -> Option<PathBuf> {
    if let Some(rest) = plan_path.strip_prefix("~/") {
        let home = crate::workspace::user_home().ok()?;
        return Some(home.join(rest));
    }
    let path = Path::new(plan_path);
    path.is_absolute().then(|| path.to_path_buf())
}

fn parse_todo_front(front: &str) -> Result<Vec<Todo>, ToolError> {
    let mut lines = front.split('\n');
    if lines.next().unwrap_or("").trim() != "todos:" {
        return Err(ToolError::Failed(
            "plan frontmatter must be a todos list".into(),
        ));
    }
    let mut items = Vec::new();
    let mut current: Option<Todo> = None;
    let flush = |current: &mut Option<Todo>, items: &mut Vec<Todo>| -> Result<(), ToolError> {
        let Some(todo) = current.take() else {
            return Ok(());
        };
        if todo.id.is_empty() {
            return Err(ToolError::Failed(
                "plan frontmatter item is missing an id".into(),
            ));
        }
        items.push(todo);
        Ok(())
    };
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix("  - id: ") {
            flush(&mut current, &mut items)?;
            current = Some(Todo {
                id: yaml_unquote(value)?,
                content: String::new(),
                status: String::new(),
            });
        } else if let Some(value) = line.strip_prefix("    content: ") {
            let Some(todo) = current.as_mut() else {
                return Err(ToolError::Failed(
                    "plan frontmatter has an unexpected line".into(),
                ));
            };
            todo.content = yaml_unquote(value)?;
        } else if let Some(value) = line.strip_prefix("    status: ") {
            let Some(todo) = current.as_mut() else {
                return Err(ToolError::Failed(
                    "plan frontmatter has an unexpected line".into(),
                ));
            };
            todo.status = yaml_unquote(value)?;
        } else {
            return Err(ToolError::Failed(
                "plan frontmatter has an unexpected line".into(),
            ));
        }
    }
    flush(&mut current, &mut items)?;
    normalize_todos(&items)
}

fn yaml_quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' | '"' => {
                quoted.push('\\');
                quoted.push(ch);
            }
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

fn yaml_unquote(value: &str) -> Result<String, ToolError> {
    let value = value.trim();
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return Err(ToolError::Failed(
            "plan frontmatter values must be quoted".into(),
        ));
    }
    let mut out = String::new();
    let mut escaped = false;
    for ch in value[1..value.len() - 1].chars() {
        if escaped {
            match ch {
                '\\' | '"' => out.push(ch),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                _ => {
                    return Err(ToolError::Failed(
                        "plan frontmatter has an unknown escape".into(),
                    ));
                }
            }
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        out.push(ch);
    }
    if escaped {
        return Err(ToolError::Failed(
            "plan frontmatter has a dangling escape".into(),
        ));
    }
    Ok(out)
}

fn attr_escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_plan_round_trips_the_body() {
        let body = "# Ship\n\nDo the thing.\n";
        let todos = normalize_todos(&[
            Todo {
                id: "modes".into(),
                content: "Add the mode registry".into(),
                status: "pending".into(),
            },
            Todo {
                id: "wire".into(),
                content: "Register \"todos\"".into(),
                status: "in_progress".into(),
            },
        ])
        .unwrap();
        let text = format_plan(&todos, body);
        let (parsed, prose) = split_plan(&text).unwrap();
        assert_eq!(prose, body);
        assert_eq!(parsed, todos);
    }

    #[test]
    fn a_file_without_frontmatter_is_the_body() {
        let (items, body) = split_plan("# Only prose\n").unwrap();
        assert!(items.is_empty());
        assert_eq!(body, "# Only prose\n");
    }
}
