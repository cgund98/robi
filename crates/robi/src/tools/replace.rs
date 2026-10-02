//! Exact search-and-replace.

const BOM: char = '\u{feff}';

/// Replace `old` with `new` in `original`.
///
/// The model's newlines are converted to the file's line ending. A leading
/// BOM is kept. `old` must be non-empty and differ from `new`.
pub fn apply_edit(
    original: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<String, String> {
    if old.is_empty() {
        return Err("old is empty; use write_file to create or replace the file".to_owned());
    }
    if old == new {
        return Err("old and new are identical".to_owned());
    }
    let (bom, body) = split_bom(original);
    let ending: &str = if body.contains("\r\n") { "\r\n" } else { "\n" };
    let old = convert_ending(old, ending);
    let new = convert_ending(new, ending);
    if old == new {
        return Err("old and new are identical".to_owned());
    }
    let next = replace_exact(body, &old, &new, replace_all)?;
    Ok(format!("{bom}{next}"))
}

fn split_bom(text: &str) -> (String, &str) {
    if let Some(rest) = text.strip_prefix(BOM) {
        (BOM.to_string(), rest)
    } else {
        (String::new(), text)
    }
}

fn convert_ending(text: &str, ending: &str) -> String {
    let normalized = text.replace("\r\n", "\n");
    if ending == "\n" {
        normalized
    } else {
        normalized.replace('\n', "\r\n")
    }
}

fn replace_exact(content: &str, old: &str, new: &str, replace_all: bool) -> Result<String, String> {
    let count = content.matches(old).count();
    if count == 0 {
        return Err(not_found(content, old));
    }
    if count > 1 && !replace_all {
        return Err(format!(
            "old text matched {count} times; include more surrounding lines so it matches once, or set replace_all"
        ));
    }
    if replace_all {
        Ok(content.replace(old, new))
    } else {
        Ok(content.replacen(old, new, 1))
    }
}

fn not_found(content: &str, old: &str) -> String {
    if !trim_lines(old).is_empty() && trim_lines(content).contains(&trim_lines(old)) {
        "old text was not found: it matches only if whitespace is ignored, so check indentation and trailing spaces; call read_file and copy the lines exactly".to_owned()
    } else {
        "old text was not found: the file may have changed or the snippet was retyped; call read_file and copy old exactly from the current content".to_owned()
    }
}

fn trim_lines(text: &str) -> String {
    text.trim()
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_match_is_replaced() {
        let next = apply_edit("alpha\nbeta\n", "beta\n", "gamma\n", false).unwrap();
        assert_eq!(next, "alpha\ngamma\n");
    }

    #[test]
    fn a_repeated_match_is_an_error_unless_replace_all_is_set() {
        let error = apply_edit("a\na\n", "a\n", "b\n", false).unwrap_err();
        assert!(error.contains("matched 2 times"), "{error}");
        let next = apply_edit("a\na\n", "a\n", "b\n", true).unwrap();
        assert_eq!(next, "b\nb\n");
    }

    #[test]
    fn a_crlf_file_keeps_its_ending() {
        let next = apply_edit("alpha\r\nbeta\r\n", "beta\n", "gamma\n", false).unwrap();
        assert_eq!(next, "alpha\r\ngamma\r\n");
    }

    #[test]
    fn a_bom_is_kept() {
        let original = "\u{feff}alpha\n".to_owned();
        let next = apply_edit(&original, "alpha\n", "beta\n", false).unwrap();
        assert!(next.starts_with('\u{feff}'));
        assert!(next.contains("beta\n"));
    }

    #[test]
    fn a_whitespace_near_miss_says_so() {
        let error = apply_edit("beta \n", "beta\n", "gamma\n", false).unwrap_err();
        assert!(error.contains("whitespace"), "{error}");
    }

    #[test]
    fn identical_text_is_refused() {
        let error = apply_edit("alpha\n", "alpha\n", "alpha\n", false).unwrap_err();
        assert!(error.contains("identical"), "{error}");
    }

    #[test]
    fn an_empty_old_names_write_file() {
        let error = apply_edit("alpha\n", "", "beta\n", false).unwrap_err();
        assert!(error.contains("write_file"), "{error}");
    }
}
