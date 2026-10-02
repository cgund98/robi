//! Assemble a system prompt from ordered blocks.
//!
//! This module renders text. It does not read files. Sources that load
//! instruction files live in `crates/robi`.

/// Marker prefixed when a block is longer than its byte budget.
///
/// The tail is kept, so the most local instructions survive.
pub const TRUNCATION_MARKER: &str = "[earlier instructions truncated]\n";

/// One section of the system prompt, in assembly order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptBlock {
    /// When set, `body` is wrapped in `<tag>…</tag>`.
    pub tag: Option<&'static str>,
    pub body: String,
}

/// Join blocks with a blank line. An empty body is omitted.
pub fn render(blocks: &[PromptBlock]) -> String {
    let mut parts = Vec::new();
    for block in blocks {
        let body = block.body.trim();
        if body.is_empty() {
            continue;
        }
        parts.push(match block.tag {
            Some(tag) => format!("<{tag}>\n{body}\n</{tag}>"),
            None => body.to_owned(),
        });
    }
    if parts.is_empty() {
        return String::new();
    }
    parts.join("\n\n") + "\n"
}

/// Keep the end of `text` when it is longer than `max_bytes`.
///
/// The cut is moved forward to a character boundary so the result stays valid
/// UTF-8. A budget smaller than the truncation marker returns the marker alone.
pub fn keep_tail(text: &str, max_bytes: usize) -> String {
    if max_bytes == 0 || text.len() <= max_bytes {
        return text.to_owned();
    }
    if TRUNCATION_MARKER.len() >= max_bytes {
        return TRUNCATION_MARKER.to_owned();
    }
    let budget = max_bytes - TRUNCATION_MARKER.len();
    let mut start = text.len().saturating_sub(budget);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    format!("{TRUNCATION_MARKER}{}", &text[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_wraps_tagged_blocks_and_skips_empty_ones() {
        let text = render(&[
            PromptBlock {
                tag: None,
                body: "  built in  ".into(),
            },
            PromptBlock {
                tag: Some("user_prompt"),
                body: "\n".into(),
            },
            PromptBlock {
                tag: Some("user_agents"),
                body: "global".into(),
            },
        ]);
        assert_eq!(text, "built in\n\n<user_agents>\nglobal\n</user_agents>\n");
    }

    #[test]
    fn keep_tail_keeps_the_end_and_a_character_boundary() {
        let text = format!("HEAD {} TAIL", "é".repeat(40));
        let kept = keep_tail(&text, TRUNCATION_MARKER.len() + 8);
        assert!(kept.starts_with(TRUNCATION_MARKER));
        assert!(kept.ends_with("TAIL"));
        assert!(kept.len() <= TRUNCATION_MARKER.len() + 8);
    }
}
