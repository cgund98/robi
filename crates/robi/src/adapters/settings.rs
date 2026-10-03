//! `~/.robi/config.toml` and `~/.robi/secrets.toml`.
//!
//! Memory is what `get` returns. `set` updates that map, then rewrites both
//! files. A failed write puts the previous value back, so a caller that sees
//! an error does not observe the rejected write.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::Mutex;

use crate::domain::{
    error::ServiceError,
    settings::store::{Setting, SettingsStore},
};

const CONFIG_FILE: &str = "config.toml";
const SECRETS_FILE: &str = "secrets.toml";

/// `~/.robi`, from `HOME`.
pub fn home_dir() -> Result<PathBuf, SettingsLoadError> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty());
    let Some(home) = home else {
        return Err(SettingsLoadError::NoHome);
    };
    Ok(PathBuf::from(home).join(".robi"))
}

/// Why startup could not load the two files.
#[derive(Debug, Error)]
pub enum SettingsLoadError {
    #[error("could not resolve the home directory")]
    NoHome,

    #[error("{path} is mode {mode:o}; refuse to load unless it is 0600")]
    Insecure { path: PathBuf, mode: u32 },

    #[error("could not read settings in {path}: {message}")]
    Io { path: PathBuf, message: String },

    #[error("could not parse {path}: {message}")]
    Parse { path: PathBuf, message: String },

    #[error("{path}: {key} must be a string")]
    NotString { path: PathBuf, key: String },
}

/// Settings kept in memory and synced to the two TOML files under `dir`.
pub struct TomlSettingsStore {
    dir: PathBuf,
    values: Mutex<BTreeMap<String, Setting>>,
}

impl TomlSettingsStore {
    /// Create `dir` when it is missing, then load both files into memory.
    ///
    /// A missing file is an empty map. A group- or world-readable secrets file
    /// is refused. When a key is in both files, the secrets copy wins.
    pub fn load(dir: impl Into<PathBuf>) -> Result<Self, SettingsLoadError> {
        let dir = dir.into();
        ensure_private_dir(&dir)?;
        let mut values = read_table(&dir.join(CONFIG_FILE), false)?;
        for (key, setting) in read_table(&dir.join(SECRETS_FILE), true)? {
            values.insert(key, setting);
        }
        Ok(Self {
            dir,
            values: Mutex::new(values),
        })
    }
}

impl fmt::Debug for TomlSettingsStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let values = match self.values.try_lock() {
            Ok(guard) => format!("{guard:?}"),
            Err(_) => "<locked>".to_owned(),
        };
        f.debug_struct("TomlSettingsStore")
            .field("dir", &self.dir)
            .field("values", &values)
            .finish()
    }
}

#[async_trait]
impl SettingsStore for TomlSettingsStore {
    async fn get(&self, key: &str) -> Result<Option<Setting>, ServiceError> {
        Ok(self.values.lock().await.get(key).cloned())
    }

