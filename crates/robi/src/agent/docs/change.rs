//! CodeMirror change sets and the version cache.
//!
//! The editor sends deltas, not the whole buffer. A delta is a list of
//! [`ChangeRange`]s against the base text the client started from, in UTF-16
//! code units (CodeMirror's own unit). [`apply_changes`] turns that back into
//! text. [`DocsEditCache`] remembers the bytes behind a version string, so a
//! later delta can be applied to a base the server no longer has on disk.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// How many versions one workspace keeps. Enough for a burst of autosaves and
/// their responses; a miss is a re-send, not an error.
pub const DEFAULT_CACHE_ENTRIES: usize = 64;

/// One change: replace the base range `from..to` with `insert`.
///
/// `from` and `to` count UTF-16 code units into the base text, matching
/// CodeMirror's `ChangeSet.iterChanges`. Ranges are ordered and do not overlap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ChangeRange {
    pub from: usize,
    pub to: usize,
    #[serde(default)]
    pub insert: String,
}

/// `sha256:<hex>` over the UTF-8 bytes of `text`.
pub fn version_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    format!("sha256:{}", hex(&digest))
}

/// Apply `ranges` to `base`.
///
/// Fails when a range points past the end of `base`, or when the ranges are
/// not ordered and non-overlapping.
pub fn apply_changes(base: &str, ranges: &[ChangeRange]) -> Result<String, String> {
    let map = utf16_byte_map(base);
    let mut out = String::with_capacity(base.len());
    let mut cursor = 0usize;
    for range in ranges {
        let from = *map
            .get(range.from)
            .ok_or_else(|| "change offset is past the end of the base text".to_owned())?;
        let to = *map
            .get(range.to)
            .ok_or_else(|| "change offset is past the end of the base text".to_owned())?;
        if to < from {
            return Err("a change range ends before it starts".to_owned());
        }
        if from < cursor {
            return Err("change ranges must be ordered and non-overlapping".to_owned());
        }
        out.push_str(&base[cursor..from]);
        out.push_str(&range.insert);
        cursor = to;
    }
    out.push_str(&base[cursor..]);
    Ok(out)
}

/// `byte_at[i]` is the byte offset of the `i`th UTF-16 code-unit boundary.
///
/// The last entry is `text.len()`. A boundary inside a surrogate pair maps to
/// the start of the character, which is what a well-formed change set sends.
fn utf16_byte_map(text: &str) -> Vec<usize> {
    let total: usize = text.chars().map(char::len_utf16).sum();
    let mut map = vec![0usize; total + 1];
    let mut utf16 = 0usize;
    for (byte, ch) in text.char_indices() {
        map[utf16] = byte;
        let units = ch.len_utf16();
        for offset in 1..units {
            map[utf16 + offset] = byte;
        }
        utf16 += units;
    }
    map[utf16] = text.len();
    map
}

/// A bounded `version -> text` cache, most-recently-used first.
///
/// Every successful read and write seeds it. A later delta names the version
/// it was built from; a miss means the base is gone and the client re-sends.
pub struct DocsEditCache {
    entries: Mutex<Inner>,
    capacity: usize,
}

struct Inner {
    map: HashMap<String, Arc<str>>,
    order: VecDeque<String>,
}

impl DocsEditCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(Inner {
                map: HashMap::new(),
                order: VecDeque::new(),
            }),
            capacity: capacity.max(1),
        }
    }

    /// Store `text` under its content hash and return that version.
    pub fn insert(&self, text: &str) -> String {
        let version = version_hash(text);
        self.put(version.clone(), text);
        version
    }

    /// Store `text` under a version its caller already computed.
    pub fn put(&self, version: String, text: &str) {
        let mut inner = self.entries.lock().unwrap_or_else(|err| err.into_inner());
        if inner.map.contains_key(&version) {
            // Refresh recency.
            if let Some(index) = inner.order.iter().position(|key| *key == version) {
                inner.order.remove(index);
            }
        }
        inner.order.push_back(version.clone());
        inner.map.insert(version, Arc::from(text));
        while inner.map.len() > self.capacity {
            let Some(oldest) = inner.order.pop_front() else {
                break;
            };
            inner.map.remove(&oldest);
        }
    }

    pub fn get(&self, version: &str) -> Option<Arc<str>> {
        let mut inner = self.entries.lock().unwrap_or_else(|err| err.into_inner());
        let text = inner.map.get(version).cloned();
        if text.is_some() {
            if let Some(index) = inner.order.iter().position(|key| key == version) {
                inner.order.remove(index);
            }
            inner.order.push_back(version.to_owned());
        }
        text
    }
}

