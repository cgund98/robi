//! Split a source file on tree-sitter nodes.

use tree_sitter::Node;

use crate::error::IndexError;

pub const SPLIT_BYTES: usize = 2048;
pub const MAX_CHUNK_BYTES: usize = 8192;
pub const EMBED_BYTES: usize = 8192;
pub const IMPORT_BYTES: usize = 500;
const WINDOW_OVERLAP_LINES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Go,
    Markdown,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::JavaScript => "javascript",
            Self::Python => "python",
            Self::Go => "go",
            Self::Markdown => "markdown",
        }
    }

    fn separator(self) -> &'static str {
        match self {
            Self::Rust | Self::Go => "::",
            _ => ".",
        }
    }

    fn chunk_kinds(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &[
                "function_item",
                "struct_item",
                "enum_item",
                "trait_item",
                "impl_item",
                "const_item",
                "static_item",
                "type_item",
                "mod_item",
            ],
            Self::TypeScript | Self::Tsx => &[
                "function_declaration",
                "class_declaration",
                "method_definition",
                "interface_declaration",
                "type_alias_declaration",
            ],
            Self::JavaScript => &[
                "function_declaration",
                "class_declaration",
                "method_definition",
            ],
            Self::Python => &["function_definition", "class_definition"],
            Self::Go => &[
                "function_declaration",
                "method_declaration",
                "type_declaration",
            ],
            Self::Markdown => &["section"],
        }
    }

    fn import_kinds(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &["use_declaration"],
            Self::TypeScript | Self::Tsx | Self::JavaScript => &["import_statement"],
            Self::Python => &["import_statement", "import_from_statement"],
            Self::Go => &["import_declaration"],
            Self::Markdown => &[],
        }
    }
}

pub fn language_for_path(path: &str) -> Option<Language> {
    let ext = path.rsplit('.').next()?;
    Some(match ext {
        "rs" => Language::Rust,
        "ts" => Language::TypeScript,
        "tsx" => Language::Tsx,
        "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
        "py" => Language::Python,
        "go" => Language::Go,
        "md" => Language::Markdown,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkKind {
    Symbol,
    Window,
}

impl ChunkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Symbol => "symbol",
            Self::Window => "window",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub start_line: u32,
    pub end_line: u32,
    pub symbol: String,
    pub kind: ChunkKind,
    pub body: String,
    /// Path, symbol, imports, and body, capped. The embedder adds its prefix.
    pub embed_text: String,
}

pub fn chunk_source(
    language: Language,
    path: &str,
    source: &str,
) -> Result<Vec<Chunk>, IndexError> {
    let mut parser = tree_sitter::Parser::new();
    let grammar = grammar(language);
    parser
        .set_language(&grammar)
        .map_err(|err| IndexError::Message(format!("set language: {err}")))?;
    let Some(tree) = parser.parse(source, None) else {
        return Ok(windows_for_file(path, source, ""));
    };
    if tree.root_node().has_error() {
        return Ok(windows_for_file(path, source, ""));
    }
    let imports = imports_of(language, tree.root_node(), source);
    let mut nodes = Vec::new();
    collect_innermost(tree.root_node(), language.chunk_kinds(), &mut nodes);
    let mut chunks = Vec::new();
    for node in nodes {
        chunks.extend(chunks_for_node(language, path, source, node, &imports));
    }
    Ok(chunks)
}

fn grammar(language: Language) -> tree_sitter::Language {
    match language {
        Language::Rust => tree_sitter_rust::LANGUAGE.into(),
        Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Language::Python => tree_sitter_python::LANGUAGE.into(),
        Language::Go => tree_sitter_go::LANGUAGE.into(),
        Language::Markdown => tree_sitter_md::LANGUAGE.into(),
    }
}

fn collect_innermost<'a>(node: Node<'a>, kinds: &[&str], out: &mut Vec<Node<'a>>) {
    let mut nested = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let before = out.len();
        collect_innermost(child, kinds, out);
        if out.len() > before {
            nested = true;
        }
    }
    if !nested && kinds.contains(&node.kind()) {
        out.push(node);
    }
}

