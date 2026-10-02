//! Identifier newtypes.
//!
//! Every id is a UUIDv7, so the timestamp leads the bytes and ids created in
//! different milliseconds sort in creation order. Within one millisecond the
//! remaining bits are random, so id order is not a total order over a transcript;
//! a store keeps insertion order and uses ids for identity.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Mint a new id.
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Wrap an existing UUID, for a store reading an id back.
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            pub fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

define_id!(
    /// Identifies a chat session.
    SessionId
);
define_id!(
    /// Identifies one message in a transcript.
    MessageId
);
define_id!(
    /// Identifies one tool call within an assistant message.
    ToolCallId
);
define_id!(
    /// Identifies a workspace root.
    WorkspaceId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let a = MessageId::new();
        let b = MessageId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn ids_created_in_different_milliseconds_sort_by_time() {
        let first = WorkspaceId::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = WorkspaceId::new();
        assert!(first < second, "UUIDv7 leads with the timestamp");
    }

    #[test]
    fn ids_round_trip_through_uuid() {
        let id = ToolCallId::new();
        assert_eq!(ToolCallId::from_uuid(id.as_uuid()), id);
    }
}
