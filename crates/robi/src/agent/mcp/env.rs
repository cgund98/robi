//! The environment an MCP stdio server receives. The parent environment is not copied.

use std::collections::BTreeMap;
use std::path::Path;

use crate::agent::sandbox::{command_env, EnvInput};

pub fn child_env(workspace: &Path, overlay: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| workspace.to_path_buf());
    let parent_path = std::env::var("PATH").unwrap_or_default();
    let temp = std::env::temp_dir();
    let mut env = command_env(&EnvInput {
        home: &home,
        workspace,
        temp_dir: &temp,
        sandboxed: false,
        parent_path: &parent_path,
        extra_path: "",
        path_prefix: "",
        lang: std::env::var("LANG").ok().as_deref(),
        user: std::env::var("USER").ok().as_deref(),
    });
    for (key, value) in overlay {
        env.retain(|(name, _)| name != key);
        env.push((key.clone(), value.clone()));
    }
    env
}

pub fn resolve_command(command: &str, env: &[(String, String)]) -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(command);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    let path_value = env.iter().find(|(key, _)| key == "PATH")?.1.as_str();
    for dir in path_value.split(':') {
        let candidate = std::path::Path::new(dir).join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scrub_keeps_the_config_secret_and_drops_the_parent_key() {
        let root = std::env::temp_dir().join(format!("robi-mcp-env-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let overlay = BTreeMap::from([("GITHUB_TOKEN".into(), "from-config".into())]);
        let env = child_env(&root, &overlay);
        assert!(env
            .iter()
            .any(|(key, value)| key == "GITHUB_TOKEN" && value == "from-config"));
        assert!(env.iter().any(|(key, _)| key == "HOME"));
        assert!(env.iter().any(|(key, _)| key == "PATH"));
        assert!(!env.iter().any(|(key, _)| key == "OPENAI_API_KEY"));
        assert!(!env.iter().any(|(key, _)| key == "DYLD_INSERT_LIBRARIES"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
