//! Line collapsing, then head, tail, and error windows.
//!
//! Both passes are pure functions of one string. The stored original keeps
//! the raw stream, carriage returns included.

use std::sync::LazyLock;

use regex::Regex;

const SMALL: usize = 4 * 1024;
const MIN_SAVINGS: usize = 1024;
const HEAD: usize = 12;
const TAIL: usize = 20;
const MAX_ERROR_LINES: usize = 200;

/// Collapse and, when the collapse is still large, slice.
///
/// `None` means the stream stays byte-identical: it was under 4 KiB, or the
/// rendered text saved less than 1 KiB.
pub fn compress_stream(input: &str) -> Option<String> {
    if input.len() < SMALL {
        return None;
    }
    let reduced = reduce_lines(input);
    let collapsed = collapse(&reduced);
    let sliced = if rendered_len(&collapsed) > SMALL && collapsed.len() >= HEAD + TAIL {
        slice(&collapsed)
    } else {
        collapsed
    };
    let text = join(&sliced);
    if input.len().saturating_sub(text.len()) < MIN_SAVINGS {
        return None;
    }
    Some(text)
}

/// Split on newlines, then keep the segment after the last carriage return.
pub fn reduce_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n')
        .map(|line| match line.rfind('\r') {
            Some(at) => line[at + 1..].to_owned(),
            None => line.to_owned(),
        })
        .collect()
}

