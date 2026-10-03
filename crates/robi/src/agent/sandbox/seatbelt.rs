//! Seatbelt policy. Rules are last-match-wins, so a later deny closes a file
//! that an earlier directory allow opened.

use std::fmt::Write as _;
use std::path::Path;

use super::profile::{NetworkMode, Profile};

pub fn seatbelt_policy(profile: &Profile) -> String {
    let mut out = String::new();
    out.push_str("(version 1)\n");
    out.push_str("(deny default)\n");
    out.push_str("(allow process*)\n");
    out.push_str("(allow signal (target same-sandbox))\n");
    out.push_str("(allow file-ioctl)\n");
    out.push_str("(allow sysctl-read)\n");
    out.push_str("(allow file-read* (subpath \"/\"))\n");
    for path in outside_denies(&profile.home) {
        deny_subpath(&mut out, &path);
    }
    allow_read(&mut out, "/private/var/select");
    allow_read_write(&mut out, &profile.workspace);
    allow_read_write(&mut out, &profile.temp_dir);
    for path in &profile.toolchain_reads {
        allow_read(&mut out, path);
    }
    for path in &profile.wide_reads {
        allow_read(&mut out, path);
    }
    for path in &profile.wide_writes {
        allow_read_write(&mut out, path);
    }
    for path in credential_dirs(&profile.home) {
        deny_subpath(&mut out, &path);
    }
    for pattern in floor_regexes() {
        deny_regex(&mut out, &pattern);
    }
    for path in git_write_denies(&profile.workspace) {
        deny_write_subpath(&mut out, &path);
    }
    for path in &profile.deny_reads {
        deny_read_subpath(&mut out, path);
    }
    for path in &profile.deny_writes {
        deny_write_subpath(&mut out, path);
    }
    for path in &profile.protected_read_dirs {
        allow_read(&mut out, path);
        for pattern in floor_regexes() {
            deny_read_regex(&mut out, &pattern);
        }
    }
    for path in &profile.protected_write_dirs {
        allow_read_write(&mut out, path);
        for pattern in floor_regexes() {
            deny_regex(&mut out, &pattern);
        }
        for path in git_write_denies(&profile.workspace) {
            deny_write_subpath(&mut out, &path);
        }
    }
    for path in &profile.protected_read_files {
        allow_read(&mut out, path);
    }
    for path in &profile.protected_write_files {
        allow_read_write(&mut out, path);
    }
    allow_read(&mut out, "/private/etc/ssl");
    allow_read(&mut out, "/etc/ssl");
    out.push_str("(allow file-read* (literal \"/dev/null\"))\n");
    out.push_str("(allow file-write* (literal \"/dev/null\"))\n");
    out.push_str("(deny network*)\n");
    if profile.network == NetworkMode::Unrestricted {
        out.push_str("(allow network*)\n");
    }
    out.push_str("(deny process-info* (target others))\n");
    out.push_str("(deny appleevent-send)\n");
    out
}

fn outside_denies(home: &Path) -> Vec<String> {
    let mut paths = vec![
        "/Users".to_owned(),
        "/private".to_owned(),
        "/var".to_owned(),
        "/tmp".to_owned(),
        "/Volumes".to_owned(),
    ];
    if let Some(home) = home.to_str() {
        if !home.is_empty() {
            paths.push(home.to_owned());
        }
    }
    paths
}

fn credential_dirs(home: &Path) -> Vec<std::path::PathBuf> {
    [
        ".ssh",
        ".aws",
        ".kube",
        ".gnupg",
        "Library/Keychains",
        ".robi",
    ]
    .into_iter()
    .map(|name| home.join(name))
    .collect()
}

fn git_write_denies(workspace: &Path) -> Vec<std::path::PathBuf> {
    [
        workspace.join(".git/config"),
        workspace.join(".git/hooks"),
        workspace.join(".git/info/attributes"),
    ]
    .into_iter()
    .collect()
}

