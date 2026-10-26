//! The `/` and `@` completion menu over the composer.
//!
//! Two sigils, one menu. `/` lists commands and `@` lists this workspace's
//! skills; the rows carry the sigil, so accepting a row is one rewrite and no
//! second decision. The trigger is the domain's own rule — a token at the start of
//! the line or after a space — so what the menu offers and what
//! `agent::skills::loads_for_text` will load cannot disagree.
//!
//! The menu is not a mode for its own sake. It is one because `keys::command` is a
//! pure function of `(Mode, Focus, KeyEvent)` and cannot see whether a menu is up;
//! a `Mode::Complete` is what makes `↑`/`↓` mean "move the row" only while there is
//! a row to move.

use ratatui_textarea::TextArea;

use crate::tui::command;

/// Which sigil opened the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sigil {
    /// `/`: the terminal acts.
    Command,
    /// `@`: the text is sent and the runtime loads the skill.
    Skill,
}

impl Sigil {
    pub fn char(self) -> char {
        match self {
            Self::Command => '/',
            Self::Skill => '@',
        }
    }
}

/// Where a skill was found, for the menu's scope column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillScope {
    User,
    Project,
}

impl SkillScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "Home",
            Self::Project => "This workspace",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "user" => Self::User,
            _ => Self::Project,
        }
    }
}

/// One row of the `@` menu, mapped from a skill in `effects`.
///
/// The TUI's own type: the whole `Skill` carries a body and a file list the menu
/// never shows, and `web_api`'s DTO is the HTTP surface's shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillEntry {
    pub id: String,
    pub label: String,
    pub description: String,
    pub scope: SkillScope,
    /// The skill can only be invoked by a person, so the menu marks it `· manual`.
    pub manual: bool,
}

/// One row the menu can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Command(&'static command::CommandSpec),
    Skill(SkillEntry),
}

impl Row {
    /// A stable name, so `PartialEq` on a command row compares the command rather
    /// than the address of a `&'static` — which the derive would do, and which is
    /// always equal for the same item but reads as a pointer comparison.
    pub fn name(&self) -> &str {
        match self {
            Self::Command(spec) => spec.name,
            Self::Skill(skill) => &skill.id,
        }
    }
}

impl Row {
    /// What the row inserts: `/name` or `@id`.
    pub fn insert(&self) -> String {
        match self {
            Self::Command(spec) => format!("/{}", spec.name),
            Self::Skill(skill) => format!("@{}", skill.id),
        }
    }

    /// The left column: the name, and `· manual` where the webview shows it.
    pub fn label(&self) -> String {
        match self {
            Self::Command(spec) => format!("/{}", spec.name),
            Self::Skill(skill) => {
                let name = if skill.label == skill.id {
                    skill.id.clone()
                } else {
                    skill.label.clone()
                };
                if skill.manual {
                    format!("@{} \u{b7} manual", skill.id)
                } else {
                    format!("@{name}")
                }
            }
        }
    }

    /// The right column: what it does.
    pub fn detail(&self) -> String {
        match self {
            // A command with no key prints no separator: `/attach` and `/open`
            // take an argument a keystroke cannot carry, and a trailing `\u{b7} `
            // reads as a column that failed to render.
            Self::Command(spec) if !spec.key.is_empty() => {
                format!("{} \u{b7} {}", spec.about, spec.key)
            }
            Self::Command(spec) => spec.about.to_owned(),
            Self::Skill(skill) => skill.description.clone(),
        }
    }

    /// The trailing column: a skill's scope. A command has none.
    pub fn scope(&self) -> Option<&'static str> {
        match self {
            Self::Command(_) => None,
            Self::Skill(skill) => Some(skill.scope.label()),
        }
    }
}

/// The open menu: what was typed, what matched, and which row is highlighted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub sigil: Sigil,
    /// The first byte of the token, which is where a rewrite starts.
    pub token_start: usize,
    pub query: String,
    pub rows: Vec<Row>,
    pub selected: usize,
}

/// The caret as `(row, character column)`.
fn cursor_of(field: &TextArea<'static>) -> (usize, usize) {
    let cursor = field.cursor();
    (cursor.0, cursor.1)
}

/// Where the caret is completing, if it is.
///
/// `None` unless the text before the caret, on the caret's own line, ends in a
/// token that begins at the line's start or after a space. `token_start` is a byte
/// offset into that line, not a column, so a multibyte line rewrites correctly.
pub fn query(lines: &[String], cursor: (usize, usize)) -> Option<(Sigil, usize, String)> {
    let (row, col) = cursor;
    let line = lines.get(row)?;
    // `col` is a character index in the text area's own coordinates.
    let before: String = line.chars().take(col).collect();
    let start = before
        .char_indices()
        .rev()
        .find(|(_, ch)| ch.is_whitespace())
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    let token = &before[start..];
    let sigil = match token.chars().next()? {
        '/' => Sigil::Command,
        '@' => Sigil::Skill,
        _ => return None,
    };
    let name = &token[1..];
    // The domain's rule: letters, digits and inner hyphens. Anything else ends the
    // token, which is what stops a path or a URL from opening the menu.
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return None;
    }
    if sigil == Sigil::Command && row != 0 {
        // A command is a message: `/` on a later line is prose.
        return None;
    }
    Some((sigil, start, name.to_owned()))
}