pub fn header(id: &str, sha256: &str, exit_code: i64) -> String {
    let prefix = &sha256[..sha256.len().min(16)];
    format!(r#"<<<ROBI_LOG id="{id}" sha256="{prefix}" exit={exit_code}>>>"#)
}

#[derive(Clone)]
struct OutLine {
    text: String,
    /// Inclusive 1-based reduced-line span this output stands for.
    origin: Option<(usize, usize)>,
    summary: bool,
    error: bool,
}

fn rendered_len(lines: &[OutLine]) -> usize {
    join(lines).len()
}

fn join(lines: &[OutLine]) -> String {
    lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    CargoTest,
    CargoProgress,
    Generic,
}

struct Classified {
    kind: Kind,
    template: String,
    name: String,
    status: String,
    verb: String,
}

struct OpenRun {
    kind: Kind,
    template: String,
    first: String,
    first_name: String,
    last_name: String,
    status: String,
    verb: String,
    start: usize,
    end: usize,
    count: usize,
    buffered: Vec<String>,
}

fn threshold(kind: Kind) -> usize {
    match kind {
        Kind::Generic => 4,
        Kind::CargoTest | Kind::CargoProgress => 3,
    }
}

fn collapse(lines: &[String]) -> Vec<OutLine> {
    let mut out = Vec::new();
    let mut open: Option<OpenRun> = None;
    for (index, line) in lines.iter().enumerate() {
        let number = index + 1;
        if error_after(line).is_some() {
            flush(&mut open, &mut out);
            out.push(verbatim(line, number, true));
            continue;
        }
        if line.trim().is_empty() {
            flush(&mut open, &mut out);
            out.push(verbatim(line, number, false));
            continue;
        }
        let class = classify(line);
        if let Some(run) = open.as_mut() {
            if run.kind == class.kind && run.template == class.template {
                run.push(line, &class, number);
                continue;
            }
        }
        flush(&mut open, &mut out);
        open = Some(OpenRun::start(class, line, number));
    }
    flush(&mut open, &mut out);
    out
}

fn verbatim(line: &str, number: usize, error: bool) -> OutLine {
    OutLine {
        text: line.to_owned(),
        origin: Some((number, number)),
        summary: false,
        error,
    }
}

impl OpenRun {
    fn start(class: Classified, line: &str, number: usize) -> Self {
        Self {
            kind: class.kind,
            template: class.template,
            first: line.to_owned(),
            first_name: class.name.clone(),
            last_name: class.name,
            status: class.status,
            verb: class.verb,
            start: number,
            end: number,
            count: 1,
            buffered: vec![line.to_owned()],
        }
    }

    fn push(&mut self, line: &str, class: &Classified, number: usize) {
        self.count += 1;
        self.end = number;
        self.last_name = class.name.clone();
        self.status = class.status.clone();
        let limit = threshold(self.kind);
        if self.count < limit {
            self.buffered.push(line.to_owned());
        } else if self.count == limit {
            self.buffered.clear();
        }
    }
}

fn flush(open: &mut Option<OpenRun>, out: &mut Vec<OutLine>) {
    let Some(run) = open.take() else {
        return;
    };
    if run.count < threshold(run.kind) {
        for (offset, line) in run.buffered.iter().enumerate() {
            out.push(verbatim(line, run.start + offset, false));
        }
        return;
    }
    let origin = Some((run.start, run.end));
    let marker = format!(
        "<<<ROBI_LOG repeated={} lines={}-{}>>>",
        run.count, run.start, run.end
    );
    match run.kind {
        Kind::CargoTest => {
            let label = if run.status == "ignored" {
                "ignored"
            } else {
                "passed"
            };
            let star = star_name(&run.first_name, &run.last_name);
            out.push(OutLine {
                text: format!(
                    "test {star} ({} tests {label}) ... {}",
                    run.count, run.status
                ),
                origin,
                summary: true,
                error: false,
            });
            out.push(OutLine {
                text: marker,
                origin: None,
                summary: true,
                error: false,
            });
        }
        Kind::CargoProgress => {
            out.push(OutLine {
                text: format!("{} ... ({} crates)", run.verb, run.count),
                origin,
                summary: true,
                error: false,
            });
            out.push(OutLine {
                text: marker,
                origin: None,
                summary: true,
                error: false,
            });
        }
        Kind::Generic => {
            out.push(OutLine {
                text: marker,
                origin: None,
                summary: true,
                error: false,
            });
            out.push(OutLine {
                text: format!("    {}", truncate_chars(&run.first, 200)),
                origin,
                summary: true,
                error: false,
            });
        }
    }
}

fn classify(line: &str) -> Classified {
    if let Some(caps) = cargo_test().captures(line) {
        let name = caps
            .name("name")
            .map(|name| name.as_str().to_owned())
            .unwrap_or_default();
        let status = caps
            .name("status")
            .map(|status| status.as_str().to_owned())
            .unwrap_or_else(|| "ok".to_owned());
        return Classified {
            kind: Kind::CargoTest,
            template: status.clone(),
            name,
            status,
            verb: String::new(),
        };
    }
    if let Some(caps) = cargo_progress().captures(line) {
        let verb = caps
            .name("verb")
            .map(|verb| verb.as_str().to_owned())
            .unwrap_or_default();
        return Classified {
            kind: Kind::CargoProgress,
            template: verb.clone(),
            name: String::new(),
            status: String::new(),
            verb,
        };
    }
    Classified {
        kind: Kind::Generic,
        template: template_of(line),
        name: String::new(),
        status: String::new(),
        verb: String::new(),
    }
}

fn cargo_test() -> &'static Regex {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^test (?<name>\S.*) \.\.\. (?<status>ok|ignored)$").expect("cargo test")
    });
    &PATTERN
}

fn cargo_progress() -> &'static Regex {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\s*(?<verb>Compiling|Checking|Downloading|Downloaded|Documenting|Finished)\b")
            .expect("cargo progress")
    });
    &PATTERN
}

fn slot() -> &'static Regex {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?x)
            [0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}
            | \b[0-9a-fA-F]{8,}\b
            | \b\d+(?:\.\d+)?(?:ms|s|us|µs|KiB|MiB|GiB|KB|MB)?\b
            ",
        )
        .expect("slot regex")
    });
    &PATTERN
}

fn template_of(line: &str) -> String {
    slot().replace_all(line.trim_end(), "0").into_owned()
}

