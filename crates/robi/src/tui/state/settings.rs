//! The settings surface: which keys it shows, and in what order.
//!
//! **The table is the GUI's table.** `src/features/settings/` has six pages, and
//! the sections here follow their row order, so the two front ends configure the
//! same things in the same order and a reader can move between them. It is not a
//! copy of the whitelist: `system_prompt` is readable over HTTP and has no GUI
//! row, so the terminal has none either. Four tests hold that line — every key is
//! known, a `Secret` field is exactly a secret key, every `Choice` value is one
//! the service accepts, and no key appears twice.
//!
//! **Four kinds, for one reason.** A terminal has no blur, so a write is a
//! keystroke and the surface decides what it sends. `Text` opens the editor band.
//! `Secret` does too, but starts empty and is re-sent only when non-empty.
//! `Toggle` and `Choice` apply on `Enter` at once, because a two-, three-, or
//! four-value field has nothing to type — and because they make a class of
//! mistake unrepresentable: `reasoning_effort` is stored unvalidated and then
//! fails the *turn*, and every `on`/`off` key has a rule the service would
//! otherwise answer with a `400`.

use std::collections::BTreeMap;

use ratatui::crossterm::event::KeyEvent;
use ratatui::style::Style;
use ratatui_textarea::TextArea;

use crate::domain::settings::keys;

/// How a field is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One line, or a newline-separated list. `Enter` opens the band.
    Text,
    /// A credential. The value is never read back, and an empty buffer leaves it.
    Secret,
    /// `on` or `off`. `Enter` flips it.
    Toggle,
    /// One of a fixed set. `Enter` advances it.
    Choice(Choice),
}

/// The values a `Choice` cycles through.
///
/// A stored value is not always a word a reader would recognise — `sandbox_network`
/// is `on` and means `Sandboxed` — so each option carries its own label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    /// `true` for the four effort keys, where an absent value means "inherit" and
    /// is a real choice rather than a gap.
    pub optional: bool,
    pub options: &'static [(&'static str, &'static str)],
}

impl Choice {
    /// The value one `Enter` moves to.
    ///
    /// An optional choice starts by leaving `inherit`, which is why `Enter` on a
    /// field that inherits writes a value rather than deleting one; the delete is
    /// at the end of the cycle, where the last option wraps back to `None`.
    pub fn next(&self, current: Option<&str>) -> Option<String> {
        let names: Vec<Option<&'static str>> = if self.optional {
            std::iter::once(None)
                .chain(self.options.iter().map(|(value, _)| Some(*value)))
                .collect()
        } else {
            self.options.iter().map(|(value, _)| Some(*value)).collect()
        };
        let here = names.iter().position(|name| *name == current).unwrap_or(0);
        names[(here + 1) % names.len()].map(str::to_owned)
    }

    /// The label a stored value wears, or the value itself when it is not one of
    /// the options — a hand-edited `config.toml` still has to be readable here.
    pub fn label(&self, current: Option<&str>) -> String {
        match current {
            None if self.optional => "inherit".to_owned(),
            None => "—".to_owned(),
            Some(value) => self
                .options
                .iter()
                .find(|(option, _)| *option == value)
                .map(|(_, label)| (*label).to_owned())
                .unwrap_or_else(|| value.to_owned()),
        }
    }
}

/// One writable row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
}

const fn field(key: &'static str, label: &'static str, kind: Kind) -> Field {
    Field { key, label, kind }
}

const EFFORT: Choice = Choice {
    optional: true,
    options: &[("low", "Low"), ("medium", "Medium"), ("high", "High")],
};

const MECHANISM: Choice = Choice {
    optional: false,
    options: &[
        (keys::MECHANISM_OS, "OS"),
        (keys::MECHANISM_CONTAINER, "Container"),
    ],
};

/// `on` is the sandboxed posture and `off` is the open one, which is the one place
/// a setting's word and its meaning disagree in this table.
const NETWORK: Choice = Choice {
    optional: false,
    options: &[(keys::NETWORK_ON, "Sandboxed"), (keys::NETWORK_OFF, "Open")],
};