fn floor_regexes() -> Vec<String> {
    [
        r#".*/\.[Ee][Nn][Vv]$"#,
        r#".*/\.[Ee][Nn][Vv]\.[^/]+"#,
        r#".*/[^/]*\.[Pp][Ee][Mm]$"#,
        r#".*/[^/]*\.[Kk][Ee][Yy]$"#,
        r#".*/[Ii][Dd]_[Rr][Ss][Aa]$"#,
        r#".*/[Ii][Dd]_[Ee][Dd]25519$"#,
        r#".*/[Cc][Rr][Ee][Dd][Ee][Nn][Tt][Ii][Aa][Ll][Ss]\.[Jj][Ss][Oo][Nn]$"#,
        r#".*/[Ss][Ee][Cc][Rr][Ee][Tt][Ss]\.[Jj][Ss][Oo][Nn]$"#,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn allow_read(out: &mut String, path: impl AsRef<Path>) {
    let _ = writeln!(out, "(allow file-read* (subpath {}))", quote(path.as_ref()));
}

fn allow_read_write(out: &mut String, path: impl AsRef<Path>) {
    allow_read(out, path.as_ref());
    let _ = writeln!(
        out,
        "(allow file-write* (subpath {}))",
        quote(path.as_ref())
    );
}

fn deny_subpath(out: &mut String, path: impl AsRef<Path>) {
    deny_read_subpath(out, path.as_ref());
    deny_write_subpath(out, path.as_ref());
}

fn deny_read_subpath(out: &mut String, path: impl AsRef<Path>) {
    let _ = writeln!(out, "(deny file-read* (subpath {}))", quote(path.as_ref()));
}

fn deny_write_subpath(out: &mut String, path: impl AsRef<Path>) {
    let _ = writeln!(out, "(deny file-write* (subpath {}))", quote(path.as_ref()));
}

fn deny_regex(out: &mut String, pattern: &str) {
    deny_read_regex(out, pattern);
    let _ = writeln!(out, "(deny file-write* (regex \"{pattern}\"))");
}

fn deny_read_regex(out: &mut String, pattern: &str) {
    let _ = writeln!(out, "(deny file-read* (regex \"{pattern}\"))");
}

fn quote(path: &Path) -> String {
    let text = path.display().to_string().replace('\\', "\\\\");
    format!("\"{text}\"")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::agent::sandbox::profile::Profile;

    fn profile() -> Profile {
        Profile {
            sandboxed: true,
            workspace: PathBuf::from("/work/app"),
            home: PathBuf::from("/Users/ada"),
            temp_dir: PathBuf::from("/private/var/folders/robi"),
            cwd: PathBuf::from("/work/app"),
            network: NetworkMode::Deny,
            env: Vec::new(),
            toolchain_reads: vec![PathBuf::from("/Users/ada/.cargo/bin")],
            wide_reads: vec![PathBuf::from("/Users/ada/code/gopi")],
            wide_writes: Vec::new(),
            protected_read_files: vec![PathBuf::from("/work/app/.env")],
            protected_write_files: Vec::new(),
            protected_read_dirs: vec![PathBuf::from("/Users/ada/.ssh")],
            protected_write_dirs: Vec::new(),
            deny_reads: vec![PathBuf::from("/work/app/build")],
            deny_writes: Vec::new(),
        }
    }

    #[test]
    fn the_floor_comes_after_the_workspace_and_a_file_grant_comes_last() {
        let policy = seatbelt_policy(&profile());
        let workspace = policy
            .find("(allow file-write* (subpath \"/work/app\"))")
            .unwrap();
        let env_deny = policy
            .find("(deny file-read* (regex \".*/\\.[Ee][Nn][Vv]$\"))")
            .unwrap();
        let file_grant = policy
            .find("(allow file-read* (subpath \"/work/app/.env\"))")
            .unwrap();
        let home_deny = policy
            .find("(deny file-read* (subpath \"/Users/ada\"))")
            .unwrap();
        let outside = policy
            .find("(allow file-read* (subpath \"/Users/ada/code/gopi\"))")
            .unwrap();
        assert!(home_deny < workspace);
        assert!(workspace < env_deny);
        assert!(env_deny < file_grant);
        assert!(home_deny < outside);
        assert!(policy.contains("(deny file-write* (subpath \"/work/app/.git/hooks\"))"));
        assert!(policy.contains("(deny network*)"));
        assert!(!policy.contains("(allow network*)"));
    }

    #[test]
    fn a_protected_directory_is_reclosed_by_the_floor() {
        let policy = seatbelt_policy(&profile());
        let open = policy
            .find("(allow file-read* (subpath \"/Users/ada/.ssh\"))")
            .unwrap();
        let again = policy
            .rfind("(deny file-read* (regex \".*/[Ii][Dd]_[Rr][Ss][Aa]$\"))")
            .unwrap();
        assert!(open < again);
    }

    #[test]
    fn unrestricted_network_is_allowed_after_the_deny() {
        let mut profile = profile();
        profile.network = NetworkMode::Unrestricted;
        let policy = seatbelt_policy(&profile);
        let deny = policy.find("(deny network*)").unwrap();
        let allow = policy.find("(allow network*)").unwrap();
        assert!(deny < allow);
    }
}
