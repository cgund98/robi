//! Fold a source buffer to signatures plus the bodies a caller named.
//!
//! The function does not read the filesystem. The tool owns the file.

use std::collections::HashSet;
use std::ops::Range;

use sha2::{Digest, Sha256};
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};

use crate::chunk::{grammar, symbol_of, Language};

const MAX_OUTLINE_BYTES: usize = 32 * 1024;
const IMPORT_LINE_LIMIT: u32 = 15;
const PARAM_BYTES: usize = 80;

#[derive(Debug)]
pub struct OutlineError {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Omitted {
    pub symbol: String,
    pub start_line: u32,
    pub end_line: u32,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub symbol: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ambiguous {
    pub focus: String,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    pub content: String,
    pub total_lines: u32,
    pub truncated: bool,
    pub depth_applied: u8,
    pub focused: Vec<String>,
    pub omitted: Vec<Omitted>,
    pub missing: Vec<String>,
    pub ambiguous: Vec<Ambiguous>,
    pub hint: Option<String>,
    pub file_sha256: String,
}

struct Captured<'tree> {
    item: Node<'tree>,
    body: Option<Node<'tree>>,
}

struct Span {
    symbol: String,
    params: String,
    kind: &'static str,
    node: Range<usize>,
    body: Option<Range<usize>>,
    parent: Option<usize>,
}

#[derive(Clone)]
struct Fold {
    body: Range<usize>,
    symbol: String,
}

/// Render `source` as an outline. `depth` is clamped by the caller to 0..=2.
pub fn outline(
    source: &str,
    language: Language,
    focus: &[String],
    depth: u8,
    expand_imports: bool,
) -> Result<Outline, OutlineError> {
    if matches!(language, Language::Markdown) {
        return Err(OutlineError {
            message: "no grammar".into(),
        });
    }
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&grammar(language))
        .map_err(|err| OutlineError {
            message: format!("set language: {err}"),
        })?;
    let tree = parser.parse(source, None).ok_or_else(|| OutlineError {
        message: "file did not parse; use read_file".into(),
    })?;
    if tree.root_node().has_error() {
        return Err(OutlineError {
            message: "file did not parse; use read_file".into(),
        });
    }
    let query =
        Query::new(&grammar(language), query_source(language)).map_err(|err| OutlineError {
            message: format!("compile query: {err}"),
        })?;
    let captured = capture(&query, tree.root_node(), source.as_bytes());
    let mut spans = spans_of(language, source, &captured);
    assign_parents(&mut spans);
    let (direct_hit, missing, ambiguous) = match_focus(source, &spans, focus, language.separator());
    let file_sha256 = hex_encode(&Sha256::digest(source.as_bytes()));
    let total_lines = total_lines(source);
    let imports = import_fold(language, tree.root_node(), source, expand_imports);

    let mut depth_applied = depth.min(2);
    let mut rendered = render_depth(source, &spans, &direct_hit, depth_applied, imports.clone());
    while rendered.content.len() > MAX_OUTLINE_BYTES && depth_applied > 0 {
        depth_applied -= 1;
        rendered = render_depth(source, &spans, &direct_hit, depth_applied, imports.clone());
    }

    let mut truncated = false;
    let mut hint = None;
    if rendered.content.len() > MAX_OUTLINE_BYTES {
        let forced =
            render_depth_forcing_focus(source, &spans, &direct_hit, depth_applied, imports.clone());
        let start = forced
            .omitted
            .iter()
            .find(|item| direct_symbol(&spans, &direct_hit, &item.symbol))
            .map(|item| item.start_line);
        rendered = forced;
        truncated = true;
        if let Some(start) = start {
            hint = Some(format!("read_file at start_line {start}"));
        }
    }
    if rendered.content.len() > MAX_OUTLINE_BYTES {
        rendered = cut_at_marker(rendered);
        truncated = true;
        hint = Some("pass a narrower focus_symbols or read_file a line range".into());
    }

    let focused = focused_symbols(&spans, &direct_hit, &rendered.folds);
    Ok(Outline {
        content: rendered.content,
        total_lines,
        truncated,
        depth_applied,
        focused,
        omitted: rendered.omitted,
        missing,
        ambiguous,
        hint,
        file_sha256,
    })
}