/// The rows a query matches, in menu order: commands first, then skills.
///
/// A command wins a name collision, and the shadowed skill is rendered
/// unavailable, because a skill that quietly hid `/open` would break the one path
/// to an external file.
pub fn rows(sigil: Sigil, query: &str, skills: &[SkillEntry]) -> Vec<Row> {
    let needle = query.to_ascii_lowercase();
    let mut rows = Vec::new();
    match sigil {
        Sigil::Command => {
            for spec in command::BUILT_INS {
                if spec.name.contains(needle.as_str()) {
                    rows.push(Row::Command(spec));
                }
            }
        }
        Sigil::Skill => {
            let mut matched: Vec<SkillEntry> = skills
                .iter()
                .filter(|skill| {
                    let haystack = format!("{} {} {}", skill.id, skill.label, skill.description)
                        .to_lowercase();
                    haystack.contains(needle.as_str())
                })
                .cloned()
                .collect();
            matched.sort_by(|left, right| left.id.cmp(&right.id));
            rows.extend(matched.into_iter().map(Row::Skill));
        }
    }
    rows
}

impl Completion {
    /// Open at the caret, or nothing when the caret is not completing anything.
    pub fn open(field: &TextArea<'static>, skills: &[SkillEntry]) -> Option<Self> {
        let (sigil, token_start, query) = query(field.lines(), cursor_of(field))?;
        let rows = rows(sigil, &query, skills);
        if rows.is_empty() {
            return None;
        }
        Some(Self {
            sigil,
            token_start,
            query,
            rows,
            selected: 0,
        })
    }

    /// Keep the menu in step with what is typed, or close it.
    pub fn refresh(&mut self, field: &TextArea<'static>, skills: &[SkillEntry]) -> bool {
        let Some((sigil, token_start, query)) = query(field.lines(), cursor_of(field)) else {
            return false;
        };
        if sigil != self.sigil {
            return false;
        }
        let rows = rows(sigil, &query, skills);
        if rows.is_empty() {
            return false;
        }
        self.token_start = token_start;
        self.query = query;
        self.rows = rows;
        self.selected = self.selected.min(self.rows.len() - 1);
        true
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
    }

    /// The row the keys act on.
    pub fn current(&self) -> Option<&Row> {
        self.rows.get(self.selected)
    }

