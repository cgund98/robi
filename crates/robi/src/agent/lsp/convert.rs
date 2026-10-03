//! Turn language-server positions into the paths and columns the model reads.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use async_lsp::lsp_types::{
    Diagnostic, DiagnosticSeverity, Location, LocationLink, NumberOrString, Position, Url,
};
use serde_json::{json, Value};

use crate::agent::workspace::{workspace_relative, PathFilter};

use super::position::{column_from_utf16, line_text};

const PREVIEW_CHARS: usize = 300;
const MESSAGE_CHARS: usize = 500;
const RELATED_LIMIT: usize = 3;

pub struct Point {
    pub absolute: PathBuf,
    pub line: u32,
    pub character: u32,
}

pub fn points_from_locations(locations: Vec<Location>) -> Vec<Point> {
    locations
        .into_iter()
        .filter_map(|location| point(location.uri, location.range.start))
        .collect()
}

pub fn points_from_links(links: Vec<LocationLink>) -> Vec<Point> {
    links
        .into_iter()
        .filter_map(|link| point(link.target_uri, link.target_selection_range.start))
        .collect()
}

fn point(uri: Url, position: Position) -> Option<Point> {
    let absolute = uri.to_file_path().ok()?;
    Some(Point {
        absolute,
        line: position.line,
        character: position.character,
    })
}

/// Locations the session may read, capped. `truncated` is true when the cap cut the list.
pub fn location_hits(
    points: &[Point],
    root: &Path,
    filter: &PathFilter,
    limit: usize,
) -> (Vec<Value>, bool) {
    let mut files = HashMap::<PathBuf, String>::new();
    let mut hits = Vec::new();
    let mut skipped = 0usize;
    for point in points {
        let Some(relative) = readable(root, filter, &point.absolute) else {
            continue;
        };
        if hits.len() >= limit {
            skipped += 1;
            continue;
        }
        let text = files
            .entry(point.absolute.clone())
            .or_insert_with(|| fs::read_to_string(&point.absolute).unwrap_or_default());
        let line_body = line_text(text, point.line + 1).unwrap_or("");
        hits.push(json!({
            "path": relative,
            "line": point.line + 1,
            "character": column_from_utf16(line_body, point.character),
            "preview": clip_chars(line_body, PREVIEW_CHARS),
        }));
    }
    (hits, skipped > 0)
}

pub fn diagnostic_items(
    items: &[Diagnostic],
    file: &Path,
    root: &Path,
    filter: &PathFilter,
    limit: usize,
) -> (Vec<Value>, bool) {
    let mut kept: Vec<&Diagnostic> = items
        .iter()
        .filter(|item| matches!(item.severity, Some(severity) if severity == DiagnosticSeverity::ERROR || severity == DiagnosticSeverity::WARNING) || item.severity.is_none())
        .collect();
    kept.sort_by_key(|item| {
        let rank = match item.severity {
            Some(severity) if severity == DiagnosticSeverity::ERROR => 0,
            _ => 1,
        };
        (rank, item.range.start.line, item.range.start.character)
    });
    let truncated = kept.len() > limit;
    let kept = kept.into_iter().take(limit);
    let mut files = HashMap::<PathBuf, String>::new();
    let diagnostics = kept
        .map(|item| {
            let line_body = line_of(&mut files, file, item.range.start);
            let mut value = json!({
                "severity": severity_name(item.severity),
                "line": item.range.start.line + 1,
                "character": column_from_utf16(&line_body, item.range.start.character),
                "message": clip_chars(&item.message, MESSAGE_CHARS),
            });
            if let Some(source) = &item.source {
                value["source"] = json!(source);
            }
            if let Some(code) = item.code.as_ref().map(code_string) {
                value["code"] = json!(code);
            }
            if let Some(related) = &item.related_information {
                let related: Vec<Value> = related
                    .iter()
                    .filter_map(|info| {
                        let path = info.location.uri.to_file_path().ok()?;
                        let relative = readable(root, filter, &path)?;
                        let body = line_of(&mut files, &path, info.location.range.start);
                        Some(json!({
                            "path": relative,
                            "line": info.location.range.start.line + 1,
                            "character": column_from_utf16(&body, info.location.range.start.character),
                            "message": clip_chars(&info.message, MESSAGE_CHARS),
                        }))
                    })
                    .take(RELATED_LIMIT)
                    .collect();
                if !related.is_empty() {
                    value["related"] = json!(related);
                }
            }
            value
        })
        .collect();
    (diagnostics, truncated)
}

fn line_of(files: &mut HashMap<PathBuf, String>, path: &Path, position: Position) -> String {
    let text = files
        .entry(path.to_path_buf())
        .or_insert_with(|| fs::read_to_string(path).unwrap_or_default());
    line_text(text, position.line + 1).unwrap_or("").to_owned()
}

fn readable(root: &Path, filter: &PathFilter, absolute: &Path) -> Option<String> {
    let relative = workspace_relative(root, absolute);
    if relative == ".." || relative.starts_with("../") {
        return None;
    }
    if !filter.allows_read(&relative) {
        return None;
    }
    Some(if relative.is_empty() {
        ".".to_owned()
    } else {
        relative
    })
}

fn severity_name(severity: Option<DiagnosticSeverity>) -> &'static str {
    match severity {
        Some(severity) if severity == DiagnosticSeverity::ERROR => "error",
        Some(severity) if severity == DiagnosticSeverity::WARNING => "warning",
        _ => "warning",
    }
}

fn code_string(code: &NumberOrString) -> String {
    match code {
        NumberOrString::Number(number) => number.to_string(),
        NumberOrString::String(text) => text.clone(),
    }
}

fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    text.chars().take(max).collect()
}

/// 1-based Unicode scalar column for an LSP UTF-16 offset in a file on disk.
pub fn scalar_column(absolute: &Path, line: u32, utf16: u32) -> u32 {
    let text = fs::read_to_string(absolute).unwrap_or_default();
    let body = line_text(&text, line + 1).unwrap_or("");
    column_from_utf16(body, utf16)
}

pub fn symbol_kind_name(kind: async_lsp::lsp_types::SymbolKind) -> &'static str {
    use async_lsp::lsp_types::SymbolKind;
    match kind {
        SymbolKind::FILE => "file",
        SymbolKind::MODULE => "module",
        SymbolKind::NAMESPACE => "namespace",
        SymbolKind::PACKAGE => "package",
        SymbolKind::CLASS => "class",
        SymbolKind::METHOD => "method",
        SymbolKind::PROPERTY => "property",
        SymbolKind::FIELD => "field",
        SymbolKind::CONSTRUCTOR => "constructor",
        SymbolKind::ENUM => "enum",
        SymbolKind::INTERFACE => "interface",
        SymbolKind::FUNCTION => "function",
        SymbolKind::VARIABLE => "variable",
        SymbolKind::CONSTANT => "constant",
        SymbolKind::STRING => "string",
        SymbolKind::NUMBER => "number",
        SymbolKind::BOOLEAN => "boolean",
        SymbolKind::ARRAY => "array",
        SymbolKind::OBJECT => "object",
        SymbolKind::KEY => "key",
        SymbolKind::NULL => "null",
        SymbolKind::ENUM_MEMBER => "enum_member",
        SymbolKind::STRUCT => "struct",
        SymbolKind::EVENT => "event",
        SymbolKind::OPERATOR => "operator",
        SymbolKind::TYPE_PARAMETER => "type_parameter",
        _ => "unknown",
    }
}
