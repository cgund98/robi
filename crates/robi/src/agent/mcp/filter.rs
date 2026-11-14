//! Which of a server's listed tools reach the session registry.
//!
//! A server entry may name the tools it contributes. The filter runs at
//! registration, so a tool that does not pass never puts a name, a description,
//! or a schema in front of the model.

use globset::{Glob, GlobMatcher};
use regex::Regex;

use super::names::registered_name;
use super::session::ListedTool;

const REGEX_PREFIX: &str = "re:";

#[derive(Debug, Clone)]
enum Matcher {
    Glob {
        pattern: String,
        matcher: GlobMatcher,
    },
    Regex {
        pattern: String,
        matcher: Regex,
    },
}

impl Matcher {
    /// A `re:` entry is a regex, anything else a glob. A glob matches the whole
    /// name, a regex matches a substring unless it anchors itself.
    fn compile(pattern: &str) -> Result<Self, String> {
        if let Some(body) = pattern.strip_prefix(REGEX_PREFIX) {
            let matcher = Regex::new(body)
                .map_err(|err| format!("tools pattern `{pattern}` is not a valid regex: {err}"))?;
            return Ok(Self::Regex {
                pattern: pattern.to_owned(),
                matcher,
            });
        }
        let matcher = Glob::new(pattern)
            .map_err(|err| format!("tools pattern `{pattern}` is not a valid glob: {err}"))?
            .compile_matcher();
        Ok(Self::Glob {
            pattern: pattern.to_owned(),
            matcher,
        })
    }

    fn pattern(&self) -> &str {
        match self {
            Self::Glob { pattern, .. } | Self::Regex { pattern, .. } => pattern,
        }
    }

    fn matches(&self, name: &str) -> bool {
        match self {
            Self::Glob { matcher, .. } => matcher.is_match(name),
            Self::Regex { matcher, .. } => matcher.is_match(name),
        }
    }
}

/// The `tools` field of one server entry.
///
/// `all()` keeps every tool, which is what an entry without `tools` means.
/// `new(&[])` keeps none, which is what `"tools": []` means.
#[derive(Debug, Clone, Default)]
pub struct ToolFilter {
    /// The patterns as written, or `None` when the entry has no `tools` field.
    patterns: Option<Vec<String>>,
    matchers: Vec<Matcher>,
}

/// Two entries are the same filter when they list the same patterns. A glob
/// matcher and a compiled regex are not comparable, so this compares the text.
impl PartialEq for ToolFilter {
    fn eq(&self, other: &Self) -> bool {
        self.patterns == other.patterns
    }
}

impl Eq for ToolFilter {}

impl ToolFilter {
    /// No filter: every tool the server lists is registered.
    pub fn all() -> Self {
        Self::default()
    }

    /// Compile the patterns. An error names the pattern that failed.
    pub fn new(patterns: &[String]) -> Result<Self, String> {
        let matchers = patterns
            .iter()
            .map(|pattern| Matcher::compile(pattern))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            patterns: Some(patterns.to_vec()),
            matchers,
        })
    }

    /// Whether this entry limits its server at all.
    pub fn is_filtered(&self) -> bool {
        self.patterns.is_some()
    }

    /// The tools that pass, and the patterns that matched nothing.
    ///
    /// A pattern is tried against the name the server lists and against the
    /// registered `mcp_<server>_<tool>` name, so either one works in a config.
    pub fn visible<'a>(
        &self,
        server: &str,
        tools: &'a [ListedTool],
    ) -> (Vec<&'a ListedTool>, Vec<String>) {
        if self.patterns.is_none() {
            return (tools.iter().collect(), Vec::new());
        }
        let matchers = &self.matchers;
        let mut used = vec![false; matchers.len()];
        let mut kept = Vec::new();
        for tool in tools {
            let registered = registered_name(server, &tool.name);
            let mut hit = false;
            for (index, matcher) in matchers.iter().enumerate() {
                if matcher.matches(&tool.name) || matcher.matches(&registered) {
                    used[index] = true;
                    hit = true;
                }
            }
            if hit {
                kept.push(tool);
            }
        }
        let unused = matchers
            .iter()
            .zip(used)
            .filter(|(_, used)| !used)
            .map(|(matcher, _)| matcher.pattern().to_owned())
            .collect();
        (kept, unused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str) -> ListedTool {
        ListedTool {
            name: name.into(),
            description: None,
            input_schema: Some(json!({"type": "object"})),
            read_only: true,
            destructive: false,
            open_world: false,
        }
    }

    fn tools() -> Vec<ListedTool> {
        vec![
            tool("get_issue"),
            tool("get_team"),
            tool("create_issue"),
            tool("search_code"),
        ]
    }

    fn kept(filter: &ToolFilter) -> Vec<String> {
        let listed = tools();
        filter
            .visible("linear", &listed)
            .0
            .into_iter()
            .map(|tool| tool.name.clone())
            .collect()
    }

    fn filter(patterns: &[&str]) -> ToolFilter {
        let patterns: Vec<String> = patterns.iter().map(|pattern| pattern.to_string()).collect();
        ToolFilter::new(&patterns).unwrap()
    }

    #[test]
    fn no_entry_keeps_every_tool() {
        let listed = tools();
        let (visible, unused) = ToolFilter::all().visible("linear", &listed);
        assert_eq!(visible.len(), listed.len());
        assert!(unused.is_empty());
        assert!(!ToolFilter::all().is_filtered());
    }

    #[test]
    fn an_empty_list_keeps_nothing() {
        assert!(kept(&filter(&[])).is_empty());
        assert!(filter(&[]).is_filtered());
    }

    #[test]
    fn an_exact_name_matches_itself_only() {
        assert_eq!(kept(&filter(&["create_issue"])), vec!["create_issue"]);
    }

    #[test]
    fn a_glob_matches_the_whole_name() {
        assert_eq!(kept(&filter(&["get_*"])), vec!["get_issue", "get_team"]);
        assert_eq!(kept(&filter(&["get_?eam"])), vec!["get_team"]);
        assert_eq!(
            kept(&filter(&["[cs]*"])),
            vec!["create_issue", "search_code"]
        );
        assert!(kept(&filter(&["issue"])).is_empty());
    }

    #[test]
    fn a_regex_entry_matches_a_substring() {
        assert_eq!(kept(&filter(&["re:^search_"])), vec!["search_code"]);
        assert_eq!(
            kept(&filter(&["re:issue$"])),
            vec!["get_issue", "create_issue"]
        );
        assert!(kept(&filter(&["re:^issue"])).is_empty());
    }

    #[test]
    fn a_pattern_may_name_the_registered_tool() {
        assert_eq!(
            kept(&filter(&["mcp_linear_get_*"])),
            vec!["get_issue", "get_team"]
        );
        assert_eq!(
            kept(&filter(&["re:^mcp_linear_create"])),
            vec!["create_issue"]
        );
    }

    #[test]
    fn an_invalid_pattern_names_itself() {
        let patterns = vec!["re:(".to_string()];
        let err = ToolFilter::new(&patterns).unwrap_err();
        assert!(err.contains("re:("), "{err}");
        let patterns = vec!["*[".to_string()];
        let err = ToolFilter::new(&patterns).unwrap_err();
        assert!(err.contains("*["), "{err}");
    }

    #[test]
    fn a_pattern_that_matches_nothing_is_reported_once() {
        let listed = tools();
        let (visible, unused) = filter(&["get_*", "nope", "re:^gone"]).visible("linear", &listed);
        assert_eq!(visible.len(), 2);
        assert_eq!(unused, vec!["nope".to_string(), "re:^gone".to_string()]);
    }
}
