//! Validating an attachment and turning it into the transcript type.
//!
//! One implementation for both front ends. The HTTP route reads it from the
//! request body; the terminal reads the file the reader named and calls
//! [`text_attachment`]. The caps, the text rule, and the in-workspace decision
//! are the same either way, which is the point of the module.
//!
//! Nothing here opens a file the caller named: the caller reads the bytes. That
//! is what makes an attachment outside the workspace safe with no grant — there
//! is no server-side read of `path` to confine. An explicit attach is the
//! consent.

use std::path::{Path, PathBuf};

use base64::Engine;
use robi_core::message::FileAttachment;

use crate::domain::error::ServiceError;

/// The most attachments one message may carry, text files and images together.
pub const MAX_ATTACHMENTS: usize = 8;

/// The largest text slice one attachment may carry. Well under `read_file`'s
/// 32 KiB window, because an attachment is inlined into every later request.
pub const MAX_TEXT_FILE_BYTES: usize = 64 * 1024;

/// The sum of every attachment's text on one message.
pub const MAX_TEXT_TOTAL_BYTES: usize = 256 * 1024;

/// The largest image one attachment may carry.
pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// The image types an attachment may be. Anything else is not an image.
pub const SUPPORTED_MEDIA_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];

/// Which refusal an [`AttachmentError`] is, so a caller can pick a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    /// The caller sent something malformed: no name, bad base64, too much.
    BadRequest,
    /// A file is over its byte cap.
    TooLarge,
    /// The bytes are not a text file.
    Unsupported,
}

/// A refused attachment.
///
/// [`std::fmt::Display`] is the bare sentence — `src/x.rs is not a text file` —
/// because the terminal prints it in one line under the composer. The HTTP route
/// picks a status from [`AttachmentError::kind`] instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentError {
    pub kind: AttachmentKind,
    pub message: String,
}

impl AttachmentError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            kind: AttachmentKind::BadRequest,
            message: message.into(),
        }
    }

    fn too_large(message: impl Into<String>) -> Self {
        Self {
            kind: AttachmentKind::TooLarge,
            message: message.into(),
        }
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            kind: AttachmentKind::Unsupported,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for AttachmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<AttachmentError> for ServiceError {
    fn from(error: AttachmentError) -> Self {
        match error.kind {
            AttachmentKind::BadRequest => ServiceError::BadRequest(error.message),
            AttachmentKind::TooLarge => ServiceError::PayloadTooLarge(error.message),
            AttachmentKind::Unsupported => ServiceError::UnsupportedMediaType(error.message),
        }
    }
}

/// One attachment as a caller hands it over, before decoding.
///
/// Not the wire type: `web_api`'s `FileAttachmentInput` is that, and it maps into
/// this one so the rules below have a single caller-facing shape.
#[derive(Debug, Clone)]
pub struct AttachmentInput {
    pub name: String,
    /// The file's absolute path, when the producer knew one. Decides
    /// in-workspace vs outside; never stored and never opened.
    pub absolute_path: Option<String>,
    /// 1-based first line of the slice, when the attach was a range.
    pub start_line: Option<u32>,
    /// 1-based last line of the slice, inclusive.
    pub end_line: Option<u32>,
    /// The file's bytes, standard base64.
    pub content_base64: String,
}

/// A 1-based, inclusive line range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

/// A path with an optional `:start-end` suffix split off it.
///
/// The suffix is recognised only when everything after the **last** colon is
/// digits or digits-dash-digits, so `a:b.rs` and `C:\src\x.rs` keep their colons
/// and a name with no colon is returned whole. `:40` is the single line 40.
pub fn split_range(source: &str) -> (String, Option<LineRange>) {
    let Some((head, tail)) = source.rsplit_once(':') else {
        return (source.to_owned(), None);
    };
    let Some(range) = parse_range(tail) else {
        return (source.to_owned(), None);
    };
    (head.to_owned(), Some(range))
}

fn parse_range(tail: &str) -> Option<LineRange> {
    let digits = |text: &str| -> Option<u32> {
        if text.is_empty() || !text.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        text.parse().ok()
    };
    match tail.split_once('-') {
        Some((start, end)) => Some(LineRange {
            start: digits(start)?,
            end: digits(end)?,
        }),
        None => {
            let line = digits(tail)?;
            Some(LineRange {
                start: line,
                end: line,
            })
        }
    }
}

/// The lines `range` names, clamped to the text, and the range actually taken.
///
/// A tail past the end clamps rather than failing, and the clamped pair comes
/// back so a chip never claims lines the file does not have. A line 0 is line 1:
/// the file has no line 0 and refusing would be a second rule to learn.
pub fn slice_lines(text: &str, range: LineRange) -> (String, LineRange) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return (String::new(), LineRange { start: 1, end: 1 });
    }
    let last = u32::try_from(lines.len()).unwrap_or(u32::MAX);
    let start = range.start.clamp(1, last);
    let end = range.end.clamp(start, last);
    let taken = &lines[(start - 1) as usize..end as usize];
    (taken.join("\n"), LineRange { start, end })
}

