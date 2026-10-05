//! API process logs under `~/.robi/logs/`.
//!
//! Each start opens a new log file and never truncates an existing one. A
//! release build uses `robi-api-<timestamp>.log`. A dev build, including
//! `pnpm tauri dev`, uses `robi-dev-<timestamp>.log`, so the two do not prune
//! each other's files. The newest file from the previous start is kept even
//! when it is old or the directory is over the cap, so the run you just left
//! is still there after a reopen.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use chrono::{SecondsFormat, Utc};
use tracing_subscriber::prelude::*;

/// Files kept after a start, including the new file. The previous run is kept
/// even when that puts the directory over this count.
const MAX_LOG_FILES: usize = 8;

/// Files older than this are deleted, except the previous run.
const MAX_LOG_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Release builds. Dev builds use [`DEV_PREFIX`] so a debug run does not prune
/// the installed app's logs, and the other way around. `robi-dev-` does not
/// start with `robi-api-`, so a prefix check cannot match both.
const PREFIX: &str = "robi-api-";
const DEV_PREFIX: &str = "robi-dev-";
const SUFFIX: &str = ".log";

/// The `logs` directory under `~/.robi`, and the `mcp` subtree that holds one
/// directory per MCP server.
pub(crate) const LOGS_DIR: &str = "logs";
pub(crate) const MCP_DIR: &str = "mcp";

/// `~/.robi/logs`, where the process writes its own log files.
pub(crate) fn logs_dir(home: &Path) -> PathBuf {
    home.join(LOGS_DIR)
}

/// `~/.robi/logs/mcp`, one subdirectory per MCP server id.
pub(crate) fn mcp_logs_dir(home: &Path) -> PathBuf {
    logs_dir(home).join(MCP_DIR)
}

fn log_prefix() -> &'static str {
    if cfg!(debug_assertions) {
        DEV_PREFIX
    } else {
        PREFIX
    }
}

/// The file this process is appending to. A startup failure writes here
/// directly: the background logger does not flush when the process aborts.
static LOG_FILE: OnceLock<PathBuf> = OnceLock::new();

/// Install the process-wide tracing subscriber.
///
/// Writes a new log file when `~/.robi` can be resolved. Debug builds also
/// write the same events to stderr. `RUST_LOG` selects verbosity, defaulting
/// to `robi=info`.
pub fn init() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "robi=info".into());

    let file_layer = match home_logs() {
        Ok((home, dir)) => match open_log_file(&dir) {
            Ok((path, file)) => {
                if let Err(err) = prune_logs(&dir, &path, SystemTime::now()) {
                    eprintln!("could not prune old log files: {err}");
                }
                // Prune the per-server MCP logs too. A server removed from the
                // config still has its old files expire here.
                crate::agent::mcp::logs::prune_orphans(&home);
                let _ = LOG_FILE.set(path.clone());
                install_panic_hook();
                let (writer, guard) = tracing_appender::non_blocking(file);
                // The guard flushes the worker on drop. It has to outlive the process.
                Box::leak(Box::new(guard));
                Some(
                    tracing_subscriber::fmt::layer()
                        .with_writer(writer)
                        .with_ansi(false),
                )
            }
            Err(err) => {
                eprintln!("could not open the log file: {err}");
                None
            }
        },
        Err(err) => {
            eprintln!("{err}");
            None
        }
    };

    let registry = tracing_subscriber::registry().with(filter);
    match file_layer {
        Some(file_layer) => {
            let registry = registry.with(file_layer);
            #[cfg(debug_assertions)]
            {
                registry.with(tracing_subscriber::fmt::layer()).init();
            }
            #[cfg(not(debug_assertions))]
            {
                registry.init();
            }
        }
        None => {
            registry.with(tracing_subscriber::fmt::layer()).init();
        }
    }
}

/// Append one error line and flush it. Used when the process is about to
/// abort and the background logger would lose the line.
pub fn record_error(message: &str) {
    let stamp = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
    let line = format!("{stamp} ERROR {message}");
    eprintln!("{line}");
    let Some(path) = LOG_FILE.get() else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new().append(true).open(path) {
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        record_error(&panic_message(info));
        previous(info);
    }));
}

fn panic_message(info: &PanicHookInfo<'_>) -> String {
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
        .unwrap_or("Box<dyn Any>");
    match info.location() {
        Some(location) => format!(
            "panic at {}:{}:{}: {payload}",
            location.file(),
            location.line(),
            location.column()
        ),
        None => format!("panic: {payload}"),
    }
}

fn home_logs() -> io::Result<(PathBuf, PathBuf)> {
    let home = crate::adapters::settings::home_dir()
        .map_err(|err| io::Error::new(ErrorKind::NotFound, err.to_string()))?;
    let dir = logs_dir(&home);
    fs::create_dir_all(&dir)?;
    Ok((home, dir))
}

/// Create a new log file. A name collision gets a numeric suffix rather than
/// an append or a truncate.
fn open_log_file(dir: &Path) -> io::Result<(PathBuf, File)> {
    let stamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let stamp = stamp.replace(':', "-");
    let mut n = 1u32;
    loop {
        let name = if n == 1 {
            format!("{}{stamp}{SUFFIX}", log_prefix())
        } else {
            format!("{}{stamp}-{n}{SUFFIX}", log_prefix())
        };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                n += 1;
                if n > 100 {
                    return Err(err);
                }
            }
            Err(err) => return Err(err),
        }
    }
}

fn prune_logs(dir: &Path, current: &Path, now: SystemTime) -> io::Result<()> {
    prune(
        dir,
        Some(current),
        &is_log_file,
        MAX_LOG_FILES,
        MAX_LOG_AGE,
        now,
    )
}

