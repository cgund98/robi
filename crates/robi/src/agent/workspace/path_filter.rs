//! Compiled allow and deny regexes for one session.

use regex::Regex;
use robi_core::ids::SessionId;

use crate::domain::chat_session::model::PathRules;

/// One compiled pattern and how many literal characters it pins down.
#[derive(Debug, Clone)]
struct Rule {
    regex: Regex,
    weight: usize,
}

/// A session's path rules, compiled once per tool call.
#[derive(Debug, Clone)]
pub struct PathFilter {
    allow_read: Vec<Rule>,
    allow_write: Vec<Rule>,
    deny_read: Vec<Rule>,
    deny_write: Vec<Rule>,
}

impl PathFilter {
    /// Compiled rules plus the read allow for this session's plan directory.
    pub fn for_session(rules: &PathRules, session_id: SessionId) -> Result<Self, String> {
        let mut rules = rules.clone();
        rules.allow_read.push(session_plan_read_allow(session_id));
        Self::compile(&rules)
    }

    pub fn compile(rules: &PathRules) -> Result<Self, String> {
        let rules = rules.with_system_defaults();
        Ok(Self {
            allow_read: compile_all(&rules.allow_read)?,
            allow_write: compile_all(&rules.allow_write)?,
            deny_read: compile_all(&rules.deny_read)?,
            deny_write: compile_all(&rules.deny_write)?,
        })
    }

    /// The most specific matching read rule wins. A deny wins a tie.
    ///
    /// A write allow that beats a write deny also permits the read.
    pub fn allows_read(&self, relative_path: &str) -> bool {
        allow_wins(&self.allow_write, &self.deny_write, relative_path)
            || !denied(&self.allow_read, &self.deny_read, relative_path)
    }

    /// The most specific matching write rule wins. A deny wins a tie.
    pub fn allows_write(&self, relative_path: &str) -> bool {
        !denied(&self.allow_write, &self.deny_write, relative_path)
    }

    /// Walks skip a directory the session is not allowed to read.
    pub fn skip_dir(&self, relative_path: &str) -> bool {
        !relative_path.is_empty() && !self.allows_read(relative_path)
    }
}

/// Read allow for `~/.robi/plans/<session_id>`, matched on the workspace-relative path.
///
/// The directory may sit after `..` and other path components when the
/// workspace is not under the home directory.
fn session_plan_read_allow(session_id: SessionId) -> String {
    format!(
        r"(^|/)\.robi/plans/{}(/|$)",
        regex_literal(&session_id.to_string())
    )
}

fn regex_literal(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if "\\.+*?()[]{}|^$".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn compile_all(patterns: &[String]) -> Result<Vec<Rule>, String> {
    patterns
        .iter()
        .map(|pattern| {
            Ok(Rule {
                regex: compile_one(pattern)?,
                weight: literal_weight(pattern),
            })
        })
        .collect()
}

fn compile_one(pattern: &str) -> Result<Regex, String> {
    Regex::new(pattern).map_err(|err| format!("invalid path pattern `{pattern}`: {err}"))
}

/// The furthest match wins. At the same end byte, more literals win. A deny wins a tie.
///
/// Allowing `gopi` stops at that directory, so a `.git` deny further down the
/// path outranks it. Allowing `gopi/.git` reaches the same point as the deny
/// and names more of the path, so that tree is readable.
fn denied(allow: &[Rule], deny: &[Rule], path: &str) -> bool {
    let Some(deny_rank) = best_rank(deny, path) else {
        return false;
    };
    match best_rank(allow, path) {
        Some(allow_rank) => allow_rank <= deny_rank,
        None => true,
    }
}

/// An allow beats a deny on this path. No matching deny means this is false.
fn allow_wins(allow: &[Rule], deny: &[Rule], path: &str) -> bool {
    let Some(deny_rank) = best_rank(deny, path) else {
        return false;
    };
    matches!(best_rank(allow, path), Some(allow_rank) if allow_rank > deny_rank)
}

/// `(end byte, literal weight)`. Higher is more specific.
fn best_rank(rules: &[Rule], path: &str) -> Option<(usize, usize)> {
    rules
        .iter()
        .filter_map(|rule| {
            let end = rule.regex.find_iter(path).map(|found| found.end()).max()?;
            Some((end, rule.weight))
        })
        .max()
}

/// Literal characters in a pattern. Wildcards, anchors, and groups do not count.
fn literal_weight(pattern: &str) -> usize {
    let mut weight = 0;
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if chars.next().is_some() {
                    weight += 1;
                }
            }
            '[' => {
                while let Some(inner) = chars.next() {
                    if inner == '\\' {
                        chars.next();
                        continue;
                    }
                    if inner == ']' {
                        break;
                    }
                }
            }
            '.' | '*' | '+' | '?' | '(' | ')' | '{' | '}' | '|' | '^' | '$' => {}
            _ => weight += 1,
        }
    }
    weight
}

