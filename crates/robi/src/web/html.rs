//! Turn an HTTP body into the text the model should see.

use scraper::{ElementRef, Html, Node};

const TEXT_CAP: usize = 32 * 1024;

const TEXT_TYPES: &[&str] = &[
    "text/plain",
    "text/markdown",
    "application/json",
    "text/xml",
    "application/xml",
    "text/csv",
];

/// The page after chrome is removed and the size cap is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReducedPage {
    pub title: Option<String>,
    pub text: String,
    pub truncated: bool,
}

/// Reduce a response body. HTML becomes markdown. Other allowed text types
/// are returned as text. Anything else is an error that names the type.
pub fn reduce_body(content_type: &str, body: &[u8]) -> Result<ReducedPage, String> {
    let media = media_type(content_type);
    if media.is_empty() {
        return Err("page content type is missing".into());
    }
    if media == "text/html" || media == "application/xhtml+xml" {
        return Ok(reduce_html(body));
    }
    if TEXT_TYPES.contains(&media.as_str()) {
        let text = String::from_utf8_lossy(body).into_owned();
        let (text, truncated) = cap_text(text, TEXT_CAP);
        return Ok(ReducedPage {
            title: None,
            text,
            truncated,
        });
    }
    Err(format!("page content type {media} is not text"))
}

fn reduce_html(body: &[u8]) -> ReducedPage {
    let document = Html::parse_document(&String::from_utf8_lossy(body));
    let title = document
        .select(&scraper::Selector::parse("title").expect("title selector"))
        .next()
        .map(|node| collapse_space(&node.text().collect::<String>()))
        .filter(|title| !title.is_empty());
    let mut out = String::new();
    if let Some(body_node) = document
        .select(&scraper::Selector::parse("body").expect("body selector"))
        .next()
    {
        write_children(body_node, &mut out, Block::Flow);
    }
    let text = collapse_blank_lines(out.trim());
    let (text, truncated) = cap_text(text, TEXT_CAP);
    ReducedPage {
        title,
        text,
        truncated,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Block {
    Flow,
    Pre,
}

fn write_children(element: ElementRef<'_>, out: &mut String, block: Block) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => write_text(text, out, block),
            Node::Element(element) => {
                if skip(element.name()) || hidden(element) {
                    continue;
                }
                let element_ref = ElementRef::wrap(child).expect("element node");
                write_element(element, element_ref, out, block);
            }
            _ => {}
        }
    }
}

fn write_text(text: &scraper::node::Text, out: &mut String, block: Block) {
    if block == Block::Pre {
        out.push_str(text);
        return;
    }
    let chunk = collapse_space(text);
    if chunk.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with(|ch: char| ch.is_whitespace()) {
        out.push(' ');
    }
    out.push_str(&chunk);
}

fn write_element(
    element: &scraper::node::Element,
    element_ref: ElementRef<'_>,
    out: &mut String,
    block: Block,
) {
    match element.name() {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = element.name().as_bytes()[1] - b'0';
            break_block(out);
            out.push_str(&"#".repeat(level as usize));
            out.push(' ');
            write_children(element_ref, out, Block::Flow);
            out.push_str("\n\n");
        }
        "p" | "div" | "section" | "article" | "blockquote" => {
            break_block(out);
            write_children(element_ref, out, Block::Flow);
            out.push_str("\n\n");
        }
        "br" => out.push('\n'),
        "pre" => {
            break_block(out);
            out.push_str("```\n");
            write_children(element_ref, out, Block::Pre);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
        "code" if block != Block::Pre => {
            out.push('`');
            write_children(element_ref, out, Block::Flow);
            out.push('`');
        }
        "a" => {
            let href = element.attr("href").unwrap_or("").trim();
            let start = out.len();
            write_children(element_ref, out, block);
            let label = out[start..].trim().to_owned();
            out.truncate(start);
            if href.is_empty() {
                out.push_str(&label);
            } else if label.is_empty() {
                out.push_str(href);
            } else {
                out.push('[');
                out.push_str(&label);
                out.push_str("](");
                out.push_str(href);
                out.push(')');
            }
        }
        "li" => {
            break_block(out);
            out.push_str("- ");
            write_children(element_ref, out, Block::Flow);
            out.push('\n');
        }
        "tr" => {
            out.push_str("| ");
            write_children(element_ref, out, Block::Flow);
            out.push_str(" |\n");
        }
        "th" | "td" => {
            write_children(element_ref, out, Block::Flow);
            out.push_str(" | ");
        }
        "img" => {
            if let Some(alt) = element.attr("alt") {
                let alt = collapse_space(alt);
                if !alt.is_empty() {
                    if !out.is_empty() && !out.ends_with(|ch: char| ch.is_whitespace()) {
                        out.push(' ');
                    }
                    out.push_str(&alt);
                }
            }
        }
        _ => write_children(element_ref, out, block),
    }
}

fn skip(name: &str) -> bool {
    matches!(
        name,
        "script"
            | "style"
            | "noscript"
            | "template"
            | "svg"
            | "iframe"
            | "object"
            | "embed"
            | "canvas"
            | "form"
            | "nav"
            | "footer"
            | "aside"
    )
}

fn hidden(element: &scraper::node::Element) -> bool {
    if element.attr("hidden").is_some() {
        return true;
    }
    element
        .attr("aria-hidden")
        .is_some_and(|value| value == "true")
}

fn break_block(out: &mut String) {
    if out.is_empty() || out.ends_with("\n\n") {
        return;
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
}

fn collapse_space(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines() {
        if line.trim().is_empty() {
            blank += 1;
            if blank <= 1 && !out.is_empty() {
                out.push('\n');
            }
            continue;
        }
        blank = 0;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.trim().to_owned()
}

fn media_type(header: &str) -> String {
    header
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

pub(crate) fn cap_text(text: String, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_drops_chrome_and_keeps_structure() {
        let page = reduce_body(
            "text/html; charset=utf-8",
            br#"<html><head><title> Docs </title><style>body{}</style></head>
            <body><nav>Home</nav><h1>Guide</h1><p>See <a href="https://example.com/a">the page</a>.</p>
            <script>alert(1)</script><pre><code>fn main() {}</code></pre>
            <div hidden>secret</div></body></html>"#,
        )
        .unwrap();
        assert_eq!(page.title.as_deref(), Some("Docs"));
        assert!(page.text.contains("# Guide"));
        assert!(page.text.contains("[the page](https://example.com/a)"));
        assert!(page.text.contains("fn main() {}"));
        assert!(!page.text.contains("alert"));
        assert!(!page.text.contains("Home"));
        assert!(!page.text.contains("secret"));
        assert!(!page.text.contains("body{}"));
    }

    #[test]
    fn json_is_returned_as_text_and_images_are_refused() {
        let page = reduce_body("application/json", br#"{"ok":true}"#).unwrap();
        assert_eq!(page.text, r#"{"ok":true}"#);
        assert!(page.title.is_none());
        let error = reduce_body("image/png", b"not-an-image").unwrap_err();
        assert!(error.contains("image/png"));
    }
}
