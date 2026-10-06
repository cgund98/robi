//! The language servers v1 knows how to start.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub struct Language {
    pub id: &'static str,
    pub extensions: &'static [&'static str],
}

pub struct ServerSpec {
    pub id: &'static str,
    pub argv: &'static [&'static str],
    pub languages: &'static [Language],
    /// Returned for `workspace/configuration`. Keyed by section.
    pub settings: fn() -> Value,
}

const SERVERS: &[ServerSpec] = &[
    ServerSpec {
        id: "rust-analyzer",
        argv: &["rust-analyzer"],
        languages: &[Language {
            id: "rust",
            extensions: &["rs"],
        }],
        settings: rust_settings,
    },
    ServerSpec {
        id: "typescript-language-server",
        argv: &["typescript-language-server", "--stdio"],
        languages: &[
            Language {
                id: "typescript",
                extensions: &["ts", "tsx", "mts", "cts"],
            },
            Language {
                id: "javascript",
                extensions: &["js", "jsx", "mjs", "cjs"],
            },
        ],
        settings: empty_settings,
    },
    ServerSpec {
        id: "ruff",
        argv: &["ruff", "server"],
        languages: &[Language {
            id: "python",
            extensions: &["py", "pyi"],
        }],
        settings: empty_settings,
    },
    ServerSpec {
        id: "pyright",
        argv: &["pyright-langserver", "--stdio"],
        languages: &[Language {
            id: "python",
            extensions: &["py", "pyi"],
        }],
        settings: empty_settings,
    },
    ServerSpec {
        id: "gopls",
        argv: &["gopls"],
        languages: &[Language {
            id: "go",
            extensions: &["go"],
        }],
        settings: empty_settings,
    },
    ServerSpec {
        id: "clangd",
        argv: &["clangd"],
        languages: &[
            Language {
                id: "c",
                extensions: &["c", "h"],
            },
            Language {
                id: "cpp",
                extensions: &["cc", "cpp", "cxx", "hh", "hpp"],
            },
        ],
        settings: empty_settings,
    },
];

fn rust_settings() -> Value {
    // `cargo metadata` and `cargo check` otherwise rewrite `Cargo.lock` and
    // the default `target` directory. A watcher on those paths, including
    // `make dev-api`, then restarts the API while the tool call is still open.
    json!({
        "rust-analyzer": {
            "checkOnSave": true,
            "cargo": {
                "targetDir": true,
                "extraArgs": ["--locked"]
            }
        }
    })
}

fn empty_settings() -> Value {
    json!({})
}

pub fn servers() -> &'static [ServerSpec] {
    SERVERS
}

pub fn spec(id: &str) -> Option<&'static ServerSpec> {
    SERVERS.iter().find(|server| server.id == id)
}

/// One catalog row that can serve a file, and whether its binary was found.
pub struct Choice {
    pub spec: &'static ServerSpec,
    pub language: &'static str,
    pub installed: bool,
}

/// Servers for this extension, in preference order.
pub fn matching(path: &Path) -> Vec<(&'static ServerSpec, &'static str)> {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for server in SERVERS {
        for language in server.languages {
            if language.extensions.contains(&ext) {
                found.push((server, language.id));
            }
        }
    }
    found
}

/// The first installed server for this extension.
///
/// When the extension matches and no binary is on `PATH`, the first row is
/// returned with `installed: false` so the tool can name that binary.
pub fn select(path: &Path, installed: impl Fn(&str) -> bool) -> Option<Choice> {
    let matches = matching(path);
    let chosen = matches
        .iter()
        .copied()
        .find(|(spec, _)| installed(spec.argv[0]));
    let installed = chosen.is_some();
    let (spec, language) = chosen.or_else(|| matches.first().copied())?;
    Some(Choice {
        spec,
        language,
        installed,
    })
}

/// First entry of the MCP host's resolved `PATH` that names an executable `bin`.
#[cfg(test)]
pub fn find_on_path(bin: &str) -> Option<PathBuf> {
    find_on_path_in(&crate::agent::mcp::resolve_path(""), bin)
}

/// First entry of `path` that names an executable `bin`.
pub fn find_on_path_in(path: &str, bin: &str) -> Option<PathBuf> {
    for dir in std::env::split_paths(path) {
        let candidate = dir.join(bin);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Value of one `workspace/configuration` item.
pub fn configuration_value(settings: &Value, section: Option<&str>) -> Value {
    let Some(section) = section.filter(|section| !section.is_empty()) else {
        return settings.clone();
    };
    let mut current = settings;
    for part in section.split('.') {
        match current.get(part) {
            Some(next) => current = next,
            None => return Value::Null,
        }
    }
    current.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_pick_one_server() {
        let rust = select(Path::new("src/lib.rs"), |_| true).unwrap();
        assert_eq!(rust.spec.id, "rust-analyzer");
        assert_eq!(rust.language, "rust");
        let typescript = select(Path::new("web/App.tsx"), |_| true).unwrap();
        assert_eq!(typescript.spec.id, "typescript-language-server");
        assert_eq!(typescript.language, "typescript");
        let c = select(Path::new("main.c"), |_| true).unwrap();
        assert_eq!(c.spec.id, "clangd");
        assert_eq!(c.language, "c");
        assert!(matching(Path::new("notes.md")).is_empty());
    }

    #[test]
    fn python_prefers_ruff_and_falls_back_to_pyright() {
        let ruff = select(Path::new("app.py"), |bin| bin == "ruff").unwrap();
        assert_eq!(ruff.spec.id, "ruff");
        assert!(ruff.installed);
        assert_eq!(ruff.language, "python");
        assert_eq!(ruff.spec.argv, ["ruff", "server"]);

        let both = select(Path::new("app.py"), |bin| {
            bin == "ruff" || bin == "pyright-langserver"
        })
        .unwrap();
        assert_eq!(both.spec.id, "ruff");

        let pyright = select(Path::new("app.pyi"), |bin| bin == "pyright-langserver").unwrap();
        assert_eq!(pyright.spec.id, "pyright");
        assert!(pyright.installed);

        let neither = select(Path::new("app.py"), |_| false).unwrap();
        assert_eq!(neither.spec.id, "ruff");
        assert!(!neither.installed);
    }

    #[test]
    fn rust_analyzer_check_on_save_is_a_section() {
        let settings = rust_settings();
        assert_eq!(
            configuration_value(&settings, Some("rust-analyzer.checkOnSave")),
            json!(true)
        );
        assert_eq!(
            configuration_value(&settings, Some("rust-analyzer")),
            json!({
                "checkOnSave": true,
                "cargo": { "targetDir": true, "extraArgs": ["--locked"] }
            })
        );
        assert_eq!(
            configuration_value(&settings, Some("rust-analyzer.cargo.targetDir")),
            json!(true)
        );
    }
}
