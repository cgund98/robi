//! `(Mode, Focus, KeyEvent) -> Command`.
//!
//! A pure map, so "every binding in the design doc's table is reachable and no
//! key means two things in one mode" is a unit test rather than a manual check.
//!
//! **The arrow keys scroll, and the field still owns them when it has text.**
//! `Up`/`Down` map to [`Command::LineUp`] / [`Command::LineDown`] in both
//! focuses; `app::update` gives the key back to the text area when the composer
//! holds text, because a multi-line draft needs its caret. An empty field has
//! no caret to move, so there the arrows scroll the transcript — which is the
//! gesture people try first. `PageUp`/`PageDown` and `Ctrl-U`/`Ctrl-D` are
//! unconditional.
//!
//! **Focus is part of the map, not a check inside a handler.** The composer has
//! a text field on screen for the whole life of the process, so `q` cannot mean
//! "quit" while a user may be typing it into a field. It means quit with the
//! transcript focused and nothing at all with the composer focused, where it is
//! a character.
//!
//! **An overlay's field owns printable keys.** The sessions filter and the
//! workspace path are text, so their actions are control keys: `Ctrl-R` rename,
//! `Ctrl-D` delete, `Tab` accept a completion. The transcript's `j`/`k` are not
//! bound here for the same reason.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::state::{Focus, Mode};

/// What a key asks for. `app::update` turns one into a state change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Leave. The transcript owns the keyboard and no turn is running.
    Quit,
    /// One screen up. Releases the follow lock.
    PageUp,
    /// One screen down. Re-arms the follow lock at the last row.
    PageDown,
    /// Half a screen up.
    HalfPageUp,
    /// Half a screen down.
    HalfPageDown,
    /// The first row.
    Top,
    /// The last row.
    Bottom,
    /// One row up. The transcript's arrow key; the composer takes it back while
    /// it holds text.
    LineUp,
    /// One row down.
    LineDown,
    /// Send the composer's text. `update` refuses a blank send.
    Submit,
    /// A newline in the composer, without sending it.
    Newline,
    /// `Ctrl-C`: refuse a paused batch, stop the running turn, close an overlay,
    /// or quit when idle.
    ///
    /// `update` decides which, because it is the only place that knows what is
    /// on top and whether anything is running.
    StopOrQuit,
    /// `Esc`: stop the running turn, or clear the card cursor when idle.
    ///
    /// Never quits. `Esc` is the key people reach for to call off the thing that
    /// is happening, and a turn is exactly that; leaving stays on `Ctrl-C`
    /// (twice, from idle) and on `q`. One command covers both meanings because
    /// only one of them can be true: a turn is running or it is not.
    Interrupt,
    /// `Tab`: give the keyboard to the other region.
    FocusNext,
    /// `Shift-Tab`: the same, backwards.
    FocusPrev,
    /// `a` on a selected pending card: run that call.
    Approve,
    /// `A` on a selected pending card: run every call still waiting.
    ApproveAll,
    /// `r` on a selected pending card: open the reason field.
    OpenReason,
    /// `Enter` in a one-line prompt: save the title, or send the reason.
    PromptSubmit,
    /// `Esc` in a prompt: close it, keeping what was typed where that matters.
    CancelPrompt,
    /// `y` in a confirmation.
    ConfirmYes,
    /// `n` in a confirmation.
    ConfirmNo,
    /// `Esc` in a confirmation: back to the overlay, selection intact.
    CancelConfirm,
    /// The next / previous pending call.
    SelectNext,
    SelectPrev,
    /// `Enter` on a selected card: read the whole body in its own view.
    OpenDetail,
    /// `Esc` in the detail view: back to the transcript.
    CloseDetail,
    /// `Ctrl-S`: the sessions overlay.
    OpenSessions,
    /// `Ctrl-W`: the workspace overlay.
    OpenWorkspaces,
    /// `Ctrl-N`: a new chat, or a new workspace from the workspace overlay.
    NewSession,
    NewWorkspace,
    /// `Ctrl-R`: rename the selected session (the open one, from `chat`).
    RenameSession,
    /// `Ctrl-D` or `Del`: remove the selected row, behind a confirmation.
    DeleteSelected,
    /// Move the overlay's selection one row.
    ListNext,
    ListPrev,
    /// Move the overlay's selection a page.
    ListPageDown,
    ListPageUp,
    /// `Enter` on an overlay row: open it, or descend into its screen.
    OverlayOpen,
    /// `Esc` in an overlay: close it, or step back a screen first.
    CloseOverlay,
    /// `Esc` in the workspaces view: back to the chat.
    CloseManage,
    /// `Tab` on the create screen: put the highlighted directory in the field.
    AcceptCandidate,
}