fn chunks_for_node(
    language: Language,
    path: &str,
    source: &str,
    node: Node<'_>,
    imports: &str,
) -> Vec<Chunk> {
    let symbol = symbol_of(language, node, source);
    let text = node_text(node, source);
    if text.len() <= SPLIT_BYTES {
        return vec![make_chunk(
            path,
            imports,
            &symbol,
            ChunkKind::Symbol,
            text,
            line_at(source, node.start_byte()),
            end_line_at(source, node.start_byte(), node.end_byte()),
        )];
    }
    let signature = first_line(text);
    let mut children = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        children.push(child);
    }
    if children.is_empty() {
        return window_pieces(
            path,
            imports,
            &symbol,
            signature,
            text,
            node.start_byte(),
            source,
        );
    }
    let mut chunks = Vec::new();
    for child in children {
        let child_text = node_text(child, source);
        let piece = with_signature(signature, child_text);
        if piece.len() <= MAX_CHUNK_BYTES {
            chunks.push(make_chunk(
                path,
                imports,
                &symbol,
                ChunkKind::Symbol,
                &piece,
                line_at(source, child.start_byte()),
                end_line_at(source, child.start_byte(), child.end_byte()),
            ));
        } else {
            chunks.extend(window_pieces(
                path,
                imports,
                &symbol,
                signature,
                &piece,
                child.start_byte(),
                source,
            ));
        }
    }
    chunks
}

fn window_pieces(
    path: &str,
    imports: &str,
    symbol: &str,
    signature: &str,
    text: &str,
    start_byte: usize,
    source: &str,
) -> Vec<Chunk> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let mut end = index;
        let mut size = signature.len() + 1;
        while end < lines.len() && size + lines[end].len() <= MAX_CHUNK_BYTES {
            size += lines[end].len();
            end += 1;
        }
        if end == index {
            end = index + 1;
        }
        let mut body = String::new();
        let content = lines[index..end].concat();
        if !content.starts_with(signature) {
            body.push_str(signature);
            if !signature.ends_with('\n') {
                body.push('\n');
            }
        }
        body.push_str(&content);
        let body = cap_bytes(&body, MAX_CHUNK_BYTES);
        let start_line = line_at(source, start_byte) + index as u32;
        let end_line = start_line + (end - index).saturating_sub(1) as u32;
        chunks.push(make_chunk(
            path,
            imports,
            symbol,
            ChunkKind::Window,
            &body,
            start_line,
            end_line.max(start_line),
        ));
        if end >= lines.len() {
            break;
        }
        let next = end.saturating_sub(WINDOW_OVERLAP_LINES);
        index = if next > index { next } else { end };
    }
    chunks
}

fn windows_for_file(path: &str, source: &str, imports: &str) -> Vec<Chunk> {
    if source.is_empty() {
        return Vec::new();
    }
    let signature = first_line(source);
    window_pieces(path, imports, "", signature, source, 0, source)
}

fn with_signature(signature: &str, body: &str) -> String {
    if signature.is_empty() || body.starts_with(signature) {
        return body.to_owned();
    }
    let mut piece = String::with_capacity(signature.len() + body.len() + 1);
    piece.push_str(signature);
    piece.push('\n');
    piece.push_str(body);
    piece
}

fn make_chunk(
    path: &str,
    imports: &str,
    symbol: &str,
    kind: ChunkKind,
    body: &str,
    start_line: u32,
    end_line: u32,
) -> Chunk {
    let embed_text = cap_bytes(
        &format!("{path}\n{symbol}\n\n{imports}\n\n{body}"),
        EMBED_BYTES,
    );
    Chunk {
        start_line,
        end_line,
        symbol: symbol.to_owned(),
        kind,
        body: body.to_owned(),
        embed_text,
    }
}

fn imports_of(language: Language, root: Node<'_>, source: &str) -> String {
    let kinds = language.import_kinds();
    let mut text = String::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        if !kinds.contains(&child.kind()) {
            continue;
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(node_text(child, source).trim());
        if text.len() >= IMPORT_BYTES {
            break;
        }
    }
    cap_bytes(&text, IMPORT_BYTES)
}

fn symbol_of(language: Language, node: Node<'_>, source: &str) -> String {
    let mut names = Vec::new();
    let mut current = Some(node);
    while let Some(item) = current {
        if item.kind() == "method_declaration" {
            if let Some(name) = field_text(item, "name", source) {
                names.push(name);
            }
            if let Some(receiver) = receiver_type(item, source) {
                names.push(receiver);
            }
        } else if let Some(name) = node_label(item, source) {
            names.push(name);
        }
        current = item.parent();
    }
    names.reverse();
    names.join(language.separator())
}