fn star_name(first: &str, last: &str) -> String {
    let mut shared_end = 0;
    for (left, right) in first.chars().zip(last.chars()) {
        if left != right {
            break;
        }
        shared_end += left.len_utf8();
    }
    let shared = &first[..shared_end];
    let mut best: Option<usize> = None;
    let mut index = 0;
    while index < shared.len() {
        if shared[index..].starts_with("::") {
            let end = index + 2;
            if end >= 8 {
                best = Some(end);
            }
            index += 2;
            continue;
        }
        let ch = shared[index..].chars().next().unwrap_or('_');
        if ch == '_' {
            let end = index + ch.len_utf8();
            if end >= 8 {
                best = Some(end);
            }
        }
        index += ch.len_utf8();
    }
    match best {
        Some(end) => format!("{}*", &shared[..end]),
        None => "*".to_owned(),
    }
}

fn truncate_chars(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut cut = max_bytes;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    &text[..cut]
}

/// Lines after the hit. `None` when the line is not an error.
fn error_after(line: &str) -> Option<usize> {
    const LONG: usize = 40;
    const SHORT: usize = 2;
    if line.starts_with("panic:")
        || line.contains("panicked at")
        || line.starts_with("stack backtrace:")
        || line.starts_with("Traceback (most recent call last):")
        || thread_panicked(line)
    {
        return Some(LONG);
    }
    if rustc_error(line)
        || line.starts_with("Error:")
        || line.ends_with(" ... FAILED")
        || fail_line(line)
        || line.starts_with("npm ERR!")
        || ts_error(line)
        || go_error(line)
    {
        return Some(SHORT);
    }
    None
}

fn rustc_error(line: &str) -> bool {
    static PATTERN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^error(\[E\d+\])?:").expect("rustc"));
    PATTERN.is_match(line)
}

fn thread_panicked(line: &str) -> bool {
    static PATTERN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^thread '.*' panicked").expect("thread"));
    PATTERN.is_match(line)
}

fn fail_line(line: &str) -> bool {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^FAIL\b").expect("fail"));
    PATTERN.is_match(line)
}

fn ts_error(line: &str) -> bool {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^error TS\d+:").expect("ts"));
    PATTERN.is_match(line)
}

fn go_error(line: &str) -> bool {
    static PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^E\s{2,}\S").expect("go"));
    PATTERN.is_match(line)
}

fn slice(lines: &[OutLine]) -> Vec<OutLine> {
    let count = lines.len();
    if count < HEAD + TAIL {
        return lines.to_vec();
    }
    let mut keep = vec![false; count];
    keep[..HEAD.min(count)].fill(true);
    keep[count.saturating_sub(TAIL)..].fill(true);
    for (index, line) in lines.iter().enumerate() {
        if line.summary {
            keep[index] = true;
        }
    }

    let mut windowed = 0usize;
    let mut unwindowed = 0usize;
    let mut stopped = false;
    for (index, line) in lines.iter().enumerate() {
        if !line.error {
            continue;
        }
        let after = error_after(&line.text).unwrap_or(2);
        if stopped {
            unwindowed += 1;
            continue;
        }
        let start = index.saturating_sub(2);
        let end = (index + after).min(count - 1);
        let extra = (start..=end).filter(|at| !keep[*at]).count();
        if windowed + extra > MAX_ERROR_LINES {
            stopped = true;
            unwindowed += 1;
            continue;
        }
        windowed += extra;
        keep[start..=end].fill(true);
    }

    let mut rendered = Vec::new();
    let mut index = 0;
    while index < count {
        if keep[index] {
            rendered.push(lines[index].clone());
            index += 1;
            continue;
        }
        let gap = index;
        while index < count && !keep[index] {
            index += 1;
        }
        if let Some(marker) = omission(&lines[gap..index], unwindowed) {
            rendered.push(OutLine {
                text: marker,
                origin: None,
                summary: false,
                error: false,
            });
        }
    }
    rendered
}

