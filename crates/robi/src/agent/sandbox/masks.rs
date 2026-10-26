//! The workspace paths a sandbox must hide, whatever the mechanism.
//!
//! Seatbelt, bubblewrap, and the container sandbox all need the same list: every
//! protected file inside the workspace. `bwrap` emits one `--ro-bind /dev/null`
//! per path. The container emits one read-only bind mount per path. Keeping the
//! walk here is what stops the two lists from drifting apart.
//!
//! The walk is filesystem work. Callers run it on the blocking pool.

use std::path::{Path, PathBuf};

use super::profile::{classify_path, AccessClass, Profile};

/// Directories the walk does not descend into.
///
/// A build output holds generated files that the floor does not need to hide, and
/// descending into `target/` or `node_modules/` would cost thousands of `stat`
/// calls on every command.
const SKIP_DIRS: &[&str] = &["target", "node_modules", "dist", "vendor"];

/// Upper bound on directories visited. A workspace past it refuses the command
/// rather than spending seconds per `shell` call.
pub const MAX_VISITED: usize = 20_000;

/// Every protected file inside the workspace, as absolute paths.
///
/// A path the profile already opened explicitly is left alone: the grant wins
/// over the floor, and re-hiding it here would contradict the profile.
pub fn floor_files(profile: &Profile) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![profile.workspace.clone()];
    let mut visited = 0usize;
    while let Some(dir) = pending.pop() {
        visited += 1;
        if visited > MAX_VISITED {
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
                if !SKIP_DIRS.contains(&name) {
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
            if classify_path(&path, Path::new("/")) == AccessClass::ProtectedFile {
                found.push(path);
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
pub(crate) mod testing {
    use std::path::PathBuf;

    /// A scratch directory under `target/`, inside the workspace.
    ///
    /// Not `std::env::temp_dir()`: the test run may itself be sandboxed, and a
    /// nested sandbox denies writes outside the workspace.
    pub fn scratch_dir(name: &str) -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // `.robi` holds a project's `sandbox.toml`; every scratch workspace has one.
        std::fs::create_dir_all(root.join(".robi")).expect("create the scratch directory");
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::sandbox::masks::testing::scratch_dir;
    use crate::agent::sandbox::profile::{NetworkMode, WriteAccess};

    fn profile(workspace: PathBuf) -> Profile {
        Profile {
            sandboxed: true,
            write: WriteAccess::ReadWrite,
            workspace,
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

    /// Creating a floor file needs a test process the sandbox does not confine.
    ///
    /// When the suite itself runs inside Robi's sandbox, writing `.env` is exactly
    /// what the floor forbids, so the test cannot build its fixture. Skipping is
    /// honest: the assertion still runs in CI, where the harness is not nested.
    fn try_write(path: &Path, body: &str) -> bool {
        std::fs::write(path, body).is_ok()
    }

    #[test]
    fn secret_files_are_found_and_build_outputs_are_skipped() {
        let root = scratch_dir("masks-floor");
        if !try_write(&root.join(".env"), "TOKEN=1") {
            eprintln!("skipping: this test process is sandboxed against `.env` writes");
            return;
        }
        std::fs::write(root.join("id_rsa"), "key").unwrap();
        std::fs::write(root.join("main.rs"), "fn main() {}").unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/.env"), "generated").unwrap();

        let mut found = floor_files(&profile(root.clone())).unwrap();
        found.sort();
        let names: Vec<String> = found
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        // `main.rs` is ordinary workspace content. `target/.env` is skipped with
        // its parent directory, so the generated copy is not in the list either.
        assert_eq!(names, vec![".env".to_owned(), "id_rsa".to_owned()]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_explicit_grant_wins_over_the_floor() {
        let root = scratch_dir("masks-grant");
        let env = root.join(".env");
        if !try_write(&env, "TOKEN=1") {
            eprintln!("skipping: this test process is sandboxed against `.env` writes");
            return;
        }
        let mut profile = profile(root.clone());
        profile.protected_read_files = vec![env.clone()];

        let found = floor_files(&profile).unwrap();
        assert!(found.is_empty(), "found {found:?}");

        let _ = std::fs::remove_dir_all(&root);
    }
}