/// The command `key` runs in `mode` with `focus`.
///
/// Global bindings are checked before the mode's own map.
pub fn command(mode: Mode, focus: Focus, key: &KeyEvent) -> Option<Command> {
    if let Some(global) = global(key) {
        return Some(global);
    }
    match mode {
        Mode::Chat => chat(focus, key),
        Mode::Detail => detail(key),
        Mode::Prompt => prompt(key),
        Mode::Sessions => sessions(key),
        Mode::Workspaces => workspaces(key),
        Mode::Manage => manage(key),
        Mode::WorkspaceMissing => missing(key),
        Mode::Confirm => confirm(key),
        // A mode that owns its keys from the phase that adds it: an unmapped key
        // does nothing rather than falling through to a stale binding.
        _ => None,
    }
}

/// The detail view: one call, read in full.
///
/// Every scrolling key the transcript has, with the transcript's own meaning.
/// `q` is absent, as it is everywhere but `chat`, so reading a card cannot leave
/// the app.
fn detail(key: &KeyEvent) -> Option<Command> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Esc if !alt => Some(Command::CloseDetail),
        KeyCode::Char('j') if !control => Some(Command::LineDown),
        KeyCode::Char('k') if !control => Some(Command::LineUp),
        KeyCode::Down if !control => Some(Command::LineDown),
        KeyCode::Up if !control => Some(Command::LineUp),
        KeyCode::PageDown => Some(Command::PageDown),
        KeyCode::PageUp => Some(Command::PageUp),
        KeyCode::Char('d') if control => Some(Command::HalfPageDown),
        KeyCode::Char('u') if control => Some(Command::HalfPageUp),
        KeyCode::Home => Some(Command::Top),
        KeyCode::End => Some(Command::Bottom),
        _ => None,
    }
}

/// The one-line prompt. Everything printable belongs to the text area.
fn prompt(key: &KeyEvent) -> Option<Command> {
    if key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    match key.code {
        KeyCode::Enter => Some(Command::PromptSubmit),
        KeyCode::Esc => Some(Command::CancelPrompt),
        _ => None,
    }
}

/// The confirmation above an overlay.
///
/// Nothing else is bound, and nothing printable reaches a field: the overlay
/// that opened this is underneath and does not have the keyboard.
fn confirm(key: &KeyEvent) -> Option<Command> {
    if key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(Command::ConfirmYes),
        KeyCode::Char('n') | KeyCode::Char('N') => Some(Command::ConfirmNo),
        KeyCode::Esc => Some(Command::CancelConfirm),
        _ => None,
    }
}