fn omission(gap: &[OutLine], unwindowed: usize) -> Option<String> {
    let mut count = 0usize;
    let mut start = None;
    let mut end = None;
    for line in gap {
        let Some((from, to)) = line.origin else {
            continue;
        };
        count += to - from + 1;
        start = Some(start.map_or(from, |left: usize| left.min(from)));
        end = Some(end.map_or(to, |right: usize| right.max(to)));
    }
    let (start, end) = (start?, end?);
    if count == 0 {
        return None;
    }
    let mut marker = format!("<<<ROBI_LOG omitted={count} lines={start}-{end}>>>");
    if unwindowed > 0 {
        marker.push_str(&format!(" unwindowed={unwindowed}"));
    }
    Some(marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing_suite(count: usize) -> String {
        let mut lines = vec!["   Compiling demo v0.1.0".to_owned()];
        for index in 0..count {
            lines.push(format!("test tests::test_auth_{index} ... ok"));
        }
        lines.push(
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.00s"
                .to_owned(),
        );
        lines.join("\n")
    }

    #[test]
    fn a_short_stream_is_unchanged() {
        assert!(compress_stream("ok\n").is_none());
    }

    #[test]
    fn a_green_suite_collapses_and_keeps_the_result_line() {
        let input = passing_suite(200);
        assert!(input.len() > SMALL);
        let out = compress_stream(&input).expect("collapsed");
        assert!(out.contains("test tests::test_auth_* (200 tests passed) ... ok"));
        assert!(out.contains("<<<ROBI_LOG repeated=200 lines="));
        assert!(out.contains("Compiling ... (1 crates)") || out.contains("Compiling demo"));
        assert!(out.contains("test result: ok."));
        assert!(!out.contains("test tests::test_auth_5 ... ok"));
        assert!(out.len() < SMALL);
    }

    #[test]
    fn a_failed_test_stays_verbatim() {
        let mut lines = Vec::new();
        for index in 0..40 {
            lines.push(format!("test tests::test_auth_{index} ... ok"));
        }
        lines.push("test tests::auth ... FAILED".to_owned());
        lines.push("thread 'tests::auth' panicked at src/auth.rs:88:5:".to_owned());
        for index in 0..20 {
            lines.push(format!("test tests::test_other_{index} ... ok"));
        }
        let input = lines.join("\n");
        let padded = format!("{input}\n{}", "x".repeat(SMALL));
        let out = compress_stream(&padded).unwrap_or(padded);
        assert!(out.contains("test tests::auth ... FAILED"), "{out}");
    }

    #[test]
    fn error_path_ok_is_not_an_error() {
        let line = "test tests::error_path ... ok";
        assert!(error_after(line).is_none());
        assert!(cargo_test().is_match(line));
    }

    #[test]
    fn a_progress_bar_keeps_the_last_segment() {
        let reduced = reduce_lines("50%\r80%\r100%\n");
        assert_eq!(reduced, vec!["100%".to_owned(), String::new()]);
    }

    #[test]
    fn warnings_are_not_pinned() {
        assert!(error_after("warning: unused import").is_none());
        assert!(error_after("0 errors").is_none());
    }

    #[test]
    fn phase_two_keeps_a_panic_and_marks_the_gap() {
        let mut lines = vec!["head".to_owned()];
        for index in 0..200 {
            lines.push(format!("noise-{index}x {}", "abcdefghij".repeat(3)));
        }
        lines.push("thread 'tests::auth' panicked at src/auth.rs:1:1:".to_owned());
        lines.push("assertion failed".to_owned());
        for index in 0..30 {
            lines.push(format!("tail filler {index}"));
        }
        let input = lines.join("\n");
        assert!(input.len() > SMALL);
        let out = compress_stream(&input).expect("sliced");
        assert!(out.contains("panicked at"));
        assert!(out.contains("<<<ROBI_LOG omitted="), "{out}");
        assert!(out.contains("tail filler 29") || out.lines().count() < lines.len());
    }

    #[test]
    fn savings_under_one_kib_leave_the_stream() {
        let mut lines = Vec::new();
        for index in 0..30 {
            lines.push(format!("unique-{index}x {}", "z".repeat(160)));
        }
        let input = lines.join("\n");
        assert!(input.len() > SMALL);
        assert!(compress_stream(&input).is_none());
    }
}
