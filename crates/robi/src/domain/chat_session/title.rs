//! Turn a model reply into a chat session title.
//!
//! The reply is not stored. Callers write the result only when the session's
//! title is still null.

use super::service::CHAT_SESSION_TITLE_MAX_CHARS;

/// The first line of a model reply, trimmed to a title, or `None` when it is empty.
pub fn normalize_generated_title(raw: &str) -> Option<String> {
    let line = raw.lines().next()?.trim();
    let mut current = line.to_owned();
    for _ in 0..3 {
        let next = strip_label(&strip_wrapping(&current)).to_owned();
        if next == current {
            break;
        }
        current = next;
    }
    let collapsed = current.split_whitespace().collect::<Vec<_>>().join(" ");
    let collapsed = strip_one_trailing_period(&collapsed);
    if collapsed.is_empty() || collapsed.chars().all(|ch| ch == '.') {
        return None;
    }
    let title: String = collapsed
        .chars()
        .take(CHAT_SESSION_TITLE_MAX_CHARS)
        .collect();
    let title = title.trim().to_owned();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

fn strip_wrapping(value: &str) -> String {
    let mut current = value.trim().to_owned();
    for _ in 0..2 {
        let Some(stripped) = strip_one_pair(&current) else {
            break;
        };
        current = stripped.trim().to_owned();
    }
    current
}

fn strip_one_pair(value: &str) -> Option<&str> {
    let mut chars = value.chars();
    let open = chars.next()?;
    let close = match open {
        '"' | '\'' | '`' => open,
        _ => return None,
    };
    if !value.ends_with(close) || value.chars().count() < 2 {
        return None;
    }
    let inner = &value[open.len_utf8()..value.len() - close.len_utf8()];
    Some(inner)
}

fn strip_label(value: &str) -> &str {
    value
        .strip_prefix("Title:")
        .or_else(|| value.strip_prefix("title:"))
        .unwrap_or(value)
        .trim()
}

fn strip_one_trailing_period(value: &str) -> &str {
    let Some(stripped) = value.strip_suffix('.') else {
        return value;
    };
    if stripped.is_empty() || stripped.ends_with('.') {
        return value;
    }
    stripped.trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_short_title() {
        assert_eq!(
            normalize_generated_title("  Parser cleanup  ").as_deref(),
            Some("Parser cleanup")
        );
    }

    #[test]
    fn uses_the_first_line_and_drops_wrapping() {
        assert_eq!(
            normalize_generated_title("\"Parser cleanup.\"\nmore").as_deref(),
            Some("Parser cleanup")
        );
        assert_eq!(
            normalize_generated_title("Title: 'Fix the build'").as_deref(),
            Some("Fix the build")
        );
    }

    #[test]
    fn rejects_empty_replies() {
        for raw in ["", "   ", "\"\"", ".", "Title:"] {
            assert_eq!(normalize_generated_title(raw), None);
        }
    }

    #[test]
    fn truncates_to_the_column_limit() {
        let raw = "a".repeat(CHAT_SESSION_TITLE_MAX_CHARS + 40);
        let title = normalize_generated_title(&raw).unwrap();
        assert_eq!(title.chars().count(), CHAT_SESSION_TITLE_MAX_CHARS);
    }
}
