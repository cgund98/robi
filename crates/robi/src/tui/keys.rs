//! `(Mode, KeyEvent) -> Command`.
//!
//! A pure map, so "every binding in the design doc's table is reachable and no
//! key means two things in one mode" is a unit test rather than a manual check.
//!
//! Arrow keys always belong to the text area, which is why scrolling is
//! `PageUp`/`PageDown` and `Ctrl-U`/`Ctrl-D` rather than `Up`/`Down`: a
//! terminal chat keeps a text field on screen, and stealing its cursor is the
//! mistake every naive implementation makes.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::tui::state::Mode;

/// What a key asks for. `app::update` turns one into a state change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Leave, restoring the terminal.
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
}

/// The command `key` runs in `mode`.
///
/// Global bindings are checked before the mode's own map.
pub fn command(mode: Mode, key: &KeyEvent) -> Option<Command> {
    if let Some(global) = global(key) {
        return Some(global);
    }
    match mode {
        Mode::Chat => chat(key),
        // T1 fills the composer and T2 the approval modal. Every other mode
        // owns its keys from the phase that adds it, so an unmapped key here
        // does nothing rather than falling through to a stale binding.
        _ => None,
    }
}

/// Keys that mean the same thing in every mode.
fn global(key: &KeyEvent) -> Option<Command> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        // T0 runs no turn, so Ctrl-C has nothing to stop and means quit. T1
        // stops the running turn instead and quits on a second press.
        KeyCode::Char('c') if control => Some(Command::Quit),
        _ => None,
    }
}

fn chat(key: &KeyEvent) -> Option<Command> {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        // Quit is idle-only in the design's table, and T0 has no busy state.
        KeyCode::Char('q') if !control => Some(Command::Quit),
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

    #[test]
    fn q_and_ctrl_c_quit_from_chat() {
        assert_eq!(
            command(Mode::Chat, &press(KeyCode::Char('q'))),
            Some(Command::Quit)
        );
        assert_eq!(
            command(Mode::Chat, &control(KeyCode::Char('c'))),
            Some(Command::Quit)
        );
    }

    #[test]
    fn scrolling_is_not_on_the_arrow_keys() {
        for code in [KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right] {
            assert_eq!(
                command(Mode::Chat, &press(code)),
                None,
                "arrows belong to the text area"
            );
        }
    }

    #[test]
    fn every_scroll_binding_maps_to_one_command() {
        let cases = [
            (press(KeyCode::PageUp), Command::PageUp),
            (press(KeyCode::PageDown), Command::PageDown),
            (control(KeyCode::Char('u')), Command::HalfPageUp),
            (control(KeyCode::Char('d')), Command::HalfPageDown),
            (press(KeyCode::Home), Command::Top),
            (press(KeyCode::End), Command::Bottom),
        ];
        for (key, expected) in cases {
            assert_eq!(command(Mode::Chat, &key), Some(expected), "{key:?}");
        }
    }

    #[test]
    fn an_unmapped_key_does_nothing() {
        assert_eq!(command(Mode::Chat, &press(KeyCode::Char('j'))), None);
        assert_eq!(command(Mode::Chat, &press(KeyCode::Char('Q'))), None);
        assert_eq!(command(Mode::Chat, &press(KeyCode::Esc)), None);
    }

    #[test]
    fn a_mode_that_has_no_map_yet_still_quits_on_ctrl_c() {
        for mode in [Mode::Sessions, Mode::Approval, Mode::Settings] {
            assert_eq!(
                command(mode, &control(KeyCode::Char('c'))),
                Some(Command::Quit)
            );
            assert_eq!(
                command(mode, &press(KeyCode::Char('q'))),
                None,
                "q is a chat binding, not a global one"
            );
        }
    }
}
