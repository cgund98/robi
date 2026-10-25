//! The landing page: the greeting and what it says.
//!
//! The terminal's peer of `EmptyGreeting.tsx` and `greeting.ts`. It is the page
//! for a workspace with no session open — the moment before a chat exists — and
//! it is why `Ctrl-N` creates nothing: a session row appears when the first
//! message is sent, so opening a workspace and leaving never leaves an empty
//! session behind.
//!
//! The clock is not in `update`. A greeting is read once when the process starts
//! and carried as state, so `update` stays a pure function and a frame test gets
//! the same word every run.

use chrono::Timelike;

/// Time of day, in the three bands the web shell uses.
///
/// Morning ends at noon, afternoon at 5pm. `src/features/chat/greeting.ts` is the
/// other half of this table, and the two read the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Greeting {
    #[default]
    Morning,
    Afternoon,
    Evening,
}

impl Greeting {
    /// The band `hour` (0-23) falls in.
    pub fn at(hour: u32) -> Self {
        match hour {
            0..=11 => Self::Morning,
            12..=16 => Self::Afternoon,
            _ => Self::Evening,
        }
    }

    /// The band the wall clock is in, read once at startup.
    pub fn now() -> Self {
        Self::at(chrono::Local::now().hour())
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Morning => "Good morning",
            Self::Afternoon => "Good afternoon",
            Self::Evening => "Good evening",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bands_match_the_web_shells_greeting() {
        assert_eq!(Greeting::at(0), Greeting::Morning);
        assert_eq!(Greeting::at(11), Greeting::Morning);
        assert_eq!(Greeting::at(12), Greeting::Afternoon);
        assert_eq!(Greeting::at(16), Greeting::Afternoon);
        assert_eq!(Greeting::at(17), Greeting::Evening);
        assert_eq!(Greeting::at(23), Greeting::Evening);

        assert_eq!(Greeting::Morning.label(), "Good morning");
        assert_eq!(Greeting::Afternoon.label(), "Good afternoon");
        assert_eq!(Greeting::Evening.label(), "Good evening");
    }
}