/// The sessions overlay: a filter field with a list under it.
fn sessions(key: &KeyEvent) -> Option<Command> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if control {
        return match key.code {
            KeyCode::Char('s') => Some(Command::OpenSessions),
            KeyCode::Char('w') => Some(Command::OpenWorkspaces),
            KeyCode::Char('n') => Some(Command::NewSession),
            KeyCode::Char('r') => Some(Command::RenameSession),
            KeyCode::Char('d') => Some(Command::DeleteSelected),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Esc if !alt => Some(Command::CloseOverlay),
        KeyCode::Enter if !alt => Some(Command::OverlayOpen),
        KeyCode::Up if !alt => Some(Command::ListPrev),
        KeyCode::Down if !alt => Some(Command::ListNext),
        KeyCode::PageUp => Some(Command::ListPageUp),
        KeyCode::PageDown => Some(Command::ListPageDown),
        KeyCode::Delete => Some(Command::DeleteSelected),
        // Everything else is the filter's: `j` and `k` included.
        _ => None,
    }
}

/// The workspaces view: a filter with a list under it.
///
/// The same control-key rule as the overlay, because the filter is a field: every
/// other printable key is text.
fn manage(key: &KeyEvent) -> Option<Command> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if control {
        return match key.code {
            KeyCode::Char('s') => Some(Command::OpenSessions),
            KeyCode::Char('w') => Some(Command::OpenWorkspaces),
            KeyCode::Char('n') => Some(Command::NewWorkspace),
            KeyCode::Char('d') => Some(Command::DeleteSelected),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Esc if !alt => Some(Command::CloseManage),
        KeyCode::Enter if !alt => Some(Command::OverlayOpen),
        KeyCode::Up if !alt => Some(Command::ListPrev),
        KeyCode::Down if !alt => Some(Command::ListNext),
        KeyCode::PageUp => Some(Command::ListPageUp),
        KeyCode::PageDown => Some(Command::ListPageDown),
        KeyCode::Delete => Some(Command::DeleteSelected),
        _ => None,
    }
}

/// The startup prompt: create the path's workspace, or go and pick another.
///
/// Two answers and nothing else: this is a question the process stopped to ask,
/// and every other key would be a third answer nobody wrote down.
fn missing(key: &KeyEvent) -> Option<Command> {
    if key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(Command::ConfirmYes),
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(Command::ConfirmNo),
        _ => None,
    }
}

/// The workspace switcher: two screens, one map.
///
/// The screen decides what `Enter` means, which only `update` knows, so the map
/// stays one table and the handler branches.
fn workspaces(key: &KeyEvent) -> Option<Command> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if control {
        return match key.code {
            KeyCode::Char('s') => Some(Command::OpenSessions),
            KeyCode::Char('w') => Some(Command::OpenWorkspaces),
            KeyCode::Char('n') => Some(Command::NewWorkspace),
            KeyCode::Char('d') => Some(Command::DeleteSelected),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Esc if !alt => Some(Command::CloseOverlay),
        KeyCode::Enter if !alt => Some(Command::OverlayOpen),
        KeyCode::Tab if !alt => Some(Command::AcceptCandidate),
        KeyCode::Up if !alt => Some(Command::ListPrev),
        KeyCode::Down if !alt => Some(Command::ListNext),
        KeyCode::PageUp => Some(Command::ListPageUp),
        KeyCode::PageDown => Some(Command::ListPageDown),
        KeyCode::Delete => Some(Command::DeleteSelected),
        _ => None,
    }
}

/// Keys that mean the same thing in every mode.
///
/// `Esc` is deliberately not here: in `chat` it interrupts, but in an overlay it
/// means "close" and in a prompt "cancel". A global binding cannot carry that.
fn global(key: &KeyEvent) -> Option<Command> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        // Paused, this refuses the batch; busy, it stops the turn; an overlay,
        // it closes it; idle, it quits. `update` decides.
        KeyCode::Char('c') if control => Some(Command::StopOrQuit),
        _ => None,
    }
}

