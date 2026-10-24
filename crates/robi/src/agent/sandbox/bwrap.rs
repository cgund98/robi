//! Linux confinement. Mount order is last-match-wins, matching the Seatbelt profile.

use std::path::Path;

use super::profile::{NetworkMode, Profile, WriteAccess};

pub fn command(profile: &Profile, shell: &[String]) -> Result<Vec<String>, String> {
    let read_only = profile.write == WriteAccess::ReadOnly;
    let mut args = vec![
        "bwrap".to_owned(),
        "--die-with-parent".to_owned(),
        "--unshare-pid".to_owned(),
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--dev".to_owned(),
        "/dev".to_owned(),
    ];
    if profile.network == NetworkMode::Deny {
        args.push("--unshare-net".to_owned());
    }
    for path in [
        "/usr", "/bin", "/sbin", "/lib", "/lib64", "/opt", "/etc", "/nix",
    ] {
        if Path::new(path).exists() {
            args.extend(["--ro-bind".to_owned(), path.to_owned(), path.to_owned()]);
        }
    }
    args.extend(["--tmpfs".to_owned(), "/tmp".to_owned()]);
    if read_only {
        args.extend([
            "--ro-bind".to_owned(),
            profile.workspace.display().to_string(),
            profile.workspace.display().to_string(),
        ]);
    } else {
        args.extend([
            "--bind".to_owned(),
            profile.workspace.display().to_string(),
            profile.workspace.display().to_string(),
        ]);
    }
    // The private temp directory stays writable in both modes: it belongs to
    // this call and is deleted when it returns.
    args.extend([
        "--bind".to_owned(),
        profile.temp_dir.display().to_string(),
        profile.temp_dir.display().to_string(),
    ]);
    // S9: the one directory Plan mode may write, bound read-write in both modes.
    // Emitted before the deny, git-lock, and floor mounts so those still win.
    for path in &profile.extra_writes {
        bind(&mut args, path);
    }
    for path in profile
        .toolchain_reads
        .iter()
        .chain(&profile.wide_reads)
        .chain(&profile.protected_read_dirs)
        .chain(&profile.protected_read_files)
    {
        ro_bind(&mut args, path);
    }
    for path in profile
        .wide_writes
        .iter()
        .chain(&profile.protected_write_dirs)
        .chain(&profile.protected_write_files)
    {
        if read_only {
            ro_bind(&mut args, path);
        } else {
            bind(&mut args, path);
        }
    }
    for path in profile.deny_reads.iter().chain(&profile.deny_writes) {
        hide_or_readonly(&mut args, path);
    }
    for path in git_write_locks(&profile.workspace) {
        readonly(&mut args, &path);
    }
    let git = profile.workspace.join(".git");
    if !read_only {
        for path in profile
            .wide_writes
            .iter()
            .chain(&profile.protected_write_dirs)
            .chain(&profile.protected_write_files)
        {
            if path == &git || path.starts_with(&git) {
                bind(&mut args, path);
            }
        }
    }
    for path in floor_files(profile)? {
        hide(&mut args, &path);
    }
    args.extend(["--chdir".to_owned(), profile.cwd.display().to_string()]);
    for (key, value) in &profile.env {
        args.push("--setenv".to_owned());
        args.push(key.clone());
        args.push(value.clone());
    }
    args.push("--".to_owned());
    args.extend(shell.iter().cloned());
    Ok(args)
}

fn ro_bind(args: &mut Vec<String>, path: &Path) {
    if path.exists() {
        args.extend([
            "--ro-bind".to_owned(),
            path.display().to_string(),
            path.display().to_string(),
        ]);
    }
}

fn bind(args: &mut Vec<String>, path: &Path) {
    if path.exists() {
        args.extend([
            "--bind".to_owned(),
            path.display().to_string(),
            path.display().to_string(),
        ]);
    }
}

fn readonly(args: &mut Vec<String>, path: &Path) {
    if path.exists() {
        ro_bind(args, path);
    }
}

fn hide(args: &mut Vec<String>, path: &Path) {
    if path.is_file() {
        args.extend([
            "--ro-bind".to_owned(),
            "/dev/null".to_owned(),
            path.display().to_string(),
        ]);
    } else if path.is_dir() {
        args.extend(["--tmpfs".to_owned(), path.display().to_string()]);
    }
}

fn hide_or_readonly(args: &mut Vec<String>, path: &Path) {
    hide(args, path);
}

fn git_write_locks(workspace: &Path) -> Vec<std::path::PathBuf> {
    vec![workspace.join(".git")]
}