/// The six pages, in the GUI's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Providers,
    Permissions,
    Sandbox,
    Mcp,
    Audit,
}

impl Section {
    /// Every section, in rail order.
    pub const ALL: [Section; 6] = [
        Section::General,
        Section::Providers,
        Section::Permissions,
        Section::Sandbox,
        Section::Mcp,
        Section::Audit,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Providers => "Model providers",
            Section::Permissions => "Permissions",
            Section::Sandbox => "Sandbox",
            Section::Mcp => "MCP",
            Section::Audit => "Audit log",
        }
    }

    /// The writable rows. `Mcp` and `Audit` have none: they are read from app state.
    pub fn fields(self) -> &'static [Field] {
        match self {
            Section::General => GENERAL,
            Section::Providers => PROVIDERS,
            Section::Permissions => PERMISSIONS,
            Section::Sandbox => SANDBOX,
            Section::Mcp | Section::Audit => &[],
        }
    }

    /// Whether the section's first row is the active workspace.
    ///
    /// Only General: the other five are about the whole app rather than about the
    /// workspace it happens to have open.
    pub fn leads_with_workspace(self) -> bool {
        self == Section::General
    }
}

/// One editable row drawn from app state rather than from the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsRow {
    /// The field this row writes, or `None` for a row that is only read.
    pub field: Option<&'static Field>,
    pub label: String,
    /// The value as drawn: a bullet mask, an em dash, or the value itself.
    pub value: String,
}

impl SettingsRow {
    pub fn read_only(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            field: None,
            label: label.into(),
            value: value.into(),
        }
    }
}

/// A value the surface holds for one key.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Loaded {
    /// The stored text. Always `None` for a secret: the plaintext never enters the
    /// app, so nothing can print it by accident.
    pub plain: Option<String>,
    pub secret: bool,
    /// A secret is stored. For a non-secret this mirrors `plain.is_some()`.
    pub configured: bool,
}

impl Loaded {
    /// A non-secret's value.
    pub fn text(value: Option<String>) -> Self {
        let configured = value.is_some();
        Self {
            plain: value,
            secret: false,
            configured,
        }
    }

    /// A credential, reduced to the one bit the surface may know.
    pub fn credential(configured: bool) -> Self {
        Self {
            plain: None,
            secret: true,
            configured,
        }
    }
}

/// What the surface says about the last write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub message: String,
    pub error: bool,
}

/// The MCP config paths the surface shows.
///
/// The file *text* is deliberately absent: the terminal never becomes an editor,
/// and a config file carries header values the app has no reason to hold.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct McpPaths {
    pub user_path: String,
    pub project_path: String,
    pub project_enabled: bool,
    /// Where each server writes its own log: `log_root/<server id>/`.
    pub log_root: String,
}

/// The editor band's state: which field, and the text.
#[derive(Debug)]
struct Editor {
    field: &'static Field,
    area: TextArea<'static>,
}

/// The settings view's own state.
#[derive(Debug)]
pub struct SettingsView {
    section: usize,
    pub selected: usize,
    editor: Option<Editor>,
    values: BTreeMap<&'static str, Loaded>,
    note: Option<Note>,
}

impl Default for SettingsView {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsView {
    pub fn new() -> Self {
        Self {
            section: 0,
            selected: 0,
            editor: None,
            values: BTreeMap::new(),
            note: None,
        }
    }

    /// Open on General with nothing selected and no editor.
    ///
    /// The section resets, as the manage view's filter does: a reader who opened
    /// the surface wants the first page, not wherever they were last time.
    pub fn open(&mut self) {
        self.section = 0;
        self.selected = 0;
        self.editor = None;
        self.note = None;
    }

    pub fn section(&self) -> Section {
        Section::ALL[self.section.min(Section::ALL.len() - 1)]
    }

    /// Move the rail, and put the selection back on the first row.
    pub fn step_section(&mut self, delta: isize) {
        let count = Section::ALL.len() as isize;
        self.section = (self.section as isize + delta).rem_euclid(count) as usize;
        self.selected = 0;
        self.editor = None;
    }

