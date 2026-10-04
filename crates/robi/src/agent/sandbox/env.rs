//! The environment a shell command receives. The parent environment is not copied.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct EnvInput<'a> {
    pub home: &'a Path,
    pub workspace: &'a Path,
    pub temp_dir: &'a Path,
    pub sandboxed: bool,
    pub parent_path: &'a str,
    /// Newline-separated directories appended to `PATH`. A line may start with `~/`.
    pub extra_path: &'a str,
    /// Directory of absolute-argv wrappers for [`Self::extra_path`]. Empty when unset.
    pub path_prefix: &'a str,
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
    env.push(("HOME".to_owned(), input.home.display().to_string()));
    if input.sandboxed {
        // Git would open `~/.gitconfig` and the system file. Both stay outside
        // the profile. `/dev/null` is an empty config the profile can read.
        env.push(("GIT_CONFIG_GLOBAL".to_owned(), "/dev/null".to_owned()));
        env.push(("GIT_CONFIG_SYSTEM".to_owned(), "/dev/null".to_owned()));
    }
    if let Some(rustup) = canonical_dir(&input.home.join(".rustup")) {
        env.push(("RUSTUP_HOME".to_owned(), rustup.display().to_string()));
    }
    env.retain(|(key, _)| !secret_name(key));
    env
}

fn path_value(input: &EnvInput<'_>) -> String {
    if !input.sandboxed {
        return append_extra(input.parent_path, input);
    }
    let mut dirs = Vec::new();
    if !input.path_prefix.is_empty() {
        push_dir(&mut dirs, PathBuf::from(input.path_prefix));
    }
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
    for entry in input.parent_path.split(':') {
        let Some(path) = kept_path_entry(entry, input) else {
            continue;
        };
        push_dir(&mut dirs, path);
    }
    for name in [".cargo/bin", ".local/bin", "go/bin", ".pyenv/shims"] {
        push_dir(&mut dirs, input.home.join(name));
    }
    for line in input.extra_path.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        push_dir(&mut dirs, expand_tilde(line, input.home));
    }
    dirs.iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

fn append_extra(parent: &str, input: &EnvInput<'_>) -> String {
    let mut dirs: Vec<PathBuf> = parent
        .split(':')
        .filter(|entry| !entry.is_empty())
        .map(PathBuf::from)
        .collect();
    for line in input.extra_path.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        push_dir(&mut dirs, expand_tilde(line, input.home));
    }
    dirs.iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(":")
}

/// Wrappers so a `PATH` command starts with its absolute path as `argv[0]`.
///
/// A packaged binary such as pnpm opens `argv[0]` to read its payload. The
/// shell's command name is only `pnpm`, which is not the allowed directory.
pub fn install_path_wrappers(temp_dir: &Path, home: &Path, extra_path: &str) -> Option<PathBuf> {
    let bin = temp_dir.join("path");
    let mut wrote = false;
    for line in extra_path.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let dir = expand_tilde(line, home);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if !meta.is_file() || !executable(&meta) {
                continue;
            }
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if name.contains('/') || name == "." || name == ".." {
                continue;
            }
            if std::fs::create_dir_all(&bin).is_err() {
                return wrote.then_some(bin);
            }
            let script = format!(
                "#!/bin/sh\nexec -a {quoted} {quoted} \"$@\"\n",
                quoted = shell_quote(&path)
            );
            let wrapper = bin.join(name);
            if std::fs::write(&wrapper, script).is_err() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755));
            }
            wrote = true;
        }
    }
    wrote.then_some(bin)
}

