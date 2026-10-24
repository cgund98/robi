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
    /// `Ctrl-C`: stop the running turn, or quit when idle.
    StopOrQuit,
    /// `Tab`: give the keyboard to the other region.
    FocusNext,
    /// `Shift-Tab`: the same, backwards.
    FocusPrev,
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
        // T2 fills the approval modal. Every other mode owns its keys from the
        // phase that adds it, so an unmapped key here does nothing rather than
        // falling through to a stale binding.
        _ => None,
    }
}

/// Keys that mean the same thing in every mode.
fn global(key: &KeyEvent) -> Option<Command> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        // Busy, this stops the turn; idle, it quits. `update` decides by phase,
        // because only it knows whether anything is running.
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
        _ => {}
    }

    if focus.is_composer() {
        return match key.code {
            // `Alt-Enter` is the documented newline. `Ctrl-J` is the fallback
            // for terminals that deliver Alt-Enter as `Esc`, `Enter`.
            KeyCode::Enter if alt => Some(Command::Newline),
            KeyCode::Char('j') if control => Some(Command::Newline),
            KeyCode::Enter if !control && !alt => Some(Command::Submit),
            // Arrows are the transcript's unless `update` hands them back.
            KeyCode::Up if !control && !alt => Some(Command::LineUp),
            KeyCode::Down if !control && !alt => Some(Command::LineDown),
            _ => None,
        };
    }

    // The transcript, which has no text field to feed.
    match key.code {
        // Quit is idle-only and transcript-only; `update` checks the phase.
        KeyCode::Char('q') if !control && !alt => Some(Command::Quit),
        KeyCode::Up => Some(Command::LineUp),
        KeyCode::Down => Some(Command::LineDown),
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
        for mode in [Mode::Sessions, Mode::Approval, Mode::Settings] {
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

        // Arrows scroll from either focus; `update` gives them back to the
        // text area while the composer holds text.
        for focus in [Focus::Composer, Focus::Transcript] {
            assert_eq!(
                command(Mode::Chat, focus, &press(KeyCode::Up)),
                Some(Command::LineUp),
                "{focus:?}"
            );
            assert_eq!(
                command(Mode::Chat, focus, &press(KeyCode::Down)),
                Some(Command::LineDown),
                "{focus:?}"
            );
        }
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
}