    pub fn note(&self) -> Option<&Note> {
        self.note.as_ref()
    }

    pub fn clear_note(&mut self) {
        self.note = None;
    }

    pub fn saved(&mut self, label: &'static str) {
        self.note = Some(Note {
            message: format!("saved \u{b7} {label}"),
            error: false,
        });
    }

    pub fn failed(&mut self, message: impl Into<String>) {
        self.note = Some(Note {
            message: message.into(),
            error: true,
        });
    }

    /// Replace every loaded value.
    pub fn set_values(&mut self, values: BTreeMap<&'static str, Loaded>) {
        self.values = values;
    }

    pub fn value(&self, key: &str) -> Option<&Loaded> {
        self.values.get(key)
    }

    /// The text a stored key holds, or `None` when it is unset.
    pub fn plain(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .and_then(|loaded| loaded.plain.as_deref())
    }

    pub fn editing(&self) -> Option<&'static str> {
        self.editor.as_ref().map(|editor| editor.field.key)
    }

    /// The editor's field, for the band's title.
    pub fn editing_field(&self) -> Option<&'static Field> {
        self.editor.as_ref().map(|editor| editor.field)
    }

    pub fn editor(&self) -> Option<&TextArea<'static>> {
        self.editor.as_ref().map(|editor| &editor.area)
    }

    /// Rows the editor band takes: the field's lines, capped, plus its two edges.
    pub fn editor_height(&self) -> u16 {
        match &self.editor {
            Some(editor) => (editor.area.lines().len() as u16).clamp(1, 6) + 2,
            None => 0,
        }
    }

    /// Open the band on `field`, seeded with what is stored — and empty for a
    /// secret, whose stored value the app does not have and must not want.
    pub fn begin(&mut self, field: &'static Field) -> Option<String> {
        let mut area = TextArea::default();
        area.set_cursor_line_style(Style::default());
        if field.kind != Kind::Secret {
            if let Some(text) = self.plain(field.key) {
                area.insert_str(text);
                // The stored value is the starting point, so the first keystroke
                // appends rather than replacing what the reader came to edit.
                area.move_cursor(ratatui_textarea::CursorMove::End);
            }
        }
        self.editor = Some(Editor { field, area });
        self.note = None;
        Some(self.editor_text())
    }

    /// The buffer, as the write would send it.
    pub fn editor_text(&self) -> String {
        match &self.editor {
            Some(editor) => editor.area.lines().join("\n").trim().to_owned(),
            None => String::new(),
        }
    }

    pub fn editor_input(&mut self, key: &KeyEvent) {
        if let Some(editor) = self.editor.as_mut() {
            editor.area.input(*key);
        }
    }

    /// Close the band and drop what was typed.
    pub fn cancel(&mut self) {
        self.editor = None;
    }

    /// Hold the write's value against the key, so the row shows it before the
    /// store answers. A failed write reverts by re-reading, which is the only
    /// copy that cannot be wrong.
    pub fn hold(&mut self, field: &'static Field, value: Option<String>) {
        self.editor = None;
        self.values.insert(field.key, Loaded::text(value));
    }

    /// Move the selection within `rows`, clamped.
    pub fn select(&mut self, delta: isize, rows: usize) {
        if rows == 0 {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as isize + delta).clamp(0, rows as isize - 1) as usize;
    }

    /// A page at a time, clamped the way a line is.
    pub fn page(&mut self, delta: isize, rows: usize, viewport: usize) {
        self.select(delta.saturating_mul(viewport as isize), rows);
    }

    /// Keep the selection inside a list that changed under it.
    pub fn reconcile(&mut self, rows: usize) {
        if rows == 0 {
            self.selected = 0;
        } else if self.selected >= rows {
            self.selected = rows - 1;
        }
    }
}

/// Every writable field, in rail order.
pub fn all_fields() -> impl Iterator<Item = &'static Field> {
    Section::ALL
        .iter()
        .flat_map(|section| section.fields().iter())
}