fn chat(focus: Focus, key: &KeyEvent) -> Option<Command> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        KeyCode::Tab if shift => return Some(Command::FocusPrev),
        KeyCode::Tab => return Some(Command::FocusNext),
        // The interrupt, from either focus: a turn is running or it is not, and
        // that does not depend on which region has the keyboard.
        KeyCode::Esc if !alt => return Some(Command::Interrupt),
        _ => {}
    }

    // The overlay keys, from either focus. They are control keys, so they are
    // never text and never the caret's.
    if control {
        match key.code {
            KeyCode::Char('s') => return Some(Command::OpenSessions),
            KeyCode::Char('w') => return Some(Command::OpenWorkspaces),
            KeyCode::Char('n') => return Some(Command::NewSession),
            KeyCode::Char('r') => return Some(Command::RenameSession),
            _ => {}
        }
    }

    if focus.is_composer() {
        return match key.code {
            // `Alt-Enter` is the documented newline. `Ctrl-J` is the fallback
            // for terminals that deliver Alt-Enter as `Esc`, `Enter`.
            KeyCode::Enter if alt => Some(Command::Newline),
            KeyCode::Char('j') if control => Some(Command::Newline),
            KeyCode::Enter if !control && !alt => Some(Command::Submit),
            // An empty field has no caret to move, so `Up` reaches the transcript
            // and selects the last block. With text in it, `update` keeps the
            // arrows for the caret — a multi-line draft needs them.
            KeyCode::Up if !control && !alt => Some(Command::SelectPrev),
            KeyCode::Down if !control && !alt => Some(Command::LineDown),
            _ => None,
        };
    }

    // The transcript, which has no text field to feed. Every letter here is
    // free to act on the selected block.
    match key.code {
        // The arrows select; they no longer scroll a line at a time.
        KeyCode::Up if !control && !alt => Some(Command::SelectPrev),
        KeyCode::Down if !control && !alt => Some(Command::SelectNext),
        KeyCode::Char('k') if !control && !alt => Some(Command::SelectPrev),
        KeyCode::Char('j') if !control && !alt => Some(Command::SelectNext),
        // `Enter` opens a card. On prose it does nothing, which `update` decides.
        KeyCode::Enter if !control && !alt => Some(Command::OpenDetail),
        // The approval actions are only meaningful on a pending card; `update`
        // ignores them anywhere else rather than removing the letters.
        KeyCode::Char('a') if !control && !alt => Some(Command::Approve),
        KeyCode::Char('A') if !control => Some(Command::ApproveAll),
        KeyCode::Char('r') if !control && !alt => Some(Command::OpenReason),
        // Quit is idle-only and transcript-only; `update` checks the phase.
        KeyCode::Char('q') if !control && !alt => Some(Command::Quit),
        KeyCode::Char('u') if control => Some(Command::HalfPageUp),
        KeyCode::Char('d') if control => Some(Command::HalfPageDown),
        KeyCode::PageUp => Some(Command::PageUp),
        KeyCode::PageDown => Some(Command::PageDown),
        KeyCode::Home => Some(Command::Top),
        KeyCode::End => Some(Command::Bottom),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn control(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn alt(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::ALT)
    }

    #[test]
    fn the_chat_map_is_one_key_one_command_per_focus() {
        // Enter sends; Alt-Enter and Ctrl-J insert a newline instead.
        assert_eq!(
            command(Mode::Chat, Focus::Composer, &press(KeyCode::Enter)),
            Some(Command::Submit)
        );
        assert_eq!(
            command(Mode::Chat, Focus::Composer, &alt(KeyCode::Enter)),
            Some(Command::Newline)
        );
        assert_eq!(
            command(Mode::Chat, Focus::Composer, &control(KeyCode::Char('j'))),
            Some(Command::Newline)
        );

        // `q` is a character in the composer and quit in the transcript.
        assert_eq!(
            command(Mode::Chat, Focus::Composer, &press(KeyCode::Char('q'))),
            None
        );
        assert_eq!(
            command(Mode::Chat, Focus::Transcript, &press(KeyCode::Char('q'))),
            Some(Command::Quit)
        );

        // Ctrl-C is global, from either focus and from any mode.
        for focus in [Focus::Composer, Focus::Transcript] {
            assert_eq!(
                command(Mode::Chat, focus, &control(KeyCode::Char('c'))),
                Some(Command::StopOrQuit)
            );
        }
        for mode in [
            Mode::Sessions,
            Mode::Workspaces,
            Mode::Confirm,
            Mode::Settings,
            Mode::Detail,
        ] {
            assert_eq!(
                command(mode, Focus::Transcript, &control(KeyCode::Char('c'))),
                Some(Command::StopOrQuit)
            );
            assert_eq!(
                command(mode, Focus::Transcript, &press(KeyCode::Char('q'))),
                None,
                "q is a chat binding, not a global one"
            );
        }

        // Tab swaps focus; the shift variant goes back.
        assert_eq!(
            command(Mode::Chat, Focus::Composer, &press(KeyCode::Tab)),
            Some(Command::FocusNext)
        );
        assert_eq!(
            command(
                Mode::Chat,
                Focus::Transcript,
                &KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT)
            ),
            Some(Command::FocusPrev)
        );

        // Arrows select from either focus; `update` gives them back to the text
        // area while the composer holds text, where they are the caret's.
        for focus in [Focus::Composer, Focus::Transcript] {
            assert_eq!(
                command(Mode::Chat, focus, &press(KeyCode::Up)),
                Some(Command::SelectPrev),
                "{focus:?}"
            );
        }
        assert_eq!(
            command(Mode::Chat, Focus::Transcript, &press(KeyCode::Down)),
            Some(Command::SelectNext)
        );
        // Left and right stay with the caret, which is the one thing a text
        // field cannot lose.
        for code in [KeyCode::Left, KeyCode::Right] {
            for focus in [Focus::Composer, Focus::Transcript] {
                assert_eq!(
                    command(Mode::Chat, focus, &press(code)),
                    None,
                    "{code:?} belongs to the text area"
                );
            }
        }
        let scroll = [
            (press(KeyCode::PageUp), Command::PageUp),
            (press(KeyCode::PageDown), Command::PageDown),
            (control(KeyCode::Char('u')), Command::HalfPageUp),
            (control(KeyCode::Char('d')), Command::HalfPageDown),
            (press(KeyCode::Home), Command::Top),
            (press(KeyCode::End), Command::Bottom),
        ];
        for (key, expected) in scroll {
            assert_eq!(
                command(Mode::Chat, Focus::Transcript, &key),
                Some(expected),
                "{key:?}"
            );
            assert_eq!(
                command(Mode::Chat, Focus::Composer, &key),
                None,
                "{key:?} is the transcript's while the composer has focus"
            );
        }
    }

    #[test]
    fn the_overlay_keys_are_control_keys_from_either_chat_focus() {
        for focus in [Focus::Composer, Focus::Transcript] {
            let map = [
                (control(KeyCode::Char('s')), Command::OpenSessions),
                (control(KeyCode::Char('w')), Command::OpenWorkspaces),
                (control(KeyCode::Char('n')), Command::NewSession),
                (control(KeyCode::Char('r')), Command::RenameSession),
            ];
            for (key, expected) in map {
                assert_eq!(command(Mode::Chat, focus, &key), Some(expected), "{key:?}");
            }
            // The bare letters stay text or the transcript's, unchanged.
            assert_eq!(
                command(Mode::Chat, focus, &press(KeyCode::Char('s'))),
                None,
                "{focus:?}"
            );
        }
    }

    #[test]
    fn esc_interrupts_from_either_focus_and_never_quits() {
        for focus in [Focus::Composer, Focus::Transcript] {
            assert_eq!(
                command(Mode::Chat, focus, &press(KeyCode::Esc)),
                Some(Command::Interrupt),
                "{focus:?}"
            );
        }
        // The prompt's and the detail view's `Esc` are their own commands, so
        // those close without stopping anything.
        assert_eq!(
            command(Mode::Prompt, Focus::Composer, &press(KeyCode::Esc)),
            Some(Command::CancelPrompt)
        );
        assert_eq!(
            command(Mode::Detail, Focus::Transcript, &press(KeyCode::Esc)),
            Some(Command::CloseDetail)
        );
    }

    #[test]
    fn the_transcript_acts_on_the_selected_block() {
        let transcript = Focus::Transcript;
        let map = [
            (press(KeyCode::Up), Command::SelectPrev),
            (press(KeyCode::Down), Command::SelectNext),
            (press(KeyCode::Char('k')), Command::SelectPrev),
            (press(KeyCode::Char('j')), Command::SelectNext),
            (press(KeyCode::Enter), Command::OpenDetail),
            (press(KeyCode::Char('a')), Command::Approve),
            (press(KeyCode::Char('r')), Command::OpenReason),
        ];
        for (key, expected) in map {
            assert_eq!(
                command(Mode::Chat, transcript, &key),
                Some(expected),
                "{key:?}"
            );
        }
        assert_eq!(
            command(
                Mode::Chat,
                transcript,
                &KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)
            ),
            Some(Command::ApproveAll)
        );
        // `q` is still chat-and-transcript only.
        assert_eq!(
            command(Mode::Chat, transcript, &press(KeyCode::Char('q'))),
            Some(Command::Quit)
        );
    }

    #[test]
    fn the_composer_keeps_its_text_and_hands_an_empty_up_to_the_transcript() {
        // With the field holding text the arrows are the caret's; `update` makes
        // that call, and the map is the same either way.
        assert_eq!(
            command(Mode::Chat, Focus::Composer, &press(KeyCode::Up)),
            Some(Command::SelectPrev),
            "an empty field has no caret, so Up reaches the transcript"
        );
        assert_eq!(
            command(Mode::Chat, Focus::Composer, &press(KeyCode::Enter)),
            Some(Command::Submit)
        );
        // The approval letters are not text: `a` in the field is a character.
        for key in [
            press(KeyCode::Char('a')),
            press(KeyCode::Char('r')),
            press(KeyCode::Char('j')),
        ] {
            assert_eq!(
                command(Mode::Chat, Focus::Composer, &key),
                None,
                "{key:?} is text while the composer has focus"
            );
        }
    }

    #[test]
    fn the_prompt_takes_enter_and_esc_and_hands_the_rest_to_the_field() {
        let map = [
            (press(KeyCode::Enter), Command::PromptSubmit),
            (press(KeyCode::Esc), Command::CancelPrompt),
        ];
        for (key, expected) in map {
            assert_eq!(
                command(Mode::Prompt, Focus::Composer, &key),
                Some(expected),
                "{key:?}"
            );
        }
        for key in [
            press(KeyCode::Char('a')),
            press(KeyCode::Char('r')),
            press(KeyCode::Char('j')),
            press(KeyCode::Char('q')),
            press(KeyCode::Char('A')),
        ] {
            assert_eq!(
                command(Mode::Prompt, Focus::Composer, &key),
                None,
                "{key:?} is text in the prompt"
            );
        }
    }

    #[test]
    fn the_sessions_overlay_keeps_letters_for_the_filter() {
        for (key, expected) in [
            (press(KeyCode::Esc), Command::CloseOverlay),
            (press(KeyCode::Enter), Command::OverlayOpen),
            (press(KeyCode::Up), Command::ListPrev),
            (press(KeyCode::Down), Command::ListNext),
            (press(KeyCode::PageUp), Command::ListPageUp),
            (press(KeyCode::PageDown), Command::ListPageDown),
            (control(KeyCode::Char('r')), Command::RenameSession),
            (control(KeyCode::Char('d')), Command::DeleteSelected),
            (press(KeyCode::Delete), Command::DeleteSelected),
            (control(KeyCode::Char('n')), Command::NewSession),
            (control(KeyCode::Char('w')), Command::OpenWorkspaces),
        ] {
            assert_eq!(command(Mode::Sessions, Focus::Composer, &key), Some(expected), "{key:?}");
        }
        // The filter owns every other printable key, `j` and `k` included.
        for key in [
            press(KeyCode::Char('j')),
            press(KeyCode::Char('k')),
            press(KeyCode::Char('q')),
            press(KeyCode::Char('a')),
            press(KeyCode::Char('y')),
        ] {
            assert_eq!(
                command(Mode::Sessions, Focus::Composer, &key),
                None,
                "{key:?} is filter text"
            );
        }
    }

    #[test]
    fn the_workspace_overlay_binds_the_three_screens_it_draws() {
        for (key, expected) in [
            (press(KeyCode::Esc), Command::CloseOverlay),
            (press(KeyCode::Enter), Command::OverlayOpen),
            (press(KeyCode::Up), Command::ListPrev),
            (press(KeyCode::Down), Command::ListNext),
            (press(KeyCode::Tab), Command::AcceptCandidate),
            (control(KeyCode::Char('n')), Command::NewWorkspace),
            (control(KeyCode::Char('d')), Command::DeleteSelected),
            (control(KeyCode::Char('s')), Command::OpenSessions),
        ] {
            assert_eq!(
                command(Mode::Workspaces, Focus::Composer, &key),
                Some(expected),
                "{key:?}"
            );
        }
        assert_eq!(
            command(Mode::Workspaces, Focus::Composer, &press(KeyCode::Char('y'))),
            None,
            "the path and the filter are text"
        );
        assert_eq!(
            command(Mode::Workspaces, Focus::Composer, &press(KeyCode::Char('n'))),
            None,
            "a bare n is a character, not a new workspace"
        );
    }

    #[test]
    fn the_confirmation_answers_y_n_and_esc_and_nothing_else() {
        for (key, expected) in [
            (press(KeyCode::Char('y')), Command::ConfirmYes),
            (press(KeyCode::Char('Y')), Command::ConfirmYes),
            (press(KeyCode::Char('n')), Command::ConfirmNo),
            (press(KeyCode::Char('N')), Command::ConfirmNo),
            (press(KeyCode::Esc), Command::CancelConfirm),
        ] {
            assert_eq!(command(Mode::Confirm, Focus::Composer, &key), Some(expected), "{key:?}");
        }
        for key in [
            press(KeyCode::Enter),
            press(KeyCode::Char('a')),
            press(KeyCode::Up),
        ] {
            assert_eq!(
                command(Mode::Confirm, Focus::Composer, &key),
                None,
                "{key:?} means nothing while a confirmation is up"
            );
        }
    }

    #[test]
    fn the_detail_view_scrolls_and_does_not_act_on_blocks() {
        let map = [
            (press(KeyCode::Esc), Command::CloseDetail),
            (press(KeyCode::Char('j')), Command::LineDown),
            (press(KeyCode::Char('k')), Command::LineUp),
            (press(KeyCode::Down), Command::LineDown),
            (press(KeyCode::Up), Command::LineUp),
            (press(KeyCode::PageDown), Command::PageDown),
            (press(KeyCode::PageUp), Command::PageUp),
            (control(KeyCode::Char('d')), Command::HalfPageDown),
            (control(KeyCode::Char('u')), Command::HalfPageUp),
            (press(KeyCode::Home), Command::Top),
            (press(KeyCode::End), Command::Bottom),
        ];
        for (key, expected) in map {
            assert_eq!(
                command(Mode::Detail, Focus::Transcript, &key),
                Some(expected),
                "{key:?}"
            );
        }
        assert_eq!(
            command(Mode::Detail, Focus::Transcript, &press(KeyCode::Char('q'))),
            None,
            "reading a card cannot leave the app"
        );
        assert_eq!(
            command(Mode::Detail, Focus::Transcript, &press(KeyCode::Char('a'))),
            None,
            "the approval keys belong to the transcript"
        );
    }
}
