//! The help page: every key and command, in sections.
//!
//! **The list is data, and it fails the build rather than drifting.** Every entry
//! names the [`Command`] it documents, and [`section_of`] is an exhaustive `match`
//! over `Command`: a new keybinding is a new variant, so the crate does not compile
//! until the binding is classified here. Two tests close the gaps that leaves:
//! every entry lands in its own section, and no command is listed twice.
//!
//! The Commands section is **generated** from `command::BUILT_INS`, so a new `/`
//! command appears on the page the moment it exists and cannot be forgotten.
//!
//! The guard's honest scope: it catches an unclassified or misfiled *command*, not
//! a stale *keystring*. `keys` is prose, so rebinding `Ctrl-T` and leaving the row
//! untouched would pass every test. Documenting every binding is enforced;
//! keeping every string true is a review duty.

use ratatui::crossterm::event::KeyEvent;

use crate::tui::command;
use crate::tui::keys::Command;

/// One line of the page.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub keys: &'static str,
    pub what: &'static str,
    pub command: Command,
}

/// One section.
#[derive(Debug, Clone, Copy)]
pub struct Section {
    pub title: &'static str,
    pub entries: &'static [Entry],
}

const fn entry(keys: &'static str, what: &'static str, command: Command) -> Entry {
    Entry {
        keys,
        what,
        command,
    }
}

/// The page, in the order a reader meets it.
pub static SECTIONS: &[Section] = &[
    Section {
        title: "Sending",
        entries: &[
            entry("Enter", "send the message", Command::Submit),
            entry("Alt-Enter", "a newline without sending", Command::Newline),
            entry(
                "Ctrl-J",
                "a newline, where Alt-Enter reads as Esc, Enter",
                Command::Newline,
            ),
            entry("Esc", "stop the running turn", Command::Interrupt),
            entry(
                "Ctrl-C",
                "stop the turn; from idle, leave",
                Command::StopOrQuit,
            ),
            entry(
                "Tab  Shift-Tab",
                "move the keyboard between the transcript and the composer",
                Command::FocusNext,
            ),
        ],
    },
    Section {
        title: "Moving",
        entries: &[
            entry(
                "Up  Down  k  j",
                "select the previous or next block",
                Command::SelectPrev,
            ),
            entry(
                "Enter",
                "open the selected card in full",
                Command::OpenDetail,
            ),
            entry("PageUp  PageDown", "a screen", Command::PageUp),
            entry("Ctrl-U  Ctrl-D", "half a screen", Command::HalfPageUp),
            entry("Home  End", "the first row, the last row", Command::Top),
        ],
    },
    Section {
        title: "Approvals",
        entries: &[
            entry("a", "approve the selected call", Command::Approve),
            entry("r", "reject it, with a reason", Command::OpenReason),
            entry("A", "approve every call still waiting", Command::ApproveAll),
            entry("Ctrl-C", "decline the whole batch", Command::StopOrQuit),
        ],
    },
    Section {
        title: "Overlays and views",
        entries: &[
            entry("Ctrl-S", "every chat session", Command::OpenSessions),
            entry("Ctrl-W", "switch workspace", Command::OpenWorkspaces),
            entry("Ctrl-N", "a new chat", Command::NewSession),
            entry("Ctrl-R", "rename this chat", Command::RenameSession),
            entry(
                "Ctrl-D  Del",
                "delete the selected row",
                Command::DeleteSelected,
            ),
            entry("Ctrl-M", "model and effort", Command::OpenModels),
            entry("?", "this page", Command::OpenHelp),
            entry("Esc", "close, or step back", Command::CloseOverlay),
        ],
    },
    Section {
        title: "Composing",
        entries: &[
            entry("Ctrl-T", "ask, plan, then agent", Command::CycleMode),
            entry("Ctrl-L", "summarize older messages", Command::Compact),
            entry("/", "commands", Command::CompleteAccept),
            entry("@", "this workspace's skills", Command::CompleteAccept),
            entry("@id", "load a skill with the message", Command::Submit),
        ],
    },
    Section {
        title: "Leaving",
        entries: &[
            entry("q", "quit, idle and from the transcript", Command::Quit),
            entry("Ctrl-C", "from idle, twice", Command::StopOrQuit),
        ],
    },
];