struct Rendered {
    content: String,
    omitted: Vec<Omitted>,
    folds: Vec<Range<usize>>,
}

fn render_depth(
    source: &str,
    spans: &[Span],
    direct_hit: &[bool],
    depth: u8,
    imports: Option<Fold>,
) -> Rendered {
    let eligible = eligibility(spans, direct_hit, depth);
    let indexes = select(spans, direct_hit, &eligible);
    let mut folds = folds_from(spans, &indexes);
    if let Some(imports) = imports {
        folds.push(imports);
        folds.sort_by_key(|fold| fold.body.start);
        folds = drop_nested(folds);
    }
    render(source, &folds)
}

fn render_depth_forcing_focus(
    source: &str,
    spans: &[Span],
    direct_hit: &[bool],
    depth: u8,
    imports: Option<Fold>,
) -> Rendered {
    let eligible = eligibility(spans, direct_hit, depth);
    let mut indexes = select(spans, direct_hit, &eligible);
    for (index, span) in spans.iter().enumerate() {
        if direct_hit[index] && span.body.is_some() {
            indexes.push(index);
        }
    }
    let mut folds = folds_from(spans, &indexes);
    if let Some(imports) = imports {
        folds.push(imports);
    }
    folds.sort_by_key(|fold| fold.body.start);
    render(source, &drop_nested(folds))
}

fn direct_symbol(spans: &[Span], direct_hit: &[bool], symbol: &str) -> bool {
    spans
        .iter()
        .enumerate()
        .any(|(index, span)| direct_hit[index] && span.symbol == symbol)
}

fn folds_from(spans: &[Span], indexes: &[usize]) -> Vec<Fold> {
    indexes
        .iter()
        .filter_map(|index| {
            let span = &spans[*index];
            span.body.clone().map(|body| Fold {
                body,
                symbol: span.symbol.clone(),
            })
        })
        .collect()
}

fn drop_nested(mut folds: Vec<Fold>) -> Vec<Fold> {
    folds.sort_by_key(|fold| fold.body.start);
    let mut kept: Vec<Fold> = Vec::new();
    for fold in folds {
        if kept
            .iter()
            .any(|prior| prior.body.start <= fold.body.start && fold.body.end <= prior.body.end)
        {
            continue;
        }
        kept.push(fold);
    }
    kept
}

fn focused_symbols(spans: &[Span], direct_hit: &[bool], folds: &[Range<usize>]) -> Vec<String> {
    let mut names = Vec::new();
    for (index, span) in spans.iter().enumerate() {
        if !direct_hit[index] {
            continue;
        }
        let Some(body) = &span.body else {
            continue;
        };
        let covered = folds
            .iter()
            .any(|fold| fold.start <= body.start && body.end <= fold.end);
        if !covered {
            names.push(span.symbol.clone());
        }
    }
    names
}

fn eligibility(spans: &[Span], direct_hit: &[bool], depth: u8) -> Vec<bool> {
    spans
        .iter()
        .enumerate()
        .map(|(index, span)| {
            if direct_hit[index] || span.body.is_none() {
                return false;
            }
            match depth {
                0 => true,
                1 => matches!(
                    span.kind,
                    "function" | "method" | "struct" | "enum" | "interface"
                ),
                _ => {
                    matches!(span.kind, "function" | "method")
                        && span.parent.is_some_and(|parent| {
                            matches!(spans[parent].kind, "function" | "method")
                        })
                }
            }
        })
        .collect()
}

fn select(spans: &[Span], direct_hit: &[bool], eligible: &[bool]) -> Vec<usize> {
    let mut covered = direct_hit.to_vec();
    for index in 0..spans.len() {
        if let Some(parent) = spans[index].parent {
            if covered[parent] {
                covered[index] = true;
            }
        }
    }
    let mut omitted_ancestor = vec![false; spans.len()];
    let mut folds = Vec::new();
    for index in 0..spans.len() {
        if let Some(parent) = spans[index].parent {
            if omitted_ancestor[parent] {
                omitted_ancestor[index] = true;
                continue;
            }
        }
        if covered[index] || !eligible[index] {
            continue;
        }
        let holds_focus = spans.iter().enumerate().any(|(child, span)| {
            child != index
                && direct_hit[child]
                && spans[index]
                    .body
                    .as_ref()
                    .is_some_and(|body| body.contains(&span.node.start))
        });
        if holds_focus {
            continue;
        }
        omitted_ancestor[index] = true;
        folds.push(index);
    }
    folds
}