/// Prune one MCP server's directory with the shared policy.
///
/// `current` is the file just opened by that server, when there is one. Called
/// on open and, with `None`, by the startup orphan sweep.
pub(crate) fn prune_mcp_dir(dir: &Path, current: Option<&Path>, now: SystemTime) -> io::Result<()> {
    prune(
        dir,
        current,
        &is_mcp_log_file,
        MAX_LOG_FILES,
        MAX_LOG_AGE,
        now,
    )
}

/// Drop log files in `dir` by the shared policy.
///
/// `current` is the file this start just created, when there is one. It is
/// kept, along with the newest other file, even when that file is old or the
/// directory is over `max_files`. With no `current`, that exemption does not
/// apply and every file ages out or falls to the cap. Files older than
/// `max_age` are removed, and at most `max_files` are kept.
fn prune(
    dir: &Path,
    current: Option<&Path>,
    is_log: &dyn Fn(&Path) -> bool,
    max_files: usize,
    max_age: Duration,
    now: SystemTime,
) -> io::Result<()> {
    let mut older = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if Some(path.as_path()) == current || !is_log(&path) {
            continue;
        }
        let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
        older.push((modified, path));
    }
    older.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut kept = usize::from(current.is_some());
    for (index, (modified, path)) in older.iter().enumerate() {
        if current.is_some() && index == 0 {
            // The newest prior run. Kept even when it is old or over the cap,
            // so the run you just left is still there after a reopen.
            kept += 1;
            continue;
        }
        let aged = modified
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > max_age);
        if aged || kept >= max_files {
            let _ = fs::remove_file(path);
        } else {
            kept += 1;
        }
    }
    Ok(())
}

fn is_log_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with(log_prefix()) && name.ends_with(SUFFIX)
}

/// One MCP server log: `mcp-<timestamp>.log`.
fn is_mcp_log_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with("mcp-") && name.ends_with(SUFFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path, age: Duration) {
        fs::write(path, b"log").unwrap();
        let modified = SystemTime::now() - age;
        let file = File::options().write(true).open(path).unwrap();
        file.set_modified(modified).unwrap();
    }

    #[test]
    fn record_error_flushes_the_line_to_the_log_file() {
        let dir = std::env::temp_dir().join(format!("robi-logs-error-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("robi-api-error.log");
        fs::write(&path, b"").unwrap();
        LOG_FILE.set(path.clone()).ok();

        record_error("migration 9 was missing");

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("ERROR migration 9 was missing"), "{text}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn keeps_the_previous_run_and_drops_old_or_excess_files() {
        let dir = std::env::temp_dir().join(format!("robi-logs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let now = SystemTime::now();
        let current = dir.join(format!("{}current.log", log_prefix()));
        fs::write(&current, b"now").unwrap();

        // Newest prior file, and older than the age cap. It stays.
        let previous = dir.join(format!("{}previous.log", log_prefix()));
        touch(&previous, Duration::from_secs(1));

        let young: Vec<_> = (0..6)
            .map(|i| {
                let path = dir.join(format!("{}young-{i}.log", log_prefix()));
                touch(&path, Duration::from_secs(60 * (i as u64 + 2)));
                path
            })
            .collect();
        let excess = dir.join(format!("{}excess.log", log_prefix()));
        touch(&excess, Duration::from_secs(60 * 60));
        let ancient = dir.join(format!("{}ancient.log", log_prefix()));
        touch(&ancient, Duration::from_secs(10 * 24 * 60 * 60));
        fs::write(dir.join("notes.txt"), b"leave me").unwrap();
        let other_prefix = if cfg!(debug_assertions) {
            PREFIX
        } else {
            DEV_PREFIX
        };
        let other = dir.join(format!("{other_prefix}other.log"));
        touch(&other, Duration::from_secs(10 * 24 * 60 * 60));

        prune_logs(&dir, &current, now).unwrap();

        assert!(current.exists());
        assert!(previous.exists());
        for path in &young {
            assert!(path.exists(), "missing {}", path.display());
        }
        assert!(!excess.exists());
        assert!(!ancient.exists());
        assert!(dir.join("notes.txt").exists());
        assert!(other.exists());

        let only = std::env::temp_dir().join(format!("robi-logs-old-{}", std::process::id()));
        let _ = fs::remove_dir_all(&only);
        fs::create_dir_all(&only).unwrap();
        let current_only = only.join(format!("{}current.log", log_prefix()));
        fs::write(&current_only, b"now").unwrap();
        let old_previous = only.join(format!("{}old.log", log_prefix()));
        touch(&old_previous, Duration::from_secs(30 * 24 * 60 * 60));
        let also_old = only.join(format!("{}also-old.log", log_prefix()));
        touch(&also_old, Duration::from_secs(40 * 24 * 60 * 60));
        prune_logs(&only, &current_only, SystemTime::now()).unwrap();
        assert!(old_previous.exists());
        assert!(!also_old.exists());
        let _ = fs::remove_dir_all(&only);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_prune_without_a_current_file_drops_everything_aged() {
        let dir = std::env::temp_dir().join(format!("robi-logs-orphan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let old = dir.join("mcp-old.log");
        touch(&old, Duration::from_secs(30 * 24 * 60 * 60));
        let young = dir.join("mcp-young.log");
        touch(&young, Duration::from_secs(60));
        prune(
            &dir,
            None,
            &is_mcp_log_file,
            MAX_LOG_FILES,
            MAX_LOG_AGE,
            SystemTime::now(),
        )
        .unwrap();
        assert!(!old.exists());
        assert!(young.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