/// Validate the client's attachments and turn them into the transcript type.
///
/// The client sends each file's bytes base64-encoded; this decodes them and is
/// the authority on whether the file is text — the name is not consulted. Order:
/// a count cap, an oversized-payload guard, the per-file cap, the total cap, then
/// the content check. A binary file is `Unsupported`, an oversized one
/// `TooLarge`, and the rest `BadRequest`.
pub fn build_files(
    root: Option<PathBuf>,
    inputs: Vec<AttachmentInput>,
) -> Result<Vec<FileAttachment>, AttachmentError> {
    let root = root.as_deref();
    if inputs.len() > MAX_ATTACHMENTS {
        return Err(AttachmentError::bad_request(format!(
            "a message can carry at most {MAX_ATTACHMENTS} files"
        )));
    }
    let mut total = 0usize;
    let mut files = Vec::with_capacity(inputs.len());
    for input in inputs {
        if input.name.trim().is_empty() {
            return Err(AttachmentError::bad_request(
                "a file attachment needs a name",
            ));
        }
        // Guard before decoding: base64 is 4/3 of the bytes, so a payload over
        // twice the cap cannot decode to something within it.
        if input.content_base64.len() > MAX_TEXT_FILE_BYTES * 2 {
            return Err(AttachmentError::too_large(format!(
                "{} is over the {MAX_TEXT_FILE_BYTES} byte limit",
                input.name
            )));
        }
        let bytes = decode_base64(&input.name, &input.content_base64)?;
        if bytes.len() > MAX_TEXT_FILE_BYTES {
            return Err(AttachmentError::too_large(format!(
                "{} is {} bytes, over the {MAX_TEXT_FILE_BYTES} limit",
                input.name,
                bytes.len()
            )));
        }
        total += bytes.len();
        if total > MAX_TEXT_TOTAL_BYTES {
            return Err(AttachmentError::bad_request(format!(
                "the attached files total more than {MAX_TEXT_TOTAL_BYTES} bytes"
            )));
        }
        let text = decode_text(&input.name, bytes)?;
        files.push(FileAttachment {
            name: input.name,
            path: classify_path(input.absolute_path.as_deref(), root),
            start_line: input.start_line,
            end_line: input.end_line,
            text,
        });
    }
    Ok(files)
}

/// One text file the caller has already read and sliced, checked against the cap.
///
/// The terminal's entry point: it resolved the path, read and decoded the bytes,
/// and took the lines it wants. `absolute` decides the attachment's `path`,
/// which is the workspace-relative form when the file is inside the root and
/// `None` when it is not.
///
/// The cap is on **what is attached**, not on the file: a 3,000-line file cannot
/// be attached whole, and `src/app.rs:40-80` of it can. That is what makes a range
/// the answer to the limit rather than a second feature.
pub fn text_attachment(
    name: &str,
    root: &Path,
    absolute: &Path,
    text: String,
    range: Option<LineRange>,
) -> Result<FileAttachment, AttachmentError> {
    if text.len() > MAX_TEXT_FILE_BYTES {
        return Err(AttachmentError::too_large(format!(
            "{name} is {} bytes, over the {MAX_TEXT_FILE_BYTES} limit",
            text.len()
        )));
    }
    let absolute = absolute.to_string_lossy().into_owned();
    Ok(FileAttachment {
        name: name.to_owned(),
        path: classify_path(Some(&absolute), Some(root)),
        start_line: range.map(|range| range.start),
        end_line: range.map(|range| range.end),
        text,
    })
}

/// The workspace-relative path of an attachment, or `None` when it is outside.
///
/// The producer sends the file's absolute path when it knows one; this decides,
/// so no client classifies. Both paths are canonicalized, which resolves
/// symlinks — a link that points out of the workspace is outside. A path that is
/// not under the root, or cannot be resolved (a file this process cannot see, so
/// a browser upload with no path), is outside and keeps only its name. The file's
/// contents are never read.
pub fn classify_path(absolute_path: Option<&str>, root: Option<&Path>) -> Option<String> {
    let absolute_path = absolute_path?;
    let root = std::fs::canonicalize(root?).ok()?;
    let file = std::fs::canonicalize(absolute_path).ok()?;
    let relative = file.strip_prefix(&root).ok()?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    if relative.is_empty() {
        return None;
    }
    Some(relative)
}

