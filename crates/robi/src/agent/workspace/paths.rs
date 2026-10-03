//! Turn a tool path argument into a canonical absolute path.
//!
//! A path outside the workspace is still resolved. Its workspace-relative form
//! starts with `..`, and the path filter denies that form until a grant allows it.

use std::path::{Component, Path, PathBuf};

/// A path a tool may read, after `~`, relative joins, and symlink resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPath {
    pub absolute: PathBuf,
    /// Workspace-relative, with `/` separators and no leading slash.
    /// Empty when `absolute` is the workspace root. A path outside the
    /// workspace starts with `..`.
    pub relative: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    WorkspaceRoot(String),
    HomeUnavailable,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::WorkspaceRoot(message) => {
                write!(f, "could not resolve the workspace root: {message}")
            }
            ResolveError::HomeUnavailable => write!(f, "home directory is not set"),
        }
    }
}

pub fn user_home() -> Result<PathBuf, ResolveError> {
    let home = std::env::var_os("HOME").ok_or(ResolveError::HomeUnavailable)?;
    if home.is_empty() {
        return Err(ResolveError::HomeUnavailable);
    }
    Ok(PathBuf::from(home))
}

/// Resolve `argument` against `workspace_root`.
///
/// A leading `~` or `~/` expands to `home`. Any other relative form joins the
/// workspace root. An absolute path is used as given. The result is
/// canonicalized. A path outside the root is kept, and [`workspace_relative`]
/// records it with `..`.
pub fn resolve_path(
    workspace_root: &Path,
    argument: &str,
    home: &Path,
) -> Result<ResolvedPath, ResolveError> {
    let root = workspace_root
        .canonicalize()
        .map_err(|err| ResolveError::WorkspaceRoot(err.to_string()))?;
    let expanded = expand_tilde(argument, home);
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        root.join(expanded)
    };
    let absolute = canonicalize_lexical(&joined);
    Ok(ResolvedPath {
        relative: workspace_relative(&root, &absolute),
        absolute,
    })
}

/// Workspace-relative form of `path`, with `/` separators.
///
/// A path under `root` has no leading slash. A path outside it starts with `..`.
pub fn workspace_relative(root: &Path, path: &Path) -> String {
    if let Some(relative) = inside(root, path) {
        return relative;
    }
    let root = canonicalize_lexical(root);
    let path = canonicalize_lexical(path);
    if let Some(relative) = inside(&root, &path) {
        return relative;
    }
    relative_with_parents(&root, &path)
}

fn expand_tilde(argument: &str, home: &Path) -> PathBuf {
    if argument == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = argument.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(argument)
}

/// Resolve `.`, `..`, and symlinks. A missing tail stays lexical.
fn canonicalize_lexical(path: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => resolved.push(prefix.as_os_str()),
            Component::RootDir => resolved.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(part) => {
                resolved.push(part);
                if resolved.exists() {
                    if let Ok(canonical) = resolved.canonicalize() {
                        resolved = canonical;
                    }
                }
            }
        }
    }
    resolved
}

fn inside(root: &Path, path: &Path) -> Option<String> {
    if path == root {
        return Some(String::new());
    }
    let relative = path.strip_prefix(root).ok()?;
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn relative_with_parents(root: &Path, absolute: &Path) -> String {
    let root_parts: Vec<_> = root.components().collect();
    let absolute_parts: Vec<_> = absolute.components().collect();
    let mut common = 0;
    while common < root_parts.len()
        && common < absolute_parts.len()
        && root_parts[common] == absolute_parts[common]
    {
        common += 1;
    }
    let mut parts = Vec::new();
    for _ in 0..(root_parts.len() - common) {
        parts.push("..".to_owned());
    }
    for component in &absolute_parts[common..] {
        if let Component::Normal(part) = component {
            parts.push(part.to_string_lossy().into_owned());
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn workspace() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("robi-paths-{}-{}", std::process::id(), unique()));
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("nested").join("file.txt"), "hi").unwrap();
        root.canonicalize().unwrap()
    }

    fn unique() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    #[test]
    fn tilde_dot_and_parent_resolve_to_the_same_file() {
        let root = workspace();
        let name = root.file_name().unwrap().to_string_lossy();
        let home = root.clone();
        let expected = root.join("nested").join("file.txt");

        let from_home = resolve_path(&root, "~/nested/file.txt", &home).unwrap();
        let from_dot = resolve_path(&root, "./nested/file.txt", &home).unwrap();
        let from_parent =
            resolve_path(&root, &format!("../{name}/nested/file.txt"), &home).unwrap();

        assert_eq!(from_home.absolute, expected);
        assert_eq!(from_dot.absolute, expected);
        assert_eq!(from_parent.absolute, expected);
        assert_eq!(from_home.relative, "nested/file.txt");
    }

    #[test]
    fn a_parent_path_is_relative_with_dotdot() {
        let root = workspace();
        let sibling = root
            .parent()
            .unwrap()
            .join(format!("robi-sibling-{}", unique()));
        fs::create_dir_all(sibling.join("src")).unwrap();
        let name = sibling.file_name().unwrap().to_string_lossy();
        let resolved = resolve_path(&root, &format!("../{name}"), &root).unwrap();
        assert_eq!(resolved.relative, format!("../{name}"));
        assert_eq!(resolved.absolute, sibling.canonicalize().unwrap());
        let _ = fs::remove_dir_all(&sibling);
    }

    #[test]
    fn a_symlink_that_leaves_the_workspace_is_relative_with_dotdot() {
        let root = workspace();
        let outside = std::env::temp_dir().join(format!("robi-outside-{}", unique()));
        fs::write(&outside, "nope").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("leak")).unwrap();
        let resolved = resolve_path(&root, "leak", &root).unwrap();
        assert!(resolved.relative.starts_with(".."), "{}", resolved.relative);
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn tilde_user_stays_a_relative_path() {
        let root = workspace();
        let resolved = resolve_path(&root, "~otheruser/secret", &root).unwrap();
        assert_eq!(resolved.relative, "~otheruser/secret");
    }
}