fn floor_files(profile: &Profile) -> Result<Vec<std::path::PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![profile.workspace.clone()];
    let mut visited = 0usize;
    while let Some(dir) = pending.pop() {
        visited += 1;
        if visited > 20_000 {
            return Err(
                "sandbox could not be applied: the workspace is too large to hide secret files"
                    .to_owned(),
            );
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if path.is_dir() {
                if !matches!(name, "target" | "node_modules" | "dist" | "vendor") {
                    pending.push(path);
                }
                continue;
            }
            if profile
                .protected_read_files
                .iter()
                .any(|open| open == &path)
                || profile
                    .protected_write_files
                    .iter()
                    .any(|open| open == &path)
            {
                continue;
            }
            if crate::agent::sandbox::profile::classify_path(&path, Path::new("/"))
                == crate::agent::sandbox::profile::AccessClass::ProtectedFile
            {
                found.push(path);
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn profile(write: WriteAccess) -> Profile {
        Profile {
            sandboxed: true,
            write,
            workspace: PathBuf::from("/work/app"),
            home: PathBuf::from("/home/ada"),
            temp_dir: PathBuf::from("/tmp/robi-call"),
            cwd: PathBuf::from("/work/app"),
            network: NetworkMode::Deny,
            env: Vec::new(),
            toolchain_reads: Vec::new(),
            wide_reads: Vec::new(),
            wide_writes: Vec::new(),
            extra_writes: Vec::new(),
            protected_read_files: Vec::new(),
            protected_write_files: Vec::new(),
            protected_read_dirs: Vec::new(),
            protected_write_dirs: Vec::new(),
            deny_reads: Vec::new(),
            deny_writes: Vec::new(),
        }
    }

    fn shell() -> Vec<String> {
        vec!["/bin/sh".to_owned(), "-c".to_owned(), "true".to_owned()]
    }

    #[test]
    fn a_read_only_profile_ro_binds_the_workspace() {
        let args = command(&profile(WriteAccess::ReadOnly), &shell()).unwrap();
        let pairs: Vec<(&str, &str)> = args
            .windows(3)
            .filter(|w| w[0] == "--ro-bind" || w[0] == "--bind")
            .map(|w| (w[0].as_str(), w[1].as_str()))
            .collect();
        assert!(pairs.contains(&("--ro-bind", "/work/app")), "{pairs:?}");
        assert!(!pairs.contains(&("--bind", "/work/app")), "{pairs:?}");
        // The private temp directory stays writable: it belongs to the call.
        assert!(pairs.contains(&("--bind", "/tmp/robi-call")), "{pairs:?}");
    }

    #[test]
    fn a_writable_profile_binds_the_workspace_read_write() {
        let args = command(&profile(WriteAccess::ReadWrite), &shell()).unwrap();
        let pairs: Vec<(&str, &str)> = args
            .windows(3)
            .filter(|w| w[0] == "--ro-bind" || w[0] == "--bind")
            .map(|w| (w[0].as_str(), w[1].as_str()))
            .collect();
        assert!(pairs.contains(&("--bind", "/work/app")), "{pairs:?}");
    }

    /// S9: Plan's one writable directory, bound read-write in both modes and
    /// after the workspace mount, so the later deny and `.git` mounts still win.
    ///
    /// Those later mounts are emitted only for paths that exist, so this test
    /// asserts the bind's position and mode rather than a deny on a temp path.
    /// [`super::super::seatbelt`]'s ordering test covers the rule itself.
    #[test]
    fn extra_writes_are_bound_read_write_after_the_workspace() {
        // The mount is emitted only for a path that exists, so use real temp dirs.
        let root = std::env::temp_dir().join(format!(
            "robi-bwrap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let plans = root.join(".robi/plans");
        std::fs::create_dir_all(&plans).unwrap();
        for write in [WriteAccess::ReadOnly, WriteAccess::ReadWrite] {
            let mut profile = profile(write);
            profile.workspace = root.clone();
            profile.cwd = root.clone();
            profile.extra_writes = vec![plans.clone()];
            let args = command(&profile, &shell()).unwrap();
            let workspace = args
                .windows(3)
                .position(|w| {
                    w[0].starts_with("--")
                        && w[0].ends_with("bind")
                        && w[1] == root.display().to_string()
                })
                .expect("the workspace is mounted");
            let extra = args
                .windows(3)
                .position(|w| w[0] == "--bind" && w[1] == plans.display().to_string())
                .expect("the plans directory is bound read-write");
            let chdir = args
                .iter()
                .position(|arg| arg == "--chdir")
                .expect("a working directory");
            assert!(workspace < extra, "{write:?}");
            assert!(extra < chdir, "{write:?}");
            assert!(!args
                .windows(3)
                .any(|w| w[0] == "--ro-bind" && w[1] == plans.display().to_string()));
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
