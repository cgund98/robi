//! Parse `file_open_command` into argv. No process is started here.

/// How long the opener waits for an early exit before treating a live process as success.
pub const OPEN_TIMEOUT_SECONDS: u64 = 10;

/// Reject a stored command that cannot be rendered.
pub fn validate_open_command(value: &str) -> Result<(), String> {
    if value.contains('\n') || value.contains('\r') {
        return Err("file_open_command must be a single line".into());
    }
    check_placeholders(value)?;
    let tokens = tokenize(value)?;
    if tokens.first().is_none_or(String::is_empty) {
        return Err("file_open_command must name a program".into());
    }
    Ok(())
}

/// Argv for one open. `{file_path}` is the absolute path. When `line` is absent,
/// `{line}` and a colon immediately before it are removed.
pub fn render_open_command(
    template: &str,
    file_path: &str,
    line: Option<u32>,
) -> Result<Vec<String>, String> {
    validate_open_command(template)?;
    let mut argv = Vec::new();
    for token in tokenize(template)? {
        let rendered = subst_token(&token, file_path, line);
        if !rendered.is_empty() {
            argv.push(rendered);
        }
    }
    if argv.first().is_none_or(String::is_empty) {
        return Err("file_open_command must name a program".into());
    }
    Ok(argv)
}

fn check_placeholders(value: &str) -> Result<(), String> {
    let mut rest = value;
    let mut saw_path = false;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            return Err("file_open_command has an unclosed placeholder".into());
        };
        match &after[..end] {
            "file_path" => saw_path = true,
            "line" => {}
            other => {
                return Err(format!(
                    "file_open_command has an unknown placeholder: {{{other}}}"
                ));
            }
        }
        rest = &after[end + 1..];
    }
    if !saw_path {
        return Err("file_open_command must contain {file_path}".into());
    }
    Ok(())
}

/// Split on whitespace. A double quote groups one token and is not part of it.
fn tokenize(template: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    for ch in template.chars() {
        match ch {
            '"' => in_quote = !in_quote,
            c if c.is_whitespace() && !in_quote => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if in_quote {
        return Err("file_open_command has an unclosed quote".into());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

fn subst_token(token: &str, file_path: &str, line: Option<u32>) -> String {
    let mut out = String::new();
    let mut rest = token;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let Some(end) = after.find('}') else {
            out.push_str(after);
            return out;
        };
        match (&after[1..end], line) {
            ("file_path", _) => out.push_str(file_path),
            ("line", Some(n)) => out.push_str(&n.to_string()),
            ("line", None) => {
                if out.ends_with(':') {
                    out.pop();
                }
            }
            (other, _) => {
                out.push('{');
                out.push_str(other);
                out.push('}');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_appended_to_the_path() {
        let argv = render_open_command("zed {file_path}:{line}", "/tmp/a.rs", Some(12)).unwrap();
        assert_eq!(argv, vec!["zed".to_string(), "/tmp/a.rs:12".to_string()]);
    }

    #[test]
    fn a_missing_line_drops_the_colon() {
        let argv = render_open_command("zed {file_path}:{line}", "/tmp/a.rs", None).unwrap();
        assert_eq!(argv, vec!["zed".to_string(), "/tmp/a.rs".to_string()]);
    }

    #[test]
    fn quotes_keep_one_argument() {
        let argv = render_open_command(
            r#"code --goto "{file_path}:{line}""#,
            "/tmp/my file.rs",
            Some(3),
        )
        .unwrap();
        assert_eq!(
            argv,
            vec![
                "code".to_string(),
                "--goto".to_string(),
                "/tmp/my file.rs:3".to_string()
            ]
        );
    }

    #[test]
    fn an_unknown_placeholder_is_rejected() {
        let error = validate_open_command("zed {file_path} {column}").unwrap_err();
        assert!(error.contains("{column}"));
    }

    #[test]
    fn a_command_without_a_path_is_rejected() {
        let error = validate_open_command("zed {line}").unwrap_err();
        assert!(error.contains("{file_path}"));
    }
}
