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
    /// Send the composer's text. `update` refuses a blank or a busy turn.
    Submit,
    /// A newline in the composer, without sending it.
    Newline,
    /// `Ctrl-C`: refuse a paused batch, stop the running turn, or quit when idle.
    ///
    /// `update` decides which, because it is the only place that knows whether a
    /// modal is up and whether anything is running.
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
    /// `Enter` in the reason field: refuse the selected call with what was typed.
    RejectWithReason,
    /// The next / previous pending call.
    SelectNext,
    SelectPrev,
    /// `Esc` in the reason field: back to the card, keeping what was typed.
    ClosePrompt,
    /// `Enter` on a selected card: read the whole body in its own view.
    OpenDetail,
    /// `Esc` in the detail view: back to the transcript.
    CloseDetail,
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
        // Every other mode owns its keys from the phase that adds it, so an
        // unmapped key here does nothing rather than falling through to a stale
        // binding.
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

/// The reason field. Everything printable belongs to the text area.
fn prompt(key: &KeyEvent) -> Option<Command> {
    if key.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    match key.code {
        KeyCode::Enter => Some(Command::RejectWithReason),
        KeyCode::Esc => Some(Command::ClosePrompt),
        _ => None,
    }
}

/// Keys that mean the same thing in every mode.
///
/// `Esc` is deliberately not here: in `chat` it interrupts, but in the approval
/// modal it means "close the field" and in the modal with the field closed it
/// means nothing at all. A global binding cannot carry that.
fn global(key: &KeyEvent) -> Option<Command> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        // Paused, this refuses the batch; busy, it stops the turn; idle, it
        // quits. `update` decides, because only it knows which is true.
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
        KeyCode::PageUp => Some(Command::PageUp),
        KeyCode::PageDown => Some(Command::PageDown),
        KeyCode::Char('u') if control => Some(Command::HalfPageUp),
        KeyCode::Char('d') if control => Some(Command::HalfPageDown),
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

    /// One test for the whole map. A per-binding test would be a second place
    /// to edit every time a key moves, and would not catch the one thing that
    /// matters here: a key that means two things in one mode.
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
        for mode in [Mode::Sessions, Mode::Settings, Mode::Detail] {
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
            Some(Command::ClosePrompt)
        );
        assert_eq!(
            command(Mode::Detail, Focus::Transcript, &press(KeyCode::Esc)),
            Some(Command::CloseDetail)
        );
    }

    #[test]
    fn the_arrows_select_and_the_letters_act_on_the_selection() {
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
        assert_eq!(
            command(Mode::Chat, transcript, &press(KeyCode::Char('r'))),
            Some(Command::OpenReason),
            "one name per command, and `r` is the reject one"
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
            (press(KeyCode::Enter), Command::RejectWithReason),
            (press(KeyCode::Esc), Command::ClosePrompt),
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
                "{key:?} is text while the reason field has the keyboard"
            );
        }
    }

    #[test]
    fn the_detail_view_scrolls_and_cannot_quit() {
        let map = [
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