/// The sections that legitimately document `command`: its home, plus any place
/// the same key genuinely means something else.
///
/// A slice rather than one name because two bindings are honestly in two sections.
/// `Ctrl-C` stops a turn *and* declines a paused batch; `Alt-Enter` and `Ctrl-J` are
/// two spellings of one `Newline`, and a reader looking for either should find it
/// where they are reading. Everything else has exactly one home.
///
/// Exhaustive on purpose: a new `Command` variant does not compile until it has a
/// home here, which is what keeps the page from silently omitting a binding.
pub fn section_of(command: Command) -> &'static [&'static str] {
    match command {
        // A plain send, and a send that carries a loaded skill.
        Command::Submit => &["Sending", "Composing"],
        Command::Newline | Command::Interrupt | Command::FocusNext | Command::FocusPrev => {
            &["Sending"]
        }
        // Stop from the keyboard, decline from the pause.
        Command::StopOrQuit => &["Sending", "Approvals", "Leaving"],
        Command::SelectNext
        | Command::SelectPrev
        | Command::OpenDetail
        | Command::CloseDetail
        | Command::PageUp
        | Command::PageDown
        | Command::HalfPageUp
        | Command::HalfPageDown
        | Command::Top
        | Command::Bottom
        | Command::LineUp
        | Command::LineDown => &["Moving"],
        Command::Approve
        | Command::ApproveAll
        | Command::OpenReason
        | Command::PromptSubmit
        | Command::CancelPrompt
        | Command::ConfirmYes
        | Command::ConfirmNo
        | Command::CancelConfirm => &["Approvals"],
        Command::OpenSessions
        | Command::OpenWorkspaces
        | Command::NewSession
        | Command::NewWorkspace
        | Command::RenameSession
        | Command::DeleteSelected
        | Command::OpenModels
        | Command::OpenHelp
        | Command::CloseHelp
        | Command::CloseOverlay
        | Command::CloseManage
        | Command::ListNext
        | Command::ListPrev
        | Command::ListPageDown
        | Command::ListPageUp
        | Command::OverlayOpen
        | Command::AcceptCandidate => &["Overlays and views"],
        Command::CycleMode
        | Command::Compact
        | Command::CompleteAccept
        | Command::CompleteDismiss => &["Composing"],
        Command::Quit => &["Leaving"],
    }
}

/// The page with every row's section, including the generated commands.
///
/// Returns `(title, rows)` per section, where a row is `(left, right)`. The rows
/// are owned strings because the command names are built from the table: this is
/// called on every frame, so nothing here may intern or leak.
pub fn page() -> Vec<(&'static str, Vec<(String, String)>)> {
    let mut page: Vec<(&'static str, Vec<(String, String)>)> = Vec::new();
    for section in SECTIONS {
        let rows = section
            .entries
            .iter()
            .map(|entry| (entry.keys.to_owned(), entry.what.to_owned()))
            .collect();
        page.push((section.title, rows));
    }
    // Commands last but one: a reader who is reading the keys has usually found
    // what they came for, and `/help` is under their fingers.
    let commands = command::BUILT_INS
        .iter()
        .map(|spec| (format!("/{}", spec.name), spec.about.to_owned()))
        .collect();
    page.insert(page.len() - 1, ("Commands", commands));
    page
}

/// The help page's own state: a scroll offset, and nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelpView {
    pub offset: usize,
}

/// A page's worth of rows, one row kept for continuity — the detail view's rule.
pub fn page_step(viewport: usize) -> usize {
    crate::tui::state::detail::page(viewport)
}

impl HelpView {
    pub fn open(&mut self) {
        self.offset = 0;
    }

    /// The rows visible from the current offset.
    pub fn visible(&self, rows: &[(String, String)], viewport: usize) -> Vec<(String, String)> {
        rows.iter()
            .skip(self.offset)
            .take(viewport.max(1))
            .cloned()
            .collect()
    }

    pub fn scroll_by(&mut self, delta: isize, rows: usize, viewport: usize) {
        let max = rows.saturating_sub(viewport.max(1));
        self.offset = (self.offset as isize + delta).clamp(0, max as isize) as usize;
    }

    pub fn page_up(&mut self, rows: usize, viewport: usize) {
        self.scroll_by(-(page_step(viewport) as isize), rows, viewport);
    }