impl Default for DocsEditCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_ENTRIES)
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_inserts_replaces_and_deletes() {
        let base = "hello world";
        // Replace "world" (bytes 6..11) with "there".
        let ranges = vec![ChangeRange {
            from: 6,
            to: 11,
            insert: "there".to_owned(),
        }];
        assert_eq!(apply_changes(base, &ranges).unwrap(), "hello there");
        // Insert without deleting.
        let ranges = vec![ChangeRange {
            from: 5,
            to: 5,
            insert: ",".to_owned(),
        }];
        assert_eq!(apply_changes(base, &ranges).unwrap(), "hello, world");
        // Delete.
        let ranges = vec![ChangeRange {
            from: 5,
            to: 6,
            insert: String::new(),
        }];
        assert_eq!(apply_changes(base, &ranges).unwrap(), "helloworld");
    }

    #[test]
    fn apply_handles_an_empty_base() {
        let ranges = vec![ChangeRange {
            from: 0,
            to: 0,
            insert: "new".to_owned(),
        }];
        assert_eq!(apply_changes("", &ranges).unwrap(), "new");
    }

    /// The classic bug: an edit that lands after a surrogate pair must count
    /// UTF-16 units, not bytes or chars.
    #[test]
    fn offsets_are_utf16_code_units_not_bytes() {
        // "😀" is one char, two UTF-16 units, four bytes.
        let base = "a😀b";
        // In UTF-16: a(0) surrogate(1,2) b(3). Replace "b" -> offset 3..4.
        let ranges = vec![ChangeRange {
            from: 3,
            to: 4,
            insert: "c".to_owned(),
        }];
        assert_eq!(apply_changes(base, &ranges).unwrap(), "a😀c");

        // Insert right after the emoji: UTF-16 offset 3.
        let ranges = vec![ChangeRange {
            from: 3,
            to: 3,
            insert: "!".to_owned(),
        }];
        assert_eq!(apply_changes(base, &ranges).unwrap(), "a😀!b");

        // A boundary that points inside the surrogate pair maps to the char
        // start rather than panicking.
        let ranges = vec![ChangeRange {
            from: 2,
            to: 2,
            insert: "!".to_owned(),
        }];
        assert_eq!(apply_changes(base, &ranges).unwrap(), "a!😀b");
    }

    #[test]
    fn out_of_range_and_unordered_are_refused() {
        let ranges = vec![ChangeRange {
            from: 0,
            to: 99,
            insert: String::new(),
        }];
        assert!(apply_changes("short", &ranges).is_err());
        let ranges = vec![
            ChangeRange {
                from: 2,
                to: 3,
                insert: String::new(),
            },
            ChangeRange {
                from: 0,
                to: 1,
                insert: String::new(),
            },
        ];
        assert!(apply_changes("abcd", &ranges).is_err());
    }

    #[test]
    fn the_version_is_stable_and_prefixed() {
        let version = version_hash("hello");
        assert!(version.starts_with("sha256:"));
        assert_eq!(version, version_hash("hello"));
        assert_ne!(version, version_hash("hello "));
    }

    #[test]
    fn the_cache_returns_what_it_stored_and_evicts_the_oldest() {
        let cache = DocsEditCache::new(2);
        let first = cache.insert("one");
        let second = cache.insert("two");
        assert_eq!(&*cache.get(&first).unwrap(), "one");
        // Touch `first` so `second` is the least recent.
        let third = cache.insert("three");
        assert_eq!(&*cache.get(&first).unwrap(), "one");
        assert!(cache.get(&second).is_none());
        assert_eq!(&*cache.get(&third).unwrap(), "three");
    }
}