    /// Where to rewrite and what to put there: `(row, start column, length, text)`.
    ///
    /// The length is `caret column - token start`, in characters, because the token
    /// began at `token_start` and the caret is its end. It is exact mid-sentence,
    /// which is what lets `@` complete after a word.
    ///
    /// `TextArea::delete_str` deletes **forward** from the cursor, so the caller
    /// moves to the start column first; returning the column rather than a
    /// backspace count keeps that fact in one place.
    pub fn rewrite(&self, field: &TextArea<'static>) -> Option<(usize, u16, usize, String)> {
        let row = self.current()?;
        let (row_index, col) = cursor_of(field);
        let line = field.lines().get(row_index)?;
        // `token_start` is a byte offset and `col` counts characters, so the byte
        // offset is translated before the two are subtracted.
        let start_chars = line
            .char_indices()
            .take_while(|(index, _)| *index < self.token_start)
            .count();
        let remove = col.saturating_sub(start_chars);
        Some((
            row_index,
            u16::try_from(start_chars).unwrap_or(u16::MAX),
            remove,
            format!("{} ", row.insert()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(id: &str, label: &str, manual: bool) -> SkillEntry {
        SkillEntry {
            id: id.to_owned(),
            label: label.to_owned(),
            description: format!("does {id}"),
            scope: SkillScope::Project,
            manual,
        }
    }

    fn field(text: &str, col: usize) -> TextArea<'static> {
        let mut field = TextArea::new(vec![text.to_owned()]);
        field.move_cursor(ratatui_textarea::CursorMove::Jump(0, col as u16));
        field
    }

    #[test]
    fn a_slash_at_the_start_opens_the_command_menu() {
        assert_eq!(
            query(&["/mod".to_owned()], (0, 4)),
            Some((Sigil::Command, 0, "mod".to_owned()))
        );
        assert_eq!(
            query(&["/".to_owned()], (0, 1)),
            Some((Sigil::Command, 0, String::new()))
        );
    }

    #[test]
    fn an_at_sign_opens_the_skill_menu_after_a_space_too() {
        assert_eq!(
            query(&["look @cre".to_owned()], (0, 9)),
            Some((Sigil::Skill, 5, "cre".to_owned()))
        );
    }

    #[test]
    fn a_second_line_is_prose_for_a_command_and_fine_for_a_skill() {
        let lines = vec!["first".to_owned(), "/models".to_owned()];
        assert_eq!(query(&lines, (1, 7)), None);
        let lines = vec!["first".to_owned(), "@create".to_owned()];
        assert_eq!(
            query(&lines, (1, 7)),
            Some((Sigil::Skill, 0, "create".to_owned()))
        );
    }

    #[test]
    fn a_sigil_inside_a_word_is_not_a_completion() {
        assert_eq!(query(&["either/or".to_owned()], (0, 9)), None);
        assert_eq!(query(&["mail@example.com".to_owned()], (0, 16)), None);
        assert_eq!(query(&["/{not}".to_owned()], (0, 6)), None);
    }

    #[test]
    fn the_command_menu_filters_and_puts_commands_before_skills() {
        // `mod` is a prefix of both `/mode` and `/models`, and a skill of that name
        // is not merged into the command list: `/` lists commands only.
        let matched = rows(Sigil::Command, "mod", &[skill("mode", "mode", false)]);
        let names: Vec<&str> = matched.iter().map(Row::name).collect();
        // Table order, not alphabetical: the menu keeps the table's grouping.
        assert_eq!(names, ["models", "mode"]);
        assert!(matched.iter().all(|row| matches!(row, Row::Command(_))));

        let all = rows(Sigil::Command, "", &[]);
        assert_eq!(all.len(), command::BUILT_INS.len());
    }

    #[test]
    fn a_manual_skill_is_marked_and_its_scope_is_shown() {
        let skills = [
            skill("create-skill", "create-skill", false),
            skill("review-agent", "Review agent", true),
        ];
        let matched = rows(Sigil::Skill, "review", &skills);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].label(), "@review-agent \u{b7} manual");
        assert_eq!(matched[0].scope(), Some("This workspace"));
        assert_eq!(matched[0].insert(), "@review-agent");

        let plain = rows(Sigil::Skill, "create", &skills);
        assert_eq!(plain[0].label(), "@create-skill");
    }

    #[test]
    fn accepting_rewrites_only_the_token() {
        let skills = [skill("create-skill", "create-skill", false)];
        let composer = field("look @cre", 9);
        let completion = Completion::open(&composer, &skills).expect("the menu opens");
        assert_eq!(completion.sigil, Sigil::Skill);
        assert_eq!(
            completion.rewrite(&composer),
            Some((0, 5, 4, "@create-skill ".to_owned())),
            "row 0, from column 5, four characters replaced"
        );

        // Two rows: the arrows move the selection, and the clamp holds at both ends.
        let skills = [
            skill("create-skill", "create-skill", false),
            skill("configure-mcp", "configure-mcp", false),
        ];
        let composer = field("@", 1);
        let mut completion = Completion::open(&composer, &skills).expect("both rows match");
        assert_eq!(completion.rows.len(), 2);
        completion.move_by(1);
        assert_eq!(completion.selected, 1);
        completion.move_by(1);
        assert_eq!(
            completion.selected, 1,
            "the selection stops at the last row"
        );
        completion.move_by(-5);
        assert_eq!(completion.selected, 0);
    }

    #[test]
    fn a_query_that_matches_nothing_closes_the_menu() {
        let composer = field("@zzz", 4);
        assert_eq!(Completion::open(&composer, &[]), None);
    }

    #[test]
    fn refresh_closes_when_the_token_stops_matching() {
        let skills = [skill("create-skill", "create-skill", false)];
        let mut composer = field("@cr", 3);
        let mut completion = Completion::open(&composer, &skills).expect("a match");
        assert!(completion.refresh(&composer, &skills));
        composer.insert_str("zzz");
        assert!(
            !completion.refresh(&composer, &skills),
            "no row matches, so the menu closes"
        );
    }

    #[test]
    fn a_multibyte_line_rewrites_by_characters() {
        // `@` sits at byte 6 but character 2: the removal count must be characters,
        // because that is the field's own coordinate.
        let skills = [skill("create-skill", "create-skill", false)];
        let line = "\u{4f60}\u{597d} @cr";
        let cols = line.chars().count();
        let composer = field(line, cols);
        let completion = Completion::open(&composer, &skills).expect("the menu opens");
        assert_eq!(completion.token_start, 7, "a byte offset");
        assert_eq!(
            completion.rewrite(&composer),
            Some((0, 3, 3, "@create-skill ".to_owned())),
            "from column 3, three characters: @, c, r"
        );
    }
}