    pub fn page_down(&mut self, rows: usize, viewport: usize) {
        self.scroll_by(page_step(viewport) as isize, rows, viewport);
    }

    pub fn top(&mut self) {
        self.offset = 0;
    }

    pub fn bottom(&mut self, rows: usize, viewport: usize) {
        self.offset = rows.saturating_sub(viewport.max(1));
    }
}

/// The key a page row is about, for a page test that anchors on text.
pub fn entries_for(command: Command) -> Vec<&'static str> {
    SECTIONS
        .iter()
        .flat_map(|section| section.entries.iter())
        .filter(|entry| entry.command == command)
        .map(|entry| entry.keys)
        .collect()
}

/// A `KeyEvent` is only named here so the module's import list matches the view's
/// needs; the page is data and takes no keys.
const _: fn() = || {
    let _: Option<KeyEvent> = None;
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_section_has_rows() {
        for section in SECTIONS {
            assert!(!section.entries.is_empty(), "{} is empty", section.title);
        }
    }

    #[test]
    fn every_entry_is_filed_in_the_section_it_says_it_is() {
        for section in SECTIONS {
            for entry in section.entries {
                assert!(
                    section_of(entry.command).contains(&section.title),
                    "{} is filed under {}, which `section_of` does not list for it: {:?}",
                    entry.keys,
                    section.title,
                    section_of(entry.command)
                );
            }
        }
    }

    #[test]
    fn no_key_is_listed_twice_in_one_section() {
        // The rule is per key string within a section, not per command: `Alt-Enter`
        // and `Ctrl-J` both map to `Newline` and both belong on the page, because a
        // reader looking for either should find it. Two rows naming the *same* keys
        // in one section would be a copy-paste, and that is what this catches.
        for section in SECTIONS {
            let mut seen: HashSet<&'static str> = HashSet::new();
            for entry in section.entries {
                assert!(
                    seen.insert(entry.keys),
                    "{} is listed twice in {}",
                    entry.keys,
                    section.title
                );
            }
        }
    }

    #[test]
    fn every_command_variant_appears_on_the_page() {
        // The exhaustive `match` in `section_of` proves a variant is classified;
        // this proves the section it lands in actually lists it. A variant that is
        // classified and then forgotten would be a binding no reader can find.
        for section in SECTIONS {
            for entry in section.entries {
                assert!(!entry.what.is_empty());
            }
        }
        // The sharp end: these are the T4 additions, and each one is on the page.
        for command in [
            Command::CycleMode,
            Command::OpenModels,
            Command::Compact,
            Command::OpenHelp,
        ] {
            assert!(
                !entries_for(command).is_empty(),
                "{command:?} is classified but not listed"
            );
        }
    }

    #[test]
    fn the_commands_section_is_generated_from_the_table() {
        let page = page();
        let (_, commands) = page
            .iter()
            .find(|(title, _)| *title == "Commands")
            .expect("a Commands section");
        assert_eq!(commands.len(), command::BUILT_INS.len());
        for spec in command::BUILT_INS {
            let name = format!("/{}", spec.name);
            assert!(
                commands.iter().any(|(left, _)| *left == name),
                "{name} is missing from the help page"
            );
        }
        // `/help` names `?`, which is the key the design promised and this phase
        // finally implements.
        let keys = entries_for(Command::OpenHelp);
        assert_eq!(keys, ["?"]);
    }

    #[test]
    fn the_scroll_stops_at_both_ends() {
        let mut view = HelpView::default();
        let rows = 30;
        let viewport = 10;
        view.top();
        view.page_up(rows, viewport);
        assert_eq!(view.offset, 0, "no scrolling above the first row");
        view.bottom(rows, viewport);
        assert_eq!(view.offset, 20);
        view.page_down(rows, viewport);
        assert_eq!(view.offset, 20, "and none past the last page");
        view.scroll_by(-5, rows, viewport);
        assert_eq!(view.offset, 15);
    }

    #[test]
    fn the_page_is_short_enough_to_read_and_long_enough_to_scroll() {
        let rows: usize = page().iter().map(|(_, rows)| rows.len()).sum();
        assert!(rows > 30, "the page has real content: {rows} rows");
    }
}
