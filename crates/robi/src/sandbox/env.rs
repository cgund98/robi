//! The environment a shell command receives. The parent environment is not copied.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct EnvInput<'a> {
    pub home: &'a Path,
    pub workspace: &'a Path,
    pub temp_dir: &'a Path,
    pub sandboxed: bool,
    pub parent_path: &'a str,
    pub lang: Option<&'a str>,
    pub user: Option<&'a str>,
}

/// `KEY=VALUE` pairs for the child. Secret names and loader variables are absent.
pub fn command_env(input: &EnvInput<'_>) -> Vec<(String, String)> {
    let mut env = Vec::new();
    env.push(("PATH".to_owned(), path_value(input)));
    let temp = input.temp_dir.display().to_string();
    env.push(("TMPDIR".to_owned(), temp.clone()));
    env.push(("TMP".to_owned(), temp.clone()));
    env.push(("TEMP".to_owned(), temp));
    if let Some(user) = input.user.filter(|user| !user.is_empty()) {
        env.push(("USER".to_owned(), user.to_owned()));
        env.push(("LOGNAME".to_owned(), user.to_owned()));
    }
    env.push(("LANG".to_owned(), locale(input.lang)));
    env.push(("LC_ALL".to_owned(), locale(input.lang)));
    if !input.sandboxed {
        env.push(("HOME".to_owned(), input.home.display().to_string()));
    }
    if input.home.join(".rustup").is_dir() {
        env.push((
            "RUSTUP_HOME".to_owned(),
            input.home.join(".rustup").display().to_string(),
        ));
    }
    env.retain(|(key, _)| !secret_name(key));
    env
}

fn path_value(input: &EnvInput<'_>) -> String {
    let mut dirs = Vec::new();
    if input.sandboxed {
        for prefix in [
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
            "/usr/local/bin",
            "/opt/homebrew/bin",
        ] {
            push_dir(&mut dirs, PathBuf::from(prefix));
        }
    }
    for entry in input.parent_path.split(':') {
        let Some(path) = kept_path_entry(entry, input) else {
            continue;
        };
        push_dir(&mut dirs, path);
    }
    if input.sandboxed {
        for name in [".cargo/bin", ".local/bin"] {
            push_dir(&mut dirs, input.home.join(name));
        }
    }
    dirs.iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

fn kept_path_entry(entry: &str, input: &EnvInput<'_>) -> Option<PathBuf> {
    if entry.is_empty() || entry == "." {
        return None;
    }
    let path = PathBuf::from(entry);
    if !path.is_absolute() {
        return None;
    }
    let canonical = canonicalize_existing(&path)?;
    if inside(&canonical, input.workspace) {
        return None;
    }
    if input.sandboxed && inside(&canonical, input.home) {
        return None;
    }
    Some(canonical)
}

fn push_dir(dirs: &mut Vec<PathBuf>, path: PathBuf) {
    if !path.is_dir() {
        return;
    }
    let canonical = canonicalize_existing(&path).unwrap_or(path);
    if dirs.iter().any(|existing| existing == &canonical) {
        return;
    }
    dirs.push(canonical);
}

fn canonicalize_existing(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok()
}

fn inside(path: &Path, root: &Path) -> bool {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    path.starts_with(&root)
}

fn locale(lang: Option<&str>) -> String {
    match lang {
        Some(value) if is_locale_token(value) => value.to_owned(),
        _ => "C.UTF-8".to_owned(),
    }
}

fn is_locale_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '@' | '-'))
}

pub fn secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("DYLD_")
        || upper == "LD_PRELOAD"
        || upper == "LD_LIBRARY_PATH"
        || ["KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL"]
            .iter()
            .any(|part| upper.contains(part))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(
        home: &'a Path,
        workspace: &'a Path,
        temp: &'a Path,
        sandboxed: bool,
        parent_path: &'a str,
    ) -> EnvInput<'a> {
        EnvInput {
            home,
            workspace,
            temp_dir: temp,
            sandboxed,
            parent_path,
            lang: Some("en_US.UTF-8"),
            user: Some("ada"),
        }
    }

    #[test]
    fn a_sandboxed_path_drops_the_workspace_and_dot_entries() {
        let root = std::env::temp_dir().join(format!("robi-env-{}", std::process::id()));
        let home = root.join("home");
        let workspace = root.join("work");
        let temp = root.join("tmp");
        std::fs::create_dir_all(workspace.join("node_modules/.bin")).unwrap();
        std::fs::create_dir_all(&temp).unwrap();
        let parent = format!(
            ".:{}:/usr/bin",
            workspace.join("node_modules/.bin").display()
        );
        let env = command_env(&input(&home, &workspace, &temp, true, &parent));
        let path = env.iter().find(|(key, _)| key == "PATH").unwrap().1.clone();
        assert!(!path.split(':').any(|entry| entry == "."));
        assert!(!path.split(':').any(|entry| entry.contains("node_modules")));
        assert!(path
            .split(':')
            .any(|entry| entry == "/usr/bin" || entry.ends_with("/usr/bin")));
        assert!(env.iter().all(|(key, _)| !secret_name(key)));
        assert!(!env.iter().any(|(key, _)| key == "HOME"));
        assert_eq!(
            env.iter()
                .find(|(key, _)| key == "LANG")
                .map(|(_, v)| v.as_str()),
            Some("en_US.UTF-8")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unsandboxed_path_keeps_a_home_entry_and_sets_home() {
        let root = std::env::temp_dir().join(format!("robi-env-home-{}", std::process::id()));
        let home = root.join("home");
        let bin = home.join("bin");
        let workspace = root.join("work");
        let temp = root.join("tmp");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&temp).unwrap();
        let parent = bin.display().to_string();
        let env = command_env(&input(&home, &workspace, &temp, false, &parent));
        let path = &env.iter().find(|(key, _)| key == "PATH").unwrap().1;
        assert!(path.contains(bin.canonicalize().unwrap().to_str().unwrap()));
        let home_value = env.iter().find(|(key, _)| key == "HOME").unwrap().1.clone();
        assert_eq!(home_value, home.display().to_string());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_secret_name_is_rejected() {
        assert!(secret_name("OPENAI_API_KEY"));
        assert!(secret_name("DYLD_INSERT_LIBRARIES"));
        assert!(secret_name("LD_PRELOAD"));
        assert!(!secret_name("PATH"));
        assert!(!secret_name("USER"));
    }

    #[test]
    fn a_broken_locale_falls_back() {
        assert_eq!(locale(Some("en_US.UTF-8")), "en_US.UTF-8");
        assert_eq!(locale(Some("bad locale")), "C.UTF-8");
        assert_eq!(locale(None), "C.UTF-8");
    }
}