fn node_label(node: Node<'_>, source: &str) -> Option<String> {
    if let Some(name) = field_text(node, "name", source) {
        return Some(name);
    }
    if node.kind() == "impl_item" {
        if let Some(ty) = node.child_by_field_name("type") {
            let text = node_text(ty, source).trim().to_owned();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    if node.kind() == "section" {
        let heading = node_text(node, source).lines().next()?.trim();
        let heading = heading.trim_start_matches('#').trim();
        if !heading.is_empty() {
            return Some(heading.to_owned());
        }
    }
    None
}

fn receiver_type(node: Node<'_>, source: &str) -> Option<String> {
    let receiver = node.child_by_field_name("receiver")?;
    find_kind(receiver, "type_identifier", source)
}

fn find_kind(node: Node<'_>, kind: &str, source: &str) -> Option<String> {
    if node.kind() == kind {
        let text = node_text(node, source).trim().to_owned();
        if !text.is_empty() {
            return Some(text);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_kind(child, kind, source) {
            return Some(found);
        }
    }
    None
}

fn field_text(node: Node<'_>, field: &str, source: &str) -> Option<String> {
    let child = node.child_by_field_name(field)?;
    let text = node_text(child, source).trim().to_owned();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

fn line_at(source: &str, byte: usize) -> u32 {
    let end = byte.min(source.len());
    source[..end].bytes().filter(|byte| *byte == b'\n').count() as u32 + 1
}

fn end_line_at(source: &str, start_byte: usize, end_byte: usize) -> u32 {
    if end_byte <= start_byte {
        return line_at(source, start_byte);
    }
    line_at(source, end_byte - 1)
}

pub fn cap_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_impl_keeps_the_method_and_the_type() {
        let source = "impl ChatSessionService {\n    fn append(&self) {}\n}\n";
        let chunks = chunk_source(Language::Rust, "src/domain/chat.rs", source).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].symbol, "ChatSessionService::append");
        assert_eq!(chunks[0].kind, ChunkKind::Symbol);
        assert_eq!(chunks[0].start_line, 2);
    }

    #[test]
    fn each_grammar_names_its_symbol() {
        let cases = [
            (
                Language::TypeScript,
                "src/chat.ts",
                "class ChatSessionService {\n  append() {}\n}\n",
                "ChatSessionService.append",
            ),
            (
                Language::JavaScript,
                "src/chat.js",
                "function append() {}\n",
                "append",
            ),
            (
                Language::Python,
                "src/chat.py",
                "class ChatSessionService:\n    def append(self):\n        pass\n",
                "ChatSessionService.append",
            ),
            (
                Language::Go,
                "chat.go",
                "package chat\nfunc (s *ChatSessionService) Append() {}\n",
                "ChatSessionService::Append",
            ),
            (
                Language::Markdown,
                "notes.md",
                "# Persist a session\n\nThe row is written here.\n",
                "Persist a session",
            ),
        ];
        for (language, path, source, symbol) in cases {
            let chunks = chunk_source(language, path, source).unwrap();
            assert!(
                chunks
                    .iter()
                    .any(|chunk| chunk.symbol == symbol && chunk.kind == ChunkKind::Symbol),
                "{path}: {chunks:?}"
            );
        }
    }

    #[test]
    fn a_large_function_windows_and_keeps_the_signature() {
        let mut source = String::from("fn huge() {\n");
        while source.len() <= MAX_CHUNK_BYTES + 100 {
            source.push_str("    let value = 1;\n");
        }
        source.push_str("}\n");
        let chunks = chunk_source(Language::Rust, "src/huge.rs", &source).unwrap();
        let windows: Vec<_> = chunks
            .iter()
            .filter(|chunk| chunk.kind == ChunkKind::Window)
            .collect();
        assert!(!windows.is_empty(), "{:?}", chunks.len());
        assert!(
            windows
                .iter()
                .any(|chunk| chunk.body.starts_with("fn huge() {")),
            "{:?}",
            windows
                .first()
                .map(|chunk| &chunk.body[..40.min(chunk.body.len())])
        );
    }

    #[test]
    fn a_syntax_error_is_windows_with_an_empty_symbol() {
        let source = "fn append( {\n    let x = 1;\n}\n";
        let chunks = chunk_source(Language::Rust, "src/broken.rs", source).unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks.iter().all(|chunk| chunk.symbol.is_empty()));
        assert!(chunks.iter().all(|chunk| chunk.kind == ChunkKind::Window));
    }
}
