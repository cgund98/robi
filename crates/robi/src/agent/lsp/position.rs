//! LSP positions are 0-based UTF-16. Tool positions are 1-based Unicode scalars.

use async_lsp::lsp_types::Position;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionError {
    Line,
    Character,
}

pub fn to_lsp(text: &str, line: u32, character: u32) -> Result<Position, PositionError> {
    if line == 0 {
        return Err(PositionError::Line);
    }
    if character == 0 {
        return Err(PositionError::Character);
    }
    let line_text = nth_line(text, (line - 1) as usize).ok_or(PositionError::Line)?;
    let scalars = line_text.chars().count() as u32;
    if character - 1 > scalars {
        return Err(PositionError::Character);
    }
    let utf16 = line_text
        .chars()
        .take((character - 1) as usize)
        .map(utf16_units)
        .sum();
    Ok(Position::new(line - 1, utf16))
}

/// 1-based scalar column for an LSP UTF-16 offset on one line.
pub fn column_from_utf16(line_text: &str, utf16: u32) -> u32 {
    let mut seen = 0u32;
    for (index, ch) in line_text.chars().enumerate() {
        if seen >= utf16 {
            return (index as u32) + 1;
        }
        seen += utf16_units(ch);
    }
    (line_text.chars().count() as u32) + 1
}

pub fn line_text(text: &str, line: u32) -> Option<&str> {
    if line == 0 {
        return None;
    }
    nth_line(text, (line - 1) as usize)
}

fn nth_line(text: &str, index: usize) -> Option<&str> {
    let mut rest = text;
    for _ in 0..index {
        let end = rest.find('\n')?;
        rest = &rest[end + 1..];
    }
    if index > 0 && rest.is_empty() && !text.ends_with('\n') {
        return None;
    }
    if index > 0 && text.is_empty() {
        return None;
    }
    // An empty file has no line a tool can point at.
    if text.is_empty() {
        return None;
    }
    let end = rest.find('\n').unwrap_or(rest.len());
    let line = &rest[..end];
    Some(line.strip_suffix('\r').unwrap_or(line))
}

fn utf16_units(ch: char) -> u32 {
    if (ch as u32) > 0xFFFF {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_is_one_scalar_and_two_utf16_units() {
        let text = "a😀b\n";
        let at_emoji = to_lsp(text, 1, 2).unwrap();
        assert_eq!(at_emoji.character, 1);
        let at_b = to_lsp(text, 1, 3).unwrap();
        assert_eq!(at_b.character, 3);
        assert_eq!(column_from_utf16("a😀b", 1), 2);
        assert_eq!(column_from_utf16("a😀b", 3), 3);
    }

    #[test]
    fn a_position_past_the_line_is_an_error() {
        assert_eq!(to_lsp("hi\n", 1, 3).unwrap().character, 2);
        assert_eq!(to_lsp("hi\n", 1, 4), Err(PositionError::Character));
        assert_eq!(to_lsp("hi\n", 3, 1), Err(PositionError::Line));
        assert_eq!(to_lsp("", 1, 1), Err(PositionError::Line));
    }
}