/// The label of the field that writes `key`, for a note about a write.
pub fn label_of(key: &str) -> Option<&'static str> {
    all_fields()
        .find(|field| field.key == key)
        .map(|field| field.label)
}

/// Every key the surface reads, in table order, once each.
pub fn keys() -> Vec<String> {
    all_fields().map(|field| field.key.to_owned()).collect()
}

/// How a stored value is drawn for one field.
pub fn value_text(field: &Field, loaded: Option<&Loaded>) -> String {
    match field.kind {
        Kind::Secret => {
            if loaded.is_some_and(|loaded| loaded.configured) {
                "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}".to_owned()
            } else {
                "\u{2014}".to_owned()
            }
        }
        Kind::Toggle => match loaded.and_then(|loaded| loaded.plain.as_deref()) {
            Some(value) => value.to_owned(),
            None => "\u{2014}".to_owned(),
        },
        Kind::Choice(choice) => choice.label(loaded.and_then(|loaded| loaded.plain.as_deref())),
        Kind::Text => match loaded.and_then(|loaded| loaded.plain.as_deref()) {
            Some(value) if !value.is_empty() => value.to_owned(),
            _ => "\u{2014}".to_owned(),
        },
    }
}

/// The value a `Toggle` flips to.
pub fn toggled(current: Option<&str>) -> &'static str {
    if current == Some(keys::PROVIDER_OFF) {
        keys::PROVIDER_ON
    } else {
        keys::PROVIDER_OFF
    }
}

/// General: the GUI's `GeneralSettings` — the workspace, the model defaults, the
/// four limits, and the three path lists.
static GENERAL: &[Field] = &[
    field(keys::MODEL, "Model", Kind::Text),
    field(
        keys::REASONING_EFFORT,
        "Global effort",
        Kind::Choice(EFFORT),
    ),
    field(keys::MODEL_ASK, "Ask model", Kind::Text),
    field(
        keys::REASONING_EFFORT_ASK,
        "Ask effort",
        Kind::Choice(EFFORT),
    ),
    field(keys::MODEL_PLAN, "Plan model", Kind::Text),
    field(
        keys::REASONING_EFFORT_PLAN,
        "Plan effort",
        Kind::Choice(EFFORT),
    ),
    field(keys::MODEL_AGENT, "Agent model", Kind::Text),
    field(
        keys::REASONING_EFFORT_AGENT,
        "Agent effort",
        Kind::Choice(EFFORT),
    ),
    field(keys::MAX_ITERATIONS, "Max iterations", Kind::Text),
    field(
        keys::SUBAGENT_MAX_ITERATIONS,
        "Subagent iterations",
        Kind::Text,
    ),
    field(
        keys::SUBAGENT_TIMEOUT_SECONDS,
        "Subagent timeout",
        Kind::Text,
    ),
    field(keys::TOOL_TIMEOUT_SECONDS, "Tool timeout", Kind::Text),
    field(keys::PATH_ALLOW_READ, "Read paths", Kind::Text),
    field(keys::PATH_ALLOW_WRITE, "Write paths", Kind::Text),
    field(keys::PATH_ENTRIES, "PATH entries", Kind::Text),
    // `/open` needs a program to hand the file to, and a terminal-only install has
    // no other way to set one. `file_open_patterns` stays out: the terminal has no
    // viewer to route to, so there is nothing to match a pattern against.
    field(keys::FILE_OPEN_COMMAND, "Open command", Kind::Text),
];

/// Model providers: `ModelProvidersSettings`.
static PROVIDERS: &[Field] = &[
    field(keys::PROVIDER_OPENCODE_GO, "OpenCode Go", Kind::Toggle),
    field(keys::OPENCODE_GO_API_KEY, "OpenCode Go key", Kind::Secret),
    field(keys::BASE_URL, "Base URL", Kind::Text),
    field(keys::PROVIDER_ANTHROPIC, "Anthropic", Kind::Toggle),
    field(keys::ANTHROPIC_API_KEY, "Anthropic key", Kind::Secret),
    field(keys::PROVIDER_DEEPSEEK, "DeepSeek", Kind::Toggle),
    field(keys::DEEPSEEK_API_KEY, "DeepSeek key", Kind::Secret),
    field(keys::DEEPSEEK_BASE_URL, "DeepSeek base URL", Kind::Text),
    field(keys::BRAVE_SEARCH_API_KEY, "Brave key", Kind::Secret),
];