    async fn set(&self, key: &str, value: String, secret: bool) -> Result<(), ServiceError> {
        let mut values = self.values.lock().await;
        let previous = values.get(key).cloned();
        values.insert(key.to_owned(), Setting { value, secret });
        if let Err(error) = sync_files(&self.dir, &values) {
            match previous {
                Some(previous) => {
                    values.insert(key.to_owned(), previous);
                }
                None => {
                    values.remove(key);
                }
            }
            tracing::error!(error = %error, "failed to sync settings");
            return Err(ServiceError::Unknown);
        }
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<(), ServiceError> {
        let mut values = self.values.lock().await;
        let previous = values.remove(key);
        if let Err(error) = sync_files(&self.dir, &values) {
            if let Some(previous) = previous {
                values.insert(key.to_owned(), previous);
            }
            tracing::error!(error = %error, "failed to sync settings");
            return Err(ServiceError::Unknown);
        }
        Ok(())
    }
}

fn ensure_private_dir(dir: &Path) -> Result<(), SettingsLoadError> {
    fs::create_dir_all(dir).map_err(|error| SettingsLoadError::Io {
        path: dir.to_owned(),
        message: error.to_string(),
    })?;
    set_mode(dir, 0o700).map_err(|error| SettingsLoadError::Io {
        path: dir.to_owned(),
        message: error.to_string(),
    })
}

fn read_table(path: &Path, secret: bool) -> Result<BTreeMap<String, Setting>, SettingsLoadError> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    if secret {
        require_private(path)?;
    }
    let body = fs::read_to_string(path).map_err(|error| SettingsLoadError::Io {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    if body.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    let table: toml::Table = toml::from_str(&body).map_err(|error| SettingsLoadError::Parse {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    let mut values = BTreeMap::new();
    for (key, value) in table {
        let Some(text) = value.as_str() else {
            return Err(SettingsLoadError::NotString {
                path: path.to_owned(),
                key,
            });
        };
        values.insert(
            key,
            Setting {
                value: text.to_owned(),
                secret,
            },
        );
    }
    Ok(values)
}

fn require_private(path: &Path) -> Result<(), SettingsLoadError> {
    let mode = file_mode(path).map_err(|error| SettingsLoadError::Io {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    if mode & 0o077 != 0 {
        return Err(SettingsLoadError::Insecure {
            path: path.to_owned(),
            mode,
        });
    }
    Ok(())
}

fn sync_files(dir: &Path, values: &BTreeMap<String, Setting>) -> Result<(), String> {
    let mut config = BTreeMap::new();
    let mut secrets = BTreeMap::new();
    for (key, setting) in values {
        if setting.secret {
            secrets.insert(key.clone(), setting.value.clone());
        } else {
            config.insert(key.clone(), setting.value.clone());
        }
    }
    // Secrets first, so a failure while writing config cannot leave a secret
    // that was just unclassified sitting in config.toml.
    write_private(dir, SECRETS_FILE, &encode_table(&secrets)?)?;
    write_private(dir, CONFIG_FILE, &encode_table(&config)?)?;
    Ok(())
}

fn encode_table(values: &BTreeMap<String, String>) -> Result<String, String> {
    toml::to_string_pretty(values).map_err(|error| error.to_string())
}

fn write_private(dir: &Path, name: &str, body: &str) -> Result<(), String> {
    let destination = dir.join(name);
    let temporary = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let write = (|| -> Result<(), String> {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        set_mode(&temporary, 0o600).map_err(|error| error.to_string())?;
        file.write_all(body.as_bytes())
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
        Ok(())
    })();
    if write.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write
}

fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    let mut permissions = fs::metadata(path)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(mode);
    }
    let _ = mode;
    fs::set_permissions(path, permissions)
}

#[cfg(unix)]
fn file_mode(path: &Path) -> std::io::Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    Ok(fs::metadata(path)?.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn file_mode(path: &Path) -> std::io::Result<u32> {
    fs::metadata(path)?;
    Ok(0o600)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::domain::settings::keys::OPENCODE_GO_API_KEY;

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("robi-settings-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct TempHome(PathBuf);

    impl TempHome {
        fn new() -> Self {
            Self(temp_dir())
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
            let _ = fs::remove_file(&self.0);
        }
    }

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_missing_directory_loads_empty() {
        let home = TempHome::new();
        fs::remove_dir_all(home.path()).unwrap();
        let store = TomlSettingsStore::load(home.path()).unwrap();
        assert!(store.values.blocking_lock().is_empty());
        assert_eq!(mode_of(home.path()), 0o700);
    }

    #[tokio::test]
    async fn a_set_syncs_secrets_apart_from_config_and_a_reload_sees_them() {
        let home = TempHome::new();
        let store = TomlSettingsStore::load(home.path()).unwrap();
        store.set("model", "glm-5.3".into(), false).await.unwrap();
        store
            .set(OPENCODE_GO_API_KEY, "sk-live".into(), true)
            .await
            .unwrap();

        let live = store.get(OPENCODE_GO_API_KEY).await.unwrap().unwrap();
        assert_eq!(live.value, "sk-live");
        assert!(live.secret);
        let rendered = format!("{store:?}");
        assert!(
            !rendered.contains("sk-live"),
            "debug output must not contain the secret: {rendered}"
        );

        let config = fs::read_to_string(home.path().join(CONFIG_FILE)).unwrap();
        let secrets = fs::read_to_string(home.path().join(SECRETS_FILE)).unwrap();
        assert!(config.contains("glm-5.3"));
        assert!(!config.contains("sk-live"));
        assert!(secrets.contains("sk-live"));
        assert!(!secrets.contains("glm-5.3"));
        assert_eq!(mode_of(&home.path().join(CONFIG_FILE)), 0o600);
        assert_eq!(mode_of(&home.path().join(SECRETS_FILE)), 0o600);

        let reloaded = TomlSettingsStore::load(home.path()).unwrap();
        let model = reloaded.get("model").await.unwrap().unwrap();
        assert_eq!(model.value, "glm-5.3");
        assert!(!model.secret);
        let key = reloaded.get(OPENCODE_GO_API_KEY).await.unwrap().unwrap();
        assert_eq!(key.value, "sk-live");
        assert!(key.secret);
    }

    #[test]
    fn a_group_readable_secrets_file_is_refused() {
        let home = TempHome::new();
        let path = home.path().join(SECRETS_FILE);
        fs::write(&path, "opencode_go_api_key = \"sk-live\"\n").unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o644);
        fs::set_permissions(&path, permissions).unwrap();

        let error = TomlSettingsStore::load(home.path()).unwrap_err();
        assert!(
            error.to_string().contains("0600"),
            "the error names the required mode: {error}"
        );
    }

    #[test]
    fn a_key_in_both_files_keeps_the_secret_copy() {
        let home = TempHome::new();
        fs::write(home.path().join(CONFIG_FILE), "token = \"public\"\n").unwrap();
        let secrets = home.path().join(SECRETS_FILE);
        fs::write(&secrets, "token = \"private\"\n").unwrap();
        let mut permissions = fs::metadata(&secrets).unwrap().permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&secrets, permissions).unwrap();

        let store = TomlSettingsStore::load(home.path()).unwrap();
        let setting = store.values.blocking_lock().get("token").unwrap().clone();
        assert_eq!(setting.value, "private");
        assert!(setting.secret);
    }

    #[tokio::test]
    async fn a_failed_write_does_not_keep_the_new_value() {
        let home = TempHome::new();
        let store = TomlSettingsStore::load(home.path()).unwrap();
        store.set("model", "glm-5.3".into(), false).await.unwrap();

        fs::remove_dir_all(home.path()).unwrap();
        fs::write(home.path(), b"not a directory").unwrap();

        let error = store.set("model", "other".into(), false).await.unwrap_err();
        assert_eq!(error, ServiceError::Unknown);
        let current = store.get("model").await.unwrap().unwrap();
        assert_eq!(current.value, "glm-5.3");
    }
}
