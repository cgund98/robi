//! Render a user's file attachments for the provider.
//!
//! The transcript holds the attached text (`FileAttachment`), and each adapter
//! appends one block per attachment to the text of the user turn. This mirrors
//! [`skill_block`](crate::agent::skills::skill_block): the core renders the
//! message, the adapter renders the block, and the block is text so the provider
//! reads it like any other part of the prompt.

use robi_core::message::FileAttachment;

/// The provider block appended after the typed user text.
///
/// `name` is always present. A file inside the workspace carries its
/// **workspace-relative** `path`, so the model can re-read it with `read_file`.
/// A file outside the workspace carries `origin="outside-workspace"` instead —
/// the model is told it cannot re-fetch that one. `lines` is omitted for a
/// whole-file attach; when present it is the 1-based inclusive range the slice
/// came from.
pub fn file_block(file: &FileAttachment) -> String {
    let mut attrs = format!("name=\"{}\"", file.name);
    match &file.path {
        Some(path) => attrs.push_str(&format!(" path=\"{path}\"")),
        None => attrs.push_str(" origin=\"outside-workspace\""),
    }
    if let Some(lines) = lines_attr(file) {
        attrs.push_str(&format!(" lines=\"{lines}\""));
    }
    format!(
        "\n<file {attrs}>\n{}\n</file>",
        file.text.trim_end_matches('\n')
    )
}

/// The `lines` value for a ranged attach, or `None` for a whole file.
fn lines_attr(file: &FileAttachment) -> Option<String> {
    match (file.start_line, file.end_line) {
        (Some(start), Some(end)) => Some(format!("{start}-{end}")),
        (Some(start), None) => Some(start.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(
        name: &str,
        path: Option<&str>,
        start_line: Option<u32>,
        end_line: Option<u32>,
    ) -> FileAttachment {
        FileAttachment {
            name: name.to_owned(),
            path: path.map(str::to_owned),
            start_line,
            end_line,
            text: "line one\nline two\n".to_owned(),
        }
    }

    #[test]
    fn a_ranged_workspace_file_carries_path_and_lines() {
        let block = file_block(&file("error.rs", Some("src/error.rs"), Some(29), Some(34)));
        assert_eq!(
            block,
            "\n<file name=\"error.rs\" path=\"src/error.rs\" lines=\"29-34\">\n\
             line one\nline two\n</file>"
        );
    }

    #[test]
    fn an_outside_file_is_marked_as_such_and_omits_the_path() {
        let block = file_block(&file("notes.txt", None, None, None));
        assert_eq!(
            block,
            "\n<file name=\"notes.txt\" origin=\"outside-workspace\">\nline one\nline two\n</file>"
        );
    }

    #[test]
    fn a_whole_workspace_file_keeps_path_but_omits_lines() {
        let block = file_block(&file("README.md", Some("README.md"), None, None));
        assert_eq!(
            block,
            "\n<file name=\"README.md\" path=\"README.md\">\n\
             line one\nline two\n</file>"
        );
    }
}