fn render(source: &str, folds: &[Fold]) -> Rendered {
    let mut out = String::with_capacity(source.len() / 4);
    let mut cursor = 0usize;
    let mut omitted = Vec::new();
    let mut ranges = Vec::new();
    let mut hashes = HashSet::new();
    for fold in folds {
        if fold.body.start < cursor || fold.body.end > source.len() {
            continue;
        }
        out.push_str(&source[cursor..fold.body.start]);
        let indent = indent_of(source, fold.body.start);
        let (start_line, end_line) = line_span(source, &fold.body);
        let hash = short_hash(&source.as_bytes()[fold.body.clone()], &mut hashes);
        let marker = format!(
            "<<<ROBI_OMITTED symbol={} lines={start_line}-{end_line} sha256=\"{hash}\">>>",
            escape_json(&fold.symbol)
        );
        out.push('\n');
        out.push_str(indent);
        out.push_str(&marker);
        cursor = fold.body.end;
        omitted.push(Omitted {
            symbol: fold.symbol.clone(),
            start_line,
            end_line,
            sha256: hash,
        });
        ranges.push(fold.body.clone());
    }
    out.push_str(&source[cursor..]);
    Rendered {
        content: out,
        omitted,
        folds: ranges,
    }
}

fn cut_at_marker(rendered: Rendered) -> Rendered {
    if rendered.content.len() <= MAX_OUTLINE_BYTES {
        return rendered;
    }
    let mut last_end = None;
    let mut search = 0usize;
    let content = &rendered.content;
    while let Some(rel) = content[search..].find("<<<ROBI_OMITTED") {
        let start = search + rel;
        let Some(end_rel) = content[start..].find(">>>") else {
            break;
        };
        let end = start + end_rel + 3;
        if end > MAX_OUTLINE_BYTES {
            break;
        }
        last_end = Some(end);
        search = end;
    }
    let cut = last_end.unwrap_or_else(|| {
        let mut end = MAX_OUTLINE_BYTES.min(content.len());
        if let Some(marker) = content[..end].rfind("<<<ROBI_") {
            end = marker;
        }
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        end
    });
    let content = content[..cut].to_owned();
    let kept = content.matches("<<<ROBI_OMITTED").count();
    Rendered {
        content,
        omitted: rendered.omitted.into_iter().take(kept).collect(),
        folds: rendered.folds.into_iter().take(kept).collect(),
    }
}

fn capture<'tree>(query: &Query, root: Node<'tree>, source: &[u8]) -> Vec<Captured<'tree>> {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, source);
    let mut out = Vec::new();
    let Some(item_ix) = query.capture_index_for_name("item") else {
        return out;
    };
    let body_ix = query.capture_index_for_name("body");
    while let Some(matched) = matches.next() {
        let Some(item) = matched.nodes_for_capture_index(item_ix).next() else {
            continue;
        };
        let body = body_ix.and_then(|ix| matched.nodes_for_capture_index(ix).next());
        out.push(Captured { item, body });
    }
    out
}