fn executable(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
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
    if input.sandboxed && inside(&canonical, input.home) && !toolchain_home(&canonical, input.home)
    {
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

fn canonical_dir(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        Some(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
    } else {
        None
    }
}

fn toolchain_home(path: &Path, home: &Path) -> bool {
    super::TOOLCHAIN_ROOTS
        .iter()
        .any(|name| inside(path, &home.join(name)))
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
            extra_path: "",
            path_prefix: "",
            lang: Some("en_US.UTF-8"),
            user: Some("ada"),
        }
    }

    fn env_value<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        env.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
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
        let home_value = home.display().to_string();
        assert_eq!(
            env.iter()
                .find(|(key, _)| key == "HOME")
                .map(|(_, value)| value.as_str()),
            Some(home_value.as_str())
        );
        assert_eq!(
            env.iter()
                .find(|(key, _)| key == "LANG")
                .map(|(_, v)| v.as_str()),
            Some("en_US.UTF-8")
        );
        assert_eq!(env_value(&env, "GIT_CONFIG_GLOBAL"), Some("/dev/null"));
        assert_eq!(env_value(&env, "GIT_CONFIG_SYSTEM"), Some("/dev/null"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_extra_home_directory_is_appended_to_path() {
        let root = std::env::temp_dir().join(format!("robi-env-extra-{}", std::process::id()));
        let home = root.join("home");
        let bin = home.join("pnpm");
        let workspace = root.join("work");
        let temp = root.join("tmp");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&temp).unwrap();
        let mut built = input(&home, &workspace, &temp, true, "/usr/bin");
        built.extra_path = "~/pnpm";
        let env = command_env(&built);
        let path = &env.iter().find(|(key, _)| key == "PATH").unwrap().1;
        let canonical = bin.canonicalize().unwrap();
        assert!(path.split(':').any(|entry| Path::new(entry) == canonical));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_path_entry_is_started_by_its_absolute_name() {
        let root = std::env::temp_dir().join(format!("robi-env-wrap-{}", std::process::id()));
        let home = root.join("home");
        let bin = home.join("pnpm");
        let temp = root.join("tmp");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&temp).unwrap();
        let tool = bin.join("pnpm");
        std::fs::write(&tool, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let wrapped = install_path_wrappers(&temp, &home, "~/pnpm").unwrap();
        let script = std::fs::read_to_string(wrapped.join("pnpm")).unwrap();
        let absolute = tool.display().to_string();
        assert!(script.contains(&format!("exec -a '{absolute}' '{absolute}'")));
        let workspace = root.join("work");
        let mut built = input(&home, &workspace, &temp, true, "/usr/bin");
        built.extra_path = "~/pnpm";
        built.path_prefix = wrapped.to_str().unwrap();
        let env = command_env(&built);
        let path = &env.iter().find(|(key, _)| key == "PATH").unwrap().1;
        let prefix = wrapped.canonicalize().unwrap();
        assert!(path.starts_with(&prefix.display().to_string()));
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
        let parent = format!(".:{}:{}", bin.display(), workspace.display());
        let env = command_env(&input(&home, &workspace, &temp, false, &parent));
        let path = &env.iter().find(|(key, _)| key == "PATH").unwrap().1;
        assert_eq!(path, &parent);
        let home_value = env.iter().find(|(key, _)| key == "HOME").unwrap().1.clone();
        assert_eq!(home_value, home.display().to_string());
        assert_eq!(env_value(&env, "GIT_CONFIG_GLOBAL"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rustup_home_is_the_canonical_directory() {
        let root = std::env::temp_dir().join(format!("robi-rustup-{}", std::process::id()));
        let home = root.join("home");
        let real = root.join("real-rustup");
        let workspace = root.join("work");
        let temp = root.join("tmp");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&temp).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, home.join(".rustup")).unwrap();
        #[cfg(not(unix))]
        std::fs::create_dir_all(home.join(".rustup")).unwrap();
        let env = command_env(&input(&home, &workspace, &temp, true, "/usr/bin"));
        let rustup = env
            .iter()
            .find(|(key, _)| key == "RUSTUP_HOME")
            .map(|(_, value)| value.clone())
            .unwrap();
        assert_eq!(rustup, real.canonicalize().unwrap().display().to_string());
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