/// `--glob` exclusions for deny-read patterns of the form `(^|/)name(/|$)`.
///
/// A glob is omitted when an allow-read pattern matches a path inside that
/// directory, so an exception can still be searched and filtered afterward.
pub fn directory_exclusion_globs(rules: &PathRules) -> Vec<String> {
    let rules = rules.with_system_defaults();
    let mut globs = Vec::new();
    for pattern in &rules.deny_read {
        let Some(name) = anchored_directory_name(pattern) else {
            continue;
        };
        if allow_might_match_directory(&rules.allow_read, &name) {
            continue;
        }
        globs.push(format!("!**/{name}/**"));
    }
    globs
}

fn anchored_directory_name(pattern: &str) -> Option<String> {
    let rest = pattern.strip_prefix("(^|/)")?;
    let raw = rest.strip_suffix("(/|$)")?;
    let name = unescape_literal(raw)?;
    if name.is_empty() || name.contains('/') {
        return None;
    }
    Some(name)
}

fn unescape_literal(raw: &str) -> Option<String> {
    let mut name = String::new();
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            name.push(chars.next()?);
        } else if ".*+?[](){}|^$".contains(ch) {
            return None;
        } else {
            name.push(ch);
        }
    }
    Some(name)
}

fn allow_might_match_directory(allow: &[String], name: &str) -> bool {
    let probes = [
        name.to_owned(),
        format!("{name}/HEAD"),
        format!("nested/{name}"),
        format!("nested/{name}/config"),
    ];
    allow.iter().any(|pattern| {
        let Ok(regex) = Regex::new(pattern) else {
            return true;
        };
        probes.iter().any(|probe| regex.is_match(probe))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(rules: PathRules) -> PathFilter {
        PathFilter::compile(&rules).expect("patterns compile")
    }

    #[test]
    fn default_denies_secrets_and_git_and_allows_source() {
        let filter = filter(PathRules::default());
        assert!(!filter.allows_read(".env"));
        assert!(!filter.allows_read("app/.env.local"));
        assert!(!filter.allows_read(".git"));
        assert!(!filter.allows_read(".git/config"));
        assert!(!filter.allows_read("keys/id_rsa"));
        assert!(!filter.allows_read("server.pem"));
        assert!(!filter.allows_write("secrets.json"));
        assert!(filter.allows_read("src/main.rs"));
        assert!(filter.allows_read(""));
        assert!(!filter.allows_read("../gopi"));
        assert!(!filter.allows_read("../gopi/src/main.rs"));
        assert!(filter.skip_dir(".git"));
        assert!(filter.skip_dir("../gopi"));
        assert!(!filter.skip_dir("src"));
    }

    #[test]
    fn a_session_may_read_its_own_plans_and_not_another_sessions() {
        let session = SessionId::new();
        let other = SessionId::new();
        let filter = PathFilter::for_session(&PathRules::default(), session).unwrap();
        let own = format!("../../../.robi/plans/{session}/ship.md");
        let inside = format!(".robi/plans/{session}/ship.md");
        let elsewhere = format!("../../Users/ada/.robi/plans/{session}/ship.md");
        let foreign = format!("../../../.robi/plans/{other}/ship.md");
        assert!(filter.allows_read(&own));
        assert!(filter.allows_read(&inside));
        assert!(filter.allows_read(&elsewhere));
        assert!(filter.allows_read(&format!("../../../.robi/plans/{session}")));
        assert!(!filter.allows_write(&own));
        assert!(!filter.allows_read(&foreign));
        assert!(!filter.allows_read("../../../.robi/secrets.toml"));
        assert!(!filter.allows_read(&format!("../../../.robi/plans/{session}-extra/ship.md")));
    }

    #[test]
    fn a_grant_of_a_parent_outside_the_workspace_opens_that_tree() {
        let filter = filter(PathRules {
            allow_read: vec![r"^\.\./gopi(/|$)".to_owned()],
            ..PathRules::default()
        });
        assert!(filter.allows_read("../gopi"));
        assert!(filter.allows_read("../gopi/src/main.rs"));
        assert!(!filter.allows_read("../other"));
        assert!(!filter.allows_read("../gopi/.git"));
        assert!(!filter.allows_read("../gopi/.env"));
    }

    #[test]
    fn a_stored_deny_is_appended_after_the_builtin_patterns() {
        let rules = PathRules {
            deny_read: vec![r"^build(/|$)".to_owned()],
            ..PathRules::default()
        };
        assert!(rules
            .deny_read
            .iter()
            .all(|pattern| pattern != r"(^|/)\.git(/|$)"));
        let filter = filter(rules);
        assert!(!filter.allows_read(".git/config"));
        assert!(!filter.allows_read("build/out"));
        assert!(filter.allows_read("src/main.rs"));
    }

    #[test]
    fn allow_read_overrides_only_reads() {
        let rules = PathRules {
            allow_read: vec![r"^src/\.env$".to_owned()],
            ..PathRules::default()
        };
        let filter = filter(rules);
        assert!(filter.allows_read("src/.env"));
        assert!(!filter.allows_write("src/.env"));
        assert!(!filter.allows_read(".env"));
    }

    #[test]
    fn a_winning_write_allow_also_allows_the_read() {
        let filter = filter(PathRules {
            allow_write: vec![r"^src/\.env$".to_owned()],
            ..PathRules::default()
        });
        assert!(filter.allows_write("src/.env"));
        assert!(filter.allows_read("src/.env"));
        assert!(!filter.allows_read(".env"));
    }

    #[test]
    fn a_parent_allow_does_not_open_a_denied_child() {
        let rules = PathRules {
            allow_read: vec![r"^src(/|$)".to_owned()],
            allow_write: vec![r"^src(/|$)".to_owned()],
            ..PathRules::default()
        };
        let filter = filter(rules);
        assert!(filter.allows_read("src"));
        assert!(filter.allows_read("src/main.rs"));
        assert!(!filter.allows_read("src/.env"));
        assert!(!filter.allows_read("src/.git/config"));
        assert!(!filter.allows_write("src/.env"));
    }

    #[test]
    fn a_more_specific_pattern_outranks_a_broader_one() {
        let parent = filter(PathRules {
            allow_read: vec![r"^gopi(/|$)".to_owned()],
            ..PathRules::default()
        });
        assert!(parent.allows_read("gopi/src/main.rs"));
        assert!(!parent.allows_read("gopi/.git"));
        assert!(!parent.allows_read("gopi/.git/config"));

        let opened = filter(PathRules {
            allow_read: vec![r"^gopi/\.git(/|$)".to_owned()],
            ..PathRules::default()
        });
        assert!(opened.allows_read("gopi/.git"));
        assert!(opened.allows_read("gopi/.git/config"));
        assert!(!opened.allows_read("other/.git/config"));
    }

    #[test]
    fn a_wildcard_allow_does_not_outrank_a_literal_deny() {
        let filter = filter(PathRules {
            allow_read: vec![r"^src/.*$".to_owned()],
            ..PathRules::default()
        });
        assert!(filter.allows_read("src/main.rs"));
        assert!(!filter.allows_read("src/.env"));
    }

    #[test]
    fn directory_glob_skips_git_unless_an_allow_matches_inside() {
        let rules = PathRules::default();
        assert_eq!(
            directory_exclusion_globs(&rules),
            vec!["!**/.git/**".to_owned()]
        );

        let opened = PathRules {
            allow_read: vec![r"^\.git/HEAD$".to_owned()],
            ..PathRules::default()
        };
        assert!(directory_exclusion_globs(&opened).is_empty());

        let other = PathRules {
            allow_read: vec![r"^notes\.txt$".to_owned()],
            ..PathRules::default()
        };
        assert_eq!(
            directory_exclusion_globs(&other),
            vec!["!**/.git/**".to_owned()]
        );
    }
}