/// Base64-decode one attachment's bytes. A body that is not valid base64 is a
/// malformed request.
pub fn decode_base64(name: &str, encoded: &str) -> Result<Vec<u8>, AttachmentError> {
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| {
            AttachmentError::bad_request(format!("{name} is not valid base64: {error}"))
        })
}

/// The text of one attachment, or `Unsupported` when the bytes are not text.
///
/// The same rule the read tools use (`read_file`, `grep`): a NUL byte marks
/// binary, and the bytes must be valid UTF-8. This is the authority — a client
/// may sniff too, for feedback at attach time, but a client that lies is caught
/// here, and no file name or extension is trusted.
pub fn decode_text(name: &str, bytes: Vec<u8>) -> Result<String, AttachmentError> {
    if bytes.contains(&0) {
        return Err(AttachmentError::unsupported(format!(
            "{name} is not a text file"
        )));
    }
    String::from_utf8(bytes)
        .map_err(|_| AttachmentError::unsupported(format!("{name} is not utf-8 text")))
}

/// Magic-byte media type check. No decode, no `image` crate: the signature is
/// enough to catch a mis-typed part before it reaches the provider.
pub fn matches_media_type(media_type: &str, bytes: &[u8]) -> bool {
    match media_type {
        "image/png" => {
            bytes.len() >= 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        }
        "image/jpeg" => {
            bytes.len() >= 3 && bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff
        }
        // WebP starts with "RIFF" + a 4-byte size + "WEBP".
        "image/webp" => bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        // GIF87a or GIF89a.
        "image/gif" => bytes.len() >= 6 && &bytes[0..4] == b"GIF8" && bytes[5] == b'a',
        _ => false,
    }
}