fn spans_of(language: Language, source: &str, captured: &[Captured<'_>]) -> Vec<Span> {
    let mut spans: Vec<Span> = captured
        .iter()
        .map(|cap| Span {
            symbol: symbol_of(language, cap.item, source),
            params: params_of(cap.item, source),
            kind: span_kind(cap.item.kind()),
            node: cap.item.byte_range(),
            body: cap.body.map(|node| node.byte_range()),
            parent: None,
        })
        .collect();
    spans.sort_by(|left, right| {
        left.node
            .start
            .cmp(&right.node.start)
            .then(right.node.end.cmp(&left.node.end))
    });
    spans.dedup_by(|right, left| right.node == left.node && right.body == left.body);
    spans
}

fn assign_parents(spans: &mut [Span]) {
    for index in 0..spans.len() {
        let node = spans[index].node.clone();
        let mut parent = None;
        for earlier in (0..index).rev() {
            if let Some(body) = &spans[earlier].body {
                if body.start <= node.start && node.end <= body.end {
                    parent = Some(earlier);
                    break;
                }
            }
        }
        spans[index].parent = parent;
    }
}

fn match_focus(
    source: &str,
    spans: &[Span],
    focus: &[String],
    separator: &str,
) -> (Vec<bool>, Vec<String>, Vec<Ambiguous>) {
    let mut direct = vec![false; spans.len()];
    let mut missing = Vec::new();
    let mut ambiguous = Vec::new();
    for raw in focus {
        let query = prepare_focus(raw);
        if query.is_empty() {
            missing.push(raw.clone());
            continue;
        }
        match resolve_one(source, spans, &query, separator) {
            FocusMatch::None => missing.push(raw.clone()),
            FocusMatch::One(index) => direct[index] = true,
            FocusMatch::Many(candidates) => ambiguous.push(Ambiguous {
                focus: raw.clone(),
                candidates,
            }),
        }
    }
    (direct, missing, ambiguous)
}

enum FocusMatch {
    None,
    One(usize),
    Many(Vec<Candidate>),
}

fn resolve_one(source: &str, spans: &[Span], query: &str, separator: &str) -> FocusMatch {
    let stage1: Vec<usize> = spans
        .iter()
        .enumerate()
        .filter(|(_, span)| span.symbol == query)
        .map(|(index, _)| index)
        .collect();
    if let Some(found) = unique_or_many(source, spans, stage1) {
        return found;
    }
    if query.contains('(') {
        let stage2: Vec<usize> = spans
            .iter()
            .enumerate()
            .filter(|(_, span)| display_symbol(span) == query)
            .map(|(index, _)| index)
            .collect();
        if let Some(found) = unique_or_many(source, spans, stage2) {
            return found;
        }
    }
    if wrong_separator(query, separator) {
        return FocusMatch::None;
    }
    let needle = query.rsplit(separator).next().unwrap_or(query);
    let stage3: Vec<usize> = spans
        .iter()
        .enumerate()
        .filter(|(_, span)| {
            span.symbol
                .rsplit(separator)
                .next()
                .unwrap_or(span.symbol.as_str())
                == needle
        })
        .map(|(index, _)| index)
        .collect();
    unique_or_many(source, spans, stage3).unwrap_or(FocusMatch::None)
}

fn unique_or_many(source: &str, spans: &[Span], hits: Vec<usize>) -> Option<FocusMatch> {
    match hits.len() {
        0 => None,
        1 => Some(FocusMatch::One(hits[0])),
        _ => Some(FocusMatch::Many(
            hits.into_iter()
                .map(|index| {
                    let span = &spans[index];
                    let (start_line, end_line) = line_span(source, &span.node);
                    Candidate {
                        symbol: display_symbol(span),
                        start_line,
                        end_line,
                    }
                })
                .collect(),
        )),
    }
}

fn display_symbol(span: &Span) -> String {
    if span.params.is_empty() {
        span.symbol.clone()
    } else if span.params.starts_with('(') {
        format!("{}{}", span.symbol, span.params)
    } else {
        format!("{}({})", span.symbol, span.params)
    }
}

fn prepare_focus(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(stripped) = trimmed.strip_suffix("()") {
        if !stripped.contains('(') {
            return stripped.trim().to_owned();
        }
    }
    trimmed.to_owned()
}

fn wrong_separator(query: &str, separator: &str) -> bool {
    let other = if separator == "::" { "." } else { "::" };
    query.contains(other)
}

fn params_of(node: Node<'_>, source: &str) -> String {
    let Some(params) = node.child_by_field_name("parameters") else {
        return String::new();
    };
    let text = params.utf8_text(source.as_bytes()).unwrap_or("");
    collapse_ws(text)
}

fn collapse_ws(text: &str) -> String {
    let mut out = String::new();
    let mut pending = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            pending = !out.is_empty();
            continue;
        }
        if pending {
            out.push(' ');
            pending = false;
        }
        out.push(ch);
        if out.len() >= PARAM_BYTES {
            break;
        }
    }
    let mut end = out.len().min(PARAM_BYTES);
    while end > 0 && !out.is_char_boundary(end) {
        end -= 1;
    }
    out.truncate(end);
    out
}

