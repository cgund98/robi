//! Linux confinement. Mount order is last-match-wins, matching the Seatbelt profile.

use std::path::Path;

use super::profile::{NetworkMode, Profile};

pub fn command(profile: &Profile, shell: &[String]) -> Result<Vec<String>, String> {
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
    args.extend([
        "--tmpfs".to_owned(),
        "/tmp".to_owned(),
        "--bind".to_owned(),
        profile.workspace.display().to_string(),
        profile.workspace.display().to_string(),
        "--bind".to_owned(),
        profile.temp_dir.display().to_string(),
        profile.temp_dir.display().to_string(),
    ]);
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
        bind(&mut args, path);
    }
    for path in profile.deny_reads.iter().chain(&profile.deny_writes) {
        hide_or_readonly(&mut args, path);
    }
    for path in git_write_locks(&profile.workspace) {
        readonly(&mut args, &path);
    }
    let git = profile.workspace.join(".git");
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