/// The media type `bytes` is, if it is one of the four.
///
/// The terminal's producer has no content type to declare — it read a path — so
/// the bytes are the only evidence, which is the rule the rest of this module
/// already follows.
pub fn detect_media_type(bytes: &[u8]) -> Option<&'static str> {
    SUPPORTED_MEDIA_TYPES
        .iter()
        .copied()
        .find(|media_type| matches_media_type(media_type, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// An outside-workspace attachment: no absolute path.
    fn input(name: &str, text: &str) -> AttachmentInput {
        input_bytes(name, text.as_bytes())
    }

    fn input_bytes(name: &str, bytes: &[u8]) -> AttachmentInput {
        AttachmentInput {
            name: name.to_owned(),
            absolute_path: None,
            start_line: None,
            end_line: None,
            content_base64: base64(bytes),
        }
    }

    /// An attachment from an absolute path; this module classifies it.
    fn at_path(name: &str, absolute_path: &str, text: &str) -> AttachmentInput {
        AttachmentInput {
            name: name.to_owned(),
            absolute_path: Some(absolute_path.to_owned()),
            start_line: None,
            end_line: None,
            content_base64: base64(text.as_bytes()),
        }
    }

    #[test]
    fn build_files_accepts_a_normal_attachment() {
        let files = build_files(None, vec![input("a.rs", "small")]).expect("under every cap");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "a.rs");
        assert_eq!(files[0].text, "small");
    }

    #[test]
    fn build_files_accepts_a_log_file_because_its_bytes_are_text() {
        // The point of the change: the name and extension are not consulted.
        let files = build_files(None, vec![input("server.log", "2026-01-01 boot\n")])
            .expect("a .log is text by content");
        assert_eq!(files[0].text, "2026-01-01 boot\n");
    }

    #[test]
    fn build_files_rejects_a_binary_file() {
        // A NUL byte marks binary, whatever the name says.
        let error =
            build_files(None, vec![input_bytes("not-really.rs", b"\x00\x01\x02")]).unwrap_err();
        assert_eq!(error.kind, AttachmentKind::Unsupported);
        assert_eq!(error.to_string(), "not-really.rs is not a text file");
    }

    #[test]
    fn build_files_rejects_non_utf8_text() {
        let error =
            build_files(None, vec![input_bytes("latin1.txt", b"\xff\xfe\xfa")]).unwrap_err();
        assert_eq!(error.kind, AttachmentKind::Unsupported);
    }

    #[test]
    fn build_files_rejects_bad_base64() {
        let error = build_files(
            None,
            vec![AttachmentInput {
                name: "a.rs".to_owned(),
                absolute_path: None,
                start_line: None,
                end_line: None,
                content_base64: "not base64!!".to_owned(),
            }],
        )
        .unwrap_err();
        assert_eq!(error.kind, AttachmentKind::BadRequest);
    }

    #[test]
    fn build_files_rejects_too_many_files() {
        let inputs = (0..MAX_ATTACHMENTS + 1)
            .map(|index| input(&format!("f{index}.rs"), "x"))
            .collect();
        let error = build_files(None, inputs).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("a message can carry at most {MAX_ATTACHMENTS} files")
        );
    }

    #[test]
    fn build_files_rejects_an_oversized_file() {
        let big = "x".repeat(MAX_TEXT_FILE_BYTES + 1);
        let error = build_files(None, vec![input("big.rs", &big)]).unwrap_err();
        assert_eq!(error.kind, AttachmentKind::TooLarge);
    }

    #[test]
    fn build_files_rejects_a_total_over_the_cap() {
        // Each file is under the per-file cap, but together they exceed the total.
        let chunk = "x".repeat(MAX_TEXT_FILE_BYTES);
        let count = MAX_TEXT_TOTAL_BYTES / MAX_TEXT_FILE_BYTES + 1;
        let inputs = (0..count)
            .map(|index| input(&format!("f{index}.rs"), &chunk))
            .collect();
        let error = build_files(None, inputs).unwrap_err();
        assert_eq!(error.kind, AttachmentKind::BadRequest);
    }

    #[test]
    fn build_files_rejects_a_nameless_file() {
        let error = build_files(None, vec![input("   ", "x")]).unwrap_err();
        assert_eq!(error.to_string(), "a file attachment needs a name");
    }

    /// A canonical temp dir with one nested file, as a workspace root with a file.
    fn workspace_with_file(relative: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("robi-attach-{unique}"));
        let file = root.join(relative);
        std::fs::create_dir_all(file.parent().expect("a parent")).expect("mkdir");
        std::fs::write(&file, "boom").expect("write");
        (root.canonicalize().expect("canonical root"), file)
    }

    #[test]
    fn an_in_workspace_file_keeps_its_relative_path() {
        let (root, file) = workspace_with_file("src/error.rs");
        let files = build_files(
            Some(root.clone()),
            vec![at_path("error.rs", file.to_str().expect("utf-8"), "boom")],
        )
        .expect("a relative path");
        assert_eq!(files[0].path.as_deref(), Some("src/error.rs"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_file_outside_the_root_has_no_path() {
        let (root, _) = workspace_with_file("src/error.rs");
        // A real file that is not under the root.
        let outside = root
            .parent()
            .expect("a parent")
            .join("robi-outside-attach.md");
        std::fs::write(&outside, "hi").expect("write");
        let files = build_files(
            Some(root.clone()),
            vec![at_path(
                "robi-outside-attach.md",
                outside.to_str().expect("utf-8"),
                "hi",
            )],
        )
        .expect("it is text");
        assert_eq!(files[0].path, None, "a path outside the root is outside");
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_attachment_with_no_absolute_path_is_outside() {
        // A browser upload, or a client that sent no path.
        let (root, _) = workspace_with_file("src/error.rs");
        let files =
            build_files(Some(root.clone()), vec![input("server.log", "boot")]).expect("it is text");
        assert_eq!(files[0].path, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_root_means_everything_is_outside() {
        let (root, file) = workspace_with_file("src/error.rs");
        let files = build_files(
            None,
            vec![at_path("error.rs", file.to_str().expect("utf-8"), "boom")],
        )
        .expect("it is text");
        assert_eq!(files[0].path, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_that_leaves_the_root_is_outside() {
        let (root, _) = workspace_with_file("src/error.rs");
        let outside =
            std::env::temp_dir().join(format!("robi-attach-outside-{}", std::process::id()));
        std::fs::write(&outside, "secret").expect("write");
        let link = root.join("leak.md");
        std::os::unix::fs::symlink(&outside, &link).expect("symlink");
        let files = build_files(
            Some(root.clone()),
            vec![at_path("leak.md", link.to_str().expect("utf-8"), "hi")],
        )
        .expect("it is text");
        assert_eq!(files[0].path, None, "a symlink out of the root is outside");
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn text_attachment_classifies_the_path_it_was_given() {
        let (root, file) = workspace_with_file("src/error.rs");
        let attachment = text_attachment("error.rs", &root, &file, "boom".to_owned(), None)
            .expect("a text file");
        assert_eq!(attachment.name, "error.rs");
        assert_eq!(attachment.path.as_deref(), Some("src/error.rs"));
        assert_eq!(attachment.text, "boom");
        assert_eq!(attachment.start_line, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn text_attachment_keeps_the_lines_it_was_given() {
        let (root, file) = workspace_with_file("src/error.rs");
        let attachment = text_attachment(
            "error.rs",
            &root,
            &file,
            "two\nthree".to_owned(),
            Some(LineRange { start: 2, end: 3 }),
        )
        .expect("a slice");
        assert_eq!(attachment.start_line, Some(2));
        assert_eq!(attachment.end_line, Some(3));
        assert_eq!(attachment.text, "two\nthree");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn text_attachment_refuses_an_oversized_slice() {
        let (root, file) = workspace_with_file("src/error.rs");
        let error = text_attachment(
            "big.rs",
            &root,
            &file,
            "x".repeat(MAX_TEXT_FILE_BYTES + 1),
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind, AttachmentKind::TooLarge);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn decode_text_refuses_a_binary_read() {
        let error = decode_text("image.rs", b"\x00\x01".to_vec()).unwrap_err();
        assert_eq!(error.kind, AttachmentKind::Unsupported);
        assert_eq!(error.to_string(), "image.rs is not a text file");
    }

    #[test]
    fn every_supported_media_type_is_detected_from_its_signature() {
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
        assert_eq!(detect_media_type(&png), Some("image/png"));
        let jpeg = [0xff, 0xd8, 0xff, 0xe0];
        assert_eq!(detect_media_type(&jpeg), Some("image/jpeg"));
        let webp = *b"RIFF\x00\x00\x00\x00WEBP";
        assert_eq!(detect_media_type(&webp), Some("image/webp"));
        let gif = *b"GIF89a";
        assert_eq!(detect_media_type(&gif), Some("image/gif"));
        // Text is not an image, whatever it starts with.
        assert_eq!(detect_media_type(b"fn main() {}"), None);
        assert_eq!(detect_media_type(b""), None);
    }

    #[test]
    fn a_range_suffix_is_split_off_the_last_colon_only() {
        assert_eq!(
            split_range("src/app.rs:40-80"),
            (
                "src/app.rs".to_owned(),
                Some(LineRange { start: 40, end: 80 })
            )
        );
        assert_eq!(
            split_range("src/app.rs:40"),
            (
                "src/app.rs".to_owned(),
                Some(LineRange { start: 40, end: 40 })
            )
        );
        // A colon in the name is not a range.
        for source in ["a:b.rs", r"C:\src\x.rs", "notes:final.txt"] {
            assert_eq!(split_range(source), (source.to_owned(), None), "{source}");
        }
        // No colon at all is the whole string.
        assert_eq!(split_range("src/app.rs"), ("src/app.rs".to_owned(), None));
        // A bare range has an empty path, which the caller refuses.
        assert_eq!(
            split_range(":10-20"),
            (String::new(), Some(LineRange { start: 10, end: 20 }))
        );
    }

    #[test]
    fn a_slice_takes_the_lines_it_names_and_clamps_the_tail() {
        let text = "one\ntwo\nthree\nfour";
        let (taken, range) = slice_lines(text, LineRange { start: 2, end: 3 });
        assert_eq!(taken, "two\nthree");
        assert_eq!(range, LineRange { start: 2, end: 3 });

        // A tail past the end clamps and reports itself back.
        let (taken, range) = slice_lines(text, LineRange { start: 3, end: 99 });
        assert_eq!(taken, "three\nfour");
        assert_eq!(range, LineRange { start: 3, end: 4 });

        // A start past the end is the last line, not nothing.
        let (taken, range) = slice_lines(
            text,
            LineRange {
                start: 99,
                end: 120,
            },
        );
        assert_eq!(taken, "four");
        assert_eq!(range, LineRange { start: 4, end: 4 });

        // Line 0 is line 1, and an inverted range is one line.
        let (taken, range) = slice_lines(text, LineRange { start: 0, end: 0 });
        assert_eq!(taken, "one");
        assert_eq!(range, LineRange { start: 1, end: 1 });
        let (taken, _) = slice_lines(text, LineRange { start: 3, end: 2 });
        assert_eq!(taken, "three");

        // One line, and an empty file.
        assert_eq!(
            slice_lines("only", LineRange { start: 1, end: 9 }),
            ("only".to_owned(), LineRange { start: 1, end: 1 })
        );
        assert_eq!(
            slice_lines("", LineRange { start: 5, end: 9 }),
            (String::new(), LineRange { start: 1, end: 1 })
        );
    }
}