fn span_kind(kind: &str) -> &'static str {
    match kind {
        "function_item"
        | "function_declaration"
        | "function_definition"
        | "variable_declarator" => "function",
        "method_definition" | "method_declaration" => "method",
        "struct_item" => "struct",
        "enum_item" => "enum",
        "interface_declaration" => "interface",
        "trait_item" => "trait",
        "impl_item" => "impl",
        "class_declaration" | "class_definition" => "class",
        "mod_item" => "module",
        "function_signature_item" => "signature",
        "const_item" => "const",
        "static_item" => "static",
        "type_declaration" => "type",
        _ => "item",
    }
}

fn import_fold(
    language: Language,
    root: Node<'_>,
    source: &str,
    expand_imports: bool,
) -> Option<Fold> {
    if expand_imports {
        return None;
    }
    let kinds = language.import_kinds();
    let mut cursor = root.walk();
    let mut first = None;
    let mut last_end = 0usize;
    let mut started = false;
    for child in root.named_children(&mut cursor) {
        let kind = child.kind();
        if kinds.contains(&kind) {
            if first.is_none() {
                first = Some(child.start_byte());
            }
            last_end = child.end_byte();
            started = true;
            continue;
        }
        if started {
            break;
        }
        if matches!(
            kind,
            "package_clause" | "comment" | "attribute_item" | "inner_attribute_item"
        ) {
            continue;
        }
    }
    let start = first?;
    let body = start..last_end;
    let (start_line, end_line) = line_span(source, &body);
    if end_line.saturating_sub(start_line) < IMPORT_LINE_LIMIT {
        return None;
    }
    Some(Fold {
        body,
        symbol: "imports".into(),
    })
}

fn indent_of(source: &str, body_start: usize) -> &str {
    let line_start = source[..body_start]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let line = &source[line_start..body_start];
    let end = line
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(index, _)| index)
        .unwrap_or(line.len());
    &line[..end]
}

fn line_span(source: &str, range: &Range<usize>) -> (u32, u32) {
    let start = line_at(source, range.start);
    let end = if range.end <= range.start {
        start
    } else {
        line_at(source, range.end - 1)
    };
    (start, end)
}

