//! Turn session path rules and one call's extra paths into sandbox roots.
//!
//! Stored rules stay four lists. This module emits only the shapes Seatbelt can
//! order: a literal prefix, an exact file, or the workspace wildcard. Any other
//! regex refuses the command.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    Deny,
    Unrestricted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedRule {
    /// `^.*$`, the workspace itself. The profile already allows it.
    Workspace,
    Prefix {
        relative: String,
        directory: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessClass {
    Wide,
    ProtectedFile,
    ProtectedDir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub sandboxed: bool,
    pub workspace: PathBuf,
    pub home: PathBuf,
    pub temp_dir: PathBuf,
    pub cwd: PathBuf,
    pub network: NetworkMode,
    pub env: Vec<(String, String)>,
    pub toolchain_reads: Vec<PathBuf>,
    pub wide_reads: Vec<PathBuf>,
    pub wide_writes: Vec<PathBuf>,
    pub protected_read_files: Vec<PathBuf>,
    pub protected_write_files: Vec<PathBuf>,
    pub protected_read_dirs: Vec<PathBuf>,
    pub protected_write_dirs: Vec<PathBuf>,
    pub deny_reads: Vec<PathBuf>,
    pub deny_writes: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileError(pub String);

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn parse_rule(pattern: &str) -> Result<ParsedRule, ProfileError> {
    if pattern == r"^.*$" {
        return Ok(ParsedRule::Workspace);
    }
    let rest = pattern
        .strip_prefix('^')
        .ok_or_else(|| untranslatable(pattern))?;
    let (body, directory) = if let Some(body) = rest.strip_suffix(r"(/|$)") {
        (body, true)
    } else if let Some(body) = rest.strip_suffix('$') {
        (body, false)
    } else {
        return Err(untranslatable(pattern));
    };
    let relative = unescape_literal(pattern, body)?;
    if relative.is_empty() {
        return Err(untranslatable(pattern));
    }
    Ok(ParsedRule::Prefix {
        relative,
        directory,
    })
}

fn untranslatable(pattern: &str) -> ProfileError {
    ProfileError(format!(
        "path rule cannot be enforced by the sandbox: {pattern}"
    ))
}

fn unescape_literal(pattern: &str, body: &str) -> Result<String, ProfileError> {
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let Some(next) = chars.next() else {
                return Err(untranslatable(pattern));
            };
            out.push(next);
            continue;
        }
        if ".*+?()[]{}|^$".contains(ch) {
            return Err(untranslatable(pattern));
        }
        out.push(ch);
    }
    Ok(out)
}

pub fn classify_path(path: &Path, home: &Path) -> AccessClass {
    if credential_dir(path, home) {
        return AccessClass::ProtectedDir;
    }
    if floor_file_name(path) {
        return if path.is_dir() {
            AccessClass::ProtectedDir
        } else {
            AccessClass::ProtectedFile
        };
    }
    AccessClass::Wide
}

fn credential_dir(path: &Path, home: &Path) -> bool {
    [
        home.join(".ssh"),
        home.join(".aws"),
        home.join(".kube"),
        home.join(".gnupg"),
        home.join("Library/Keychains"),
        home.join(".robi"),
    ]
    .into_iter()
    .any(|dir| path == dir)
}

fn floor_file_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    name == ".env"
        || name
            .strip_prefix(".env.")
            .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name == "id_rsa"
        || name == "id_ed25519"
        || name == "credentials.json"
        || name == "secrets.json"
}

pub fn push_classified(path: PathBuf, home: &Path, read: bool, profile: &mut Profile) {
    match classify_path(&path, home) {
        AccessClass::Wide if read => push_unique(&mut profile.wide_reads, path),
        AccessClass::Wide => push_unique(&mut profile.wide_writes, path),
        AccessClass::ProtectedFile if read => push_unique(&mut profile.protected_read_files, path),
        AccessClass::ProtectedFile => push_unique(&mut profile.protected_write_files, path),
        AccessClass::ProtectedDir if read => push_unique(&mut profile.protected_read_dirs, path),
        AccessClass::ProtectedDir => push_unique(&mut profile.protected_write_dirs, path),
    }
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

/// A session allow inside the workspace is already covered, except a floor file.
/// An allow outside the workspace becomes an extra read root.
pub fn apply_allow_read(
    pattern: &str,
    workspace: &Path,
    home: &Path,
    profile: &mut Profile,
) -> Result<(), ProfileError> {
    match parse_rule(pattern)? {
        ParsedRule::Workspace => Ok(()),
        ParsedRule::Prefix {
            relative,
            directory,
        } => {
            let path = resolve_relative(workspace, &relative);
            if directory && path.is_file() {
                return Err(untranslatable(pattern));
            }
            if inside_workspace(&path, workspace) {
                if matches!(
                    classify_path(&path, home),
                    AccessClass::ProtectedFile | AccessClass::ProtectedDir
                ) {
                    push_classified(path, home, true, profile);
                }
                return Ok(());
            }
            push_classified(path, home, true, profile);
            Ok(())
        }
    }
}

pub fn apply_deny(
    pattern: &str,
    workspace: &Path,
    read: bool,
    profile: &mut Profile,
) -> Result<(), ProfileError> {
    match parse_rule(pattern)? {
        ParsedRule::Workspace => Err(untranslatable(pattern)),
        ParsedRule::Prefix { relative, .. } => {
            let path = resolve_relative(workspace, &relative);
            if read {
                push_unique(&mut profile.deny_reads, path);
            } else {
                push_unique(&mut profile.deny_writes, path);
            }
            Ok(())
        }
    }
}

fn resolve_relative(workspace: &Path, relative: &str) -> PathBuf {
    let joined = workspace.join(relative);
    joined.canonicalize().unwrap_or(joined)
}

fn inside_workspace(path: &Path, workspace: &Path) -> bool {
    let workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    path.starts_with(&workspace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_patterns_parse_and_a_wildcard_does_not() {
        assert_eq!(
            parse_rule(r"^src/\.env$").unwrap(),
            ParsedRule::Prefix {
                relative: "src/.env".to_owned(),
                directory: false
            }
        );
        assert_eq!(
            parse_rule(r"^\.\./gopi(/|$)").unwrap(),
            ParsedRule::Prefix {
                relative: "../gopi".to_owned(),
                directory: true
            }
        );
        assert_eq!(parse_rule(r"^.*$").unwrap(), ParsedRule::Workspace);
        assert!(parse_rule(r"^src/.*$").is_err());
        assert!(parse_rule(r"(^|/)\.env$").is_err());
    }

    #[test]
    fn an_env_file_is_protected_and_a_source_file_is_not() {
        let home = Path::new("/Users/ada");
        assert_eq!(
            classify_path(Path::new("/work/src/.env"), home),
            AccessClass::ProtectedFile
        );
        assert_eq!(
            classify_path(Path::new("/work/src/main.rs"), home),
            AccessClass::Wide
        );
        assert_eq!(
            classify_path(&home.join(".ssh"), home),
            AccessClass::ProtectedDir
        );
    }
}