/// Permissions: `PermissionsSettings`.
static PERMISSIONS: &[Field] = &[
    field(keys::WEB_SEARCH_APPROVAL, "Web search asks", Kind::Toggle),
    field(keys::WEB_FETCH_APPROVAL, "Web fetch asks", Kind::Toggle),
];

/// Sandbox: `SandboxSettings`, its four headings flattened into one section.
static SANDBOX: &[Field] = &[
    field(
        keys::SANDBOX_MECHANISM,
        "Mechanism",
        Kind::Choice(MECHANISM),
    ),
    field(keys::SANDBOX_IMAGE, "Image", Kind::Text),
    field(keys::SANDBOX_RUNTIME, "Runtime", Kind::Text),
    field(keys::SANDBOX_MEMORY_MB, "Memory (MiB)", Kind::Text),
    field(keys::SANDBOX_CPUS, "CPUs", Kind::Text),
    field(keys::SANDBOX_PIDS_LIMIT, "Process limit", Kind::Text),
    field(keys::SANDBOX_NETWORK, "Network", Kind::Choice(NETWORK)),
    field(keys::SANDBOX_PROJECT_FILES, "Project files", Kind::Toggle),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn all_fields() -> Vec<Field> {
        Section::ALL
            .iter()
            .flat_map(|section| section.fields().iter().copied())
            .collect()
    }

    #[test]
    fn every_key_the_surface_shows_is_one_the_service_knows() {
        for field in all_fields() {
            assert!(
                keys::known_setting(field.key).is_some(),
                "{} is not a known setting",
                field.key
            );
        }
    }

    #[test]
    fn a_secret_field_is_exactly_a_secret_key() {
        // The mask, the empty-buffer rule, and the fact the value is never read
        // back all key off this one flag, so a disagreement is a leak waiting.
        for field in all_fields() {
            let known = keys::known_setting(field.key).expect("a known setting");
            assert_eq!(
                known.secret,
                field.kind == Kind::Secret,
                "{} disagrees about being a secret",
                field.key
            );
        }
    }

    #[test]
    fn every_choice_offers_a_value_the_service_accepts() {
        for field in all_fields() {
            let Kind::Choice(choice) = field.kind else {
                continue;
            };
            for (value, _) in choice.options {
                let ok = match field.key {
                    keys::SANDBOX_MECHANISM => {
                        *value == keys::MECHANISM_OS || *value == keys::MECHANISM_CONTAINER
                    }
                    keys::SANDBOX_NETWORK => {
                        *value == keys::NETWORK_ON || *value == keys::NETWORK_OFF
                    }
                    _ => matches!(*value, "low" | "medium" | "high"),
                };
                assert!(
                    ok,
                    "{} offers {value}, which the service refuses",
                    field.key
                );
            }
        }
    }

    #[test]
    fn no_key_appears_twice() {
        let mut seen = std::collections::HashSet::new();
        for field in all_fields() {
            assert!(seen.insert(field.key), "{} is listed twice", field.key);
        }
        assert_eq!(keys().len(), seen.len());
    }

    #[test]
    fn the_sections_are_the_gui_pages() {
        assert_eq!(
            Section::ALL.map(Section::title),
            [
                "General",
                "Model providers",
                "Permissions",
                "Sandbox",
                "MCP",
                "Audit log"
            ]
        );
        // The two appended pages read from app state and write nothing.
        assert!(Section::Mcp.fields().is_empty());
        assert!(Section::Audit.fields().is_empty());
        assert_eq!(Section::Sandbox.fields().len(), 8);
    }

    #[test]
    fn the_selection_stops_at_both_ends() {
        let mut view = SettingsView::new();
        view.select(-1, 5);
        assert_eq!(view.selected, 0, "no selection above the first row");
        view.select(99, 5);
        assert_eq!(view.selected, 4, "and none past the last");
        view.reconcile(3);
        assert_eq!(view.selected, 2, "a shorter list pulls it back");
        view.reconcile(0);
        assert_eq!(view.selected, 0);
    }

    #[test]
    fn a_page_moves_by_the_viewport_and_a_section_resets_the_row() {
        let mut view = SettingsView::new();
        view.select(4, 20);
        view.page(1, 20, 5);
        assert_eq!(view.selected, 9);
        view.page(1, 20, 5);
        assert_eq!(view.selected, 14);
        view.page(1, 20, 5);
        assert_eq!(view.selected, 19, "clamped at the end, not wrapped");
        view.step_section(1);
        assert_eq!(view.section(), Section::Providers);
        assert_eq!(view.selected, 0, "a new section starts at its first row");
        view.step_section(-1);
        assert_eq!(view.section(), Section::General, "the rail wraps");
    }

    #[test]
    fn an_effort_choice_leaves_inherit_first_and_returns_to_it_last() {
        let choice = EFFORT;
        assert_eq!(choice.next(None).as_deref(), Some("low"));
        assert_eq!(choice.next(Some("low")).as_deref(), Some("medium"));
        assert_eq!(
            choice.next(Some("high")),
            None,
            "the delete is the last step"
        );
        assert_eq!(choice.label(None), "inherit");
        assert_eq!(choice.label(Some("high")), "High");
        // A hand-edited value with no option row still reads back.
        assert_eq!(choice.label(Some("extreme")), "extreme");
    }

    #[test]
    fn a_fixed_choice_wraps_between_its_two_values() {
        assert_eq!(MECHANISM.next(Some("os")).as_deref(), Some("container"));
        assert_eq!(MECHANISM.next(Some("container")).as_deref(), Some("os"));
        assert_eq!(NETWORK.label(Some("on")), "Sandboxed");
        assert_eq!(NETWORK.label(Some("off")), "Open");
    }

    #[test]
    fn a_toggle_flips_both_ways_and_a_blank_reads_as_off() {
        assert_eq!(toggled(None), keys::PROVIDER_OFF);
        assert_eq!(toggled(Some(keys::PROVIDER_ON)), keys::PROVIDER_OFF);
        assert_eq!(toggled(Some(keys::PROVIDER_OFF)), keys::PROVIDER_ON);
    }

    #[test]
    fn a_secret_draws_bullets_once_stored_and_never_its_text() {
        let secret = Field {
            key: keys::ANTHROPIC_API_KEY,
            label: "Anthropic key",
            kind: Kind::Secret,
        };
        assert_eq!(
            value_text(&secret, Some(&Loaded::credential(true))),
            "\u{2022}".repeat(8)
        );
        assert_eq!(
            value_text(&secret, Some(&Loaded::credential(false))),
            "\u{2014}"
        );
        let loaded = Loaded::credential(true);
        assert!(loaded.plain.is_none(), "a credential has no readable text");
    }

    #[test]
    fn the_editor_starts_from_the_stored_text_and_never_from_a_secret() {
        let mut view = SettingsView::new();
        let mut values = BTreeMap::new();
        values.insert(keys::PATH_ALLOW_READ, Loaded::text(Some("/a\n/b".into())));
        values.insert(keys::OPENCODE_GO_API_KEY, Loaded::credential(true));
        view.set_values(values);

        let read = Section::General
            .fields()
            .iter()
            .find(|field| field.key == keys::PATH_ALLOW_READ)
            .expect("a read-path row");
        view.begin(read);
        assert_eq!(view.editor_text(), "/a\n/b");
        assert_eq!(view.editor_height(), 4, "two lines and its two edges");

        let key = Section::Providers
            .fields()
            .iter()
            .find(|field| field.key == keys::OPENCODE_GO_API_KEY)
            .expect("a key row");
        view.begin(key);
        assert_eq!(view.editor_text(), "", "a secret never starts filled");
        view.cancel();
        assert!(view.editing().is_none());
    }
}