fn line_at(source: &str, byte: usize) -> u32 {
    source[..byte.min(source.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count() as u32
        + 1
}

fn total_lines(source: &str) -> u32 {
    if source.is_empty() {
        return 1;
    }
    let newlines = source.bytes().filter(|byte| *byte == b'\n').count() as u32;
    if source.ends_with('\n') {
        newlines
    } else {
        newlines + 1
    }
}

fn short_hash(bytes: &[u8], taken: &mut HashSet<String>) -> String {
    let digest = Sha256::digest(bytes);
    let mut width = 8usize;
    loop {
        let hex = hex_encode(&digest[..width.min(digest.len())]);
        if taken.insert(hex.clone()) || width >= digest.len() {
            return hex;
        }
        width += 1;
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn escape_json(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if other.is_control() => {
                out.push_str(&format!("\\u{:04x}", other as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn query_source(language: Language) -> &'static str {
    match language {
        Language::Rust => RUST_QUERY,
        Language::TypeScript | Language::Tsx => TYPESCRIPT_QUERY,
        Language::JavaScript => JAVASCRIPT_QUERY,
        Language::Python => PYTHON_QUERY,
        Language::Go => GO_QUERY,
        Language::Markdown => "",
    }
}

const RUST_QUERY: &str = r#"
(function_item name: (_) @name body: (_) @body) @item
(function_signature_item name: (_) @name) @item
(struct_item name: (_) @name body: (_) @body) @item
(enum_item name: (_) @name body: (_) @body) @item
(trait_item name: (_) @name body: (_) @body) @item
(impl_item body: (_) @body) @item
(mod_item name: (_) @name body: (_) @body) @item
(const_item name: (_) @name) @item
(static_item name: (_) @name) @item
"#;

const TYPESCRIPT_QUERY: &str = r#"
(function_declaration name: (_) @name body: (_) @body) @item
(method_definition name: (_) @name body: (_) @body) @item
(class_declaration name: (_) @name body: (_) @body) @item
(interface_declaration name: (_) @name body: (_) @body) @item
(variable_declarator name: (identifier) @name value: (arrow_function body: (_) @body)) @item
"#;

const JAVASCRIPT_QUERY: &str = r#"
(function_declaration name: (_) @name body: (_) @body) @item
(method_definition name: (_) @name body: (_) @body) @item
(class_declaration name: (_) @name body: (_) @body) @item
(variable_declarator name: (identifier) @name value: (arrow_function body: (_) @body)) @item
"#;

const PYTHON_QUERY: &str = r#"
(function_definition name: (_) @name body: (_) @body) @item
(class_definition name: (_) @name body: (_) @body) @item
"#;

const GO_QUERY: &str = r#"
(function_declaration name: (_) @name body: (_) @body) @item
(method_declaration name: (_) @name body: (_) @body) @item
(type_declaration (type_spec name: (_) @name)) @item
"#;

#[cfg(test)]
mod tests {
    use super::*;

    const PAY: &str = r#"impl PaymentService {
    pub fn process_card(&self, card: &Card) -> Result<Receipt, PayError> {
        let charge = self.gateway.charge(card)?;
        self.ledger.append(charge);
        Ok(Receipt::from(charge))
    }

    fn validate_expiry(&self, card: &Card) -> Result<(), PayError> {
        if card.expired() {
            return Err(PayError::Expired);
        }
        Ok(())
    }
}
"#;

    #[test]
    fn focus_keeps_one_method_and_marks_the_sibling() {
        let outline = outline(
            PAY,
            Language::Rust,
            &["PaymentService::process_card".into()],
            1,
            false,
        )
        .unwrap();
        assert!(outline
            .content
            .contains("let charge = self.gateway.charge(card)?;"));
        assert!(outline.content.contains("fn validate_expiry"));
        assert!(outline.content.contains("<<<ROBI_OMITTED"));
        assert!(!outline.content.contains("card.expired()"));
        assert_eq!(outline.focused, vec!["PaymentService::process_card"]);
        assert_eq!(outline.omitted.len(), 1);
        assert_eq!(outline.omitted[0].symbol, "PaymentService::validate_expiry");
        assert!(outline
            .content
            .contains(&format!("sha256=\"{}\"", outline.omitted[0].sha256)));
        assert_eq!(outline.omitted[0].sha256.len(), 16);
        assert!(outline.missing.is_empty());
        assert!(!outline.truncated);
    }

    #[test]
    fn depth_zero_folds_the_impl_until_focus_holds_it_open() {
        let folded = outline(PAY, Language::Rust, &[], 0, false).unwrap();
        assert_eq!(folded.omitted.len(), 1);
        assert!(folded.content.contains("impl PaymentService"));
        assert!(!folded.content.contains("process_card"));

        let open = outline(PAY, Language::Rust, &["validate_expiry".into()], 0, false).unwrap();
        assert!(open.content.contains("card.expired()"));
        assert!(open
            .focused
            .iter()
            .any(|name| name.ends_with("validate_expiry")));
        assert!(open
            .omitted
            .iter()
            .any(|item| item.symbol.ends_with("process_card")));
    }

    #[test]
    fn two_overloads_are_ambiguous_until_the_parameter_form() {
        let source = "function process_card(card: Card) {\n  return card;\n}\nfunction process_card(card: Card, pin: string) {\n  return pin;\n}\n";
        let ambiguous = outline(
            source,
            Language::TypeScript,
            &["process_card".into()],
            1,
            false,
        )
        .unwrap();
        assert!(ambiguous.focused.is_empty());
        assert_eq!(ambiguous.ambiguous.len(), 1);
        assert_eq!(ambiguous.ambiguous[0].candidates.len(), 2);
        assert!(ambiguous.content.contains("<<<ROBI_OMITTED"));

        let form = ambiguous.ambiguous[0].candidates[0].symbol.clone();
        let opened = outline(source, Language::TypeScript, &[form], 1, false).unwrap();
        assert_eq!(opened.focused.len(), 1);
        assert!(opened.ambiguous.is_empty());
    }

    #[test]
    fn typescript_focus_keeps_one_method_and_folds_the_interface() {
        let source = r#"class PaymentService {
  processCard(card: Card): Receipt {
    return { id: card.id };
  }

  validateExpiry(card: Card): void {
    if (card.expired) {
      throw new Error("expired");
    }
  }
}

interface Card {
  id: string;
  expired: boolean;
}
"#;
        let outline = outline(
            source,
            Language::TypeScript,
            &["PaymentService.processCard".into()],
            1,
            false,
        )
        .unwrap();
        assert!(outline.content.contains("return { id: card.id };"));
        assert!(!outline.content.contains("card.expired"));
        assert!(!outline.content.contains("expired: boolean"));
        assert_eq!(outline.focused, vec!["PaymentService.processCard"]);
        assert!(outline
            .omitted
            .iter()
            .any(|item| item.symbol == "PaymentService.validateExpiry"));
        assert!(outline.omitted.iter().any(|item| item.symbol == "Card"));
    }

    #[test]
    fn go_focus_uses_the_receiver_and_folds_the_sibling() {
        let source = r#"package pay

func (s *PaymentService) ProcessCard(card Card) error {
	return s.charge(card)
}

func (s *PaymentService) ValidateExpiry(card Card) error {
	if card.Expired {
		return errExpired
	}
	return nil
}
"#;
        let outline = outline(
            source,
            Language::Go,
            &["PaymentService::ProcessCard".into()],
            1,
            false,
        )
        .unwrap();
        assert!(outline.content.contains("return s.charge(card)"));
        assert!(!outline.content.contains("card.Expired"));
        assert_eq!(outline.focused, vec!["PaymentService::ProcessCard"]);
        assert_eq!(
            outline
                .omitted
                .iter()
                .map(|item| item.symbol.as_str())
                .collect::<Vec<_>>(),
            vec!["PaymentService::ValidateExpiry"]
        );
    }

    #[test]
    fn python_focus_keeps_one_method_and_folds_the_sibling() {
        let source = r#"class PaymentService:
    def process_card(self, card):
        return card.charge()

    def validate_expiry(self, card):
        if card.expired:
            raise PayError("expired")
"#;
        let outline = outline(
            source,
            Language::Python,
            &["PaymentService.process_card".into()],
            1,
            false,
        )
        .unwrap();
        assert!(outline.content.contains("return card.charge()"));
        assert!(!outline.content.contains("card.expired"));
        assert_eq!(outline.focused, vec!["PaymentService.process_card"]);
        assert!(outline
            .omitted
            .iter()
            .any(|item| item.symbol == "PaymentService.validate_expiry"));
        assert!(outline.content.contains("class PaymentService:"));
    }

    #[test]
    fn a_parse_error_is_not_an_outline() {
        let err = outline("fn broken( {", Language::Rust, &[], 1, false).unwrap_err();
        assert_eq!(err.message, "file did not parse; use read_file");
    }

    #[test]
    fn a_long_import_run_becomes_one_marker() {
        let mut source = String::new();
        for index in 0..20 {
            source.push_str(&format!("use crate::mod{index};\n"));
        }
        source.push_str("fn kept() {\n    let value = 1;\n}\n");
        let folded = outline(&source, Language::Rust, &[], 1, false).unwrap();
        assert!(folded.omitted.iter().any(|item| item.symbol == "imports"));
        assert!(!folded.content.contains("use crate::mod0;"));
        let expanded = outline(&source, Language::Rust, &[], 1, true).unwrap();
        assert!(expanded.content.contains("use crate::mod0;"));
    }

    #[test]
    fn the_omitted_hash_is_the_first_eight_bytes() {
        let source = "fn one() {\n    let value = 1;\n}\nfn two() {\n    let value = 2;\n}\n";
        let outline = outline(source, Language::Rust, &["one".into()], 1, false).unwrap();
        let omitted = outline
            .omitted
            .iter()
            .find(|item| item.symbol == "two")
            .unwrap();
        let body_start = source.find("{\n    let value = 2;").unwrap();
        let body = &source[body_start..source.len() - 1];
        let digest = Sha256::digest(body.as_bytes());
        assert_eq!(omitted.sha256, hex_encode(&digest[..8]));
    }
}
