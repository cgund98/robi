//! Process logs under `~/.robi/logs/`.
//!
//! Each start opens a new log file and never truncates an existing one. The
//! name says which process wrote it: `robi-api-<timestamp>.log` for a release
//! API or app process, `robi-dev-<timestamp>.log` for a debug one, and
//! `tui-<timestamp>.log` for the terminal front end. Each prefix is pruned on
//! its own, so a TUI run never prunes the app's logs or the other way around.
//! The newest file from the previous start is kept even when it is old or the
//! directory is over the cap, so the run you just left is still there after a
//! reopen.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
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

/// The terminal front end. It is a separate process on the same directory, so
/// its files carry their own prefix: `tui-` says which program wrote the file,
/// where `robi-dev-` could be either the TUI or `pnpm tauri dev`.
const TUI_PREFIX: &str = "tui-";

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

/// The file-name prefix `target` writes and prunes by.
fn log_prefix(target: Target) -> &'static str {
    match target {
        Target::Tui => TUI_PREFIX,
        Target::Api if cfg!(debug_assertions) => DEV_PREFIX,
        Target::Api => PREFIX,
    }
}

/// Keeps the background writer alive, and is dropped by [`flush`].
///
/// `tracing_appender` flushes its queue when the guard drops, so the guard has
/// to live as long as the process logs. Holding it here rather than leaking it
/// is what makes a last line writable: `process::exit` does not run destructors,
/// and the worker does not drain on its own.
static LOG_GUARD: Mutex<Option<tracing_appender::non_blocking::WorkerGuard>> = Mutex::new(None);

/// The file this process is appending to. A startup failure writes here
/// directly: the background logger does not flush when the process aborts.
static LOG_FILE: OnceLock<PathBuf> = OnceLock::new();

/// Which process is installing the subscriber.
///
/// Only [`Target::Api`] may write to stderr: a terminal front end draws on
/// stderr's target, so a tracing line there overwrites the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// `robi-api` and the Tauri process.
    Api,
    /// The terminal front end, `robi`.
    Tui,
}

/// Whether `target` writes the same events to stderr.
///
/// `Api` does in a debug build, which is what `pnpm tauri dev` reads. `Tui`
/// never does, in any build: the terminal owns those cells.
fn stderr_layer(target: Target) -> bool {
    match target {
        Target::Api => cfg!(debug_assertions),
        Target::Tui => false,
    }
}

/// Install the process-wide tracing subscriber.
///
/// Writes a new log file when `~/.robi` can be resolved. Debug builds also
/// write the same events to stderr. `RUST_LOG` selects verbosity, defaulting
/// to `robi=info`.
pub fn init() {
    install(Target::Api);
}

/// Same file layer as [`init`], and never a stderr layer.
///
/// A terminal front end renders on the cells stderr would write to, so one
/// tracing line there corrupts the frame. This holds in every build, which is
/// why it is a separate entry point rather than a `debug_assertions` check.
pub fn init_tui() {
    install(Target::Tui);
}

fn install(target: Target) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "robi=info".into());

    let file_layer = match home_logs() {
        Ok((home, dir)) => match open_log_file(&dir, target) {
            Ok((path, file)) => {
                if let Err(err) = prune_logs(&dir, &path, target, SystemTime::now()) {
                    eprintln!("could not prune old log files: {err}");
                }
                // Prune the per-server MCP logs too. A server removed from the
                // config still has its old files expire here.
                crate::agent::mcp::logs::prune_orphans(&home);
                let _ = LOG_FILE.set(path.clone());
                install_panic_hook();
                let (writer, guard) = tracing_appender::non_blocking(file);
                if let Ok(mut slot) = LOG_GUARD.lock() {
                    *slot = Some(guard);
                }
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
    match (file_layer, stderr_layer(target)) {
        (Some(file_layer), true) => {
            registry
                .with(file_layer)
                .with(tracing_subscriber::fmt::layer())
                .init();
        }
        (Some(file_layer), false) => {
            registry.with(file_layer).init();
        }
        (None, true) => {
            registry.with(tracing_subscriber::fmt::layer()).init();
        }
        // A terminal front end with no resolvable log file logs nowhere. The
        // alternative writes over the frame it is drawing.
        (None, false) => {
            registry.init();
        }
    }
}

/// Write out whatever the background logger still holds, then stop it.
///
/// Call this last, just before the process ends. Without it the tail of the log
/// is lost: the worker flushes on drop, and `process::exit` skips that.
pub fn flush() {
    if let Ok(mut slot) = LOG_GUARD.lock() {
        slot.take();
    }
}

/// The file this process is logging to, once it has one.
///
/// The terminal front end cannot print the path on stderr, so its first log
/// line names the file instead.
pub fn log_file() -> Option<&'static Path> {
    LOG_FILE.get().map(PathBuf::as_path)
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
fn open_log_file(dir: &Path, target: Target) -> io::Result<(PathBuf, File)> {
    let stamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let stamp = stamp.replace(':', "-");
    let mut n = 1u32;
    loop {
        let name = if n == 1 {
            format!("{}{stamp}{SUFFIX}", log_prefix(target))
        } else {
            format!("{}{stamp}-{n}{SUFFIX}", log_prefix(target))
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

fn prune_logs(dir: &Path, current: &Path, target: Target, now: SystemTime) -> io::Result<()> {
    prune(
        dir,
        Some(current),
        &|path| is_log_file(path, target),
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

/// A log file this process owns: its own prefix, and not another's.
fn is_log_file(path: &Path, target: Target) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.starts_with(log_prefix(target)) && name.ends_with(SUFFIX)
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

    #[test]
    fn the_terminal_front_end_never_writes_to_stderr() {
        assert!(
            !stderr_layer(Target::Tui),
            "a tracing line on stderr overwrites the frame the TUI draws"
        );
        assert_eq!(
            stderr_layer(Target::Api),
            cfg!(debug_assertions),
            "the API keeps its debug-build stderr layer"
        );
    }

    #[test]
    fn flush_writes_the_line_the_background_worker_still_holds() {
        let dir = std::env::temp_dir().join(format!("robi-logs-flush-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tui-flush.log");
        let file = File::create(&path).unwrap();
        let (writer, guard) = tracing_appender::non_blocking(file);
        *LOG_GUARD.lock().unwrap() = Some(guard);

        use tracing_subscriber::fmt::MakeWriter;
        writer
            .make_writer()
            .write_all(b"terminal front end exited\n")
            .unwrap();
        flush();

        assert!(LOG_GUARD.lock().unwrap().is_none(), "flush takes the guard");
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text, "terminal front end exited\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_process_writes_and_prunes_by_its_own_prefix() {
        assert_eq!(log_prefix(Target::Tui), "tui-");
        assert_eq!(
            log_prefix(Target::Api),
            if cfg!(debug_assertions) {
                DEV_PREFIX
            } else {
                PREFIX
            }
        );
        let dir = Path::new("/tmp");
        assert!(!is_log_file(&dir.join("robi-api-x.log"), Target::Tui));
        assert!(is_log_file(&dir.join("tui-x.log"), Target::Tui));
    }

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
        let current = dir.join(format!("{}current.log", log_prefix(Target::Api)));
        fs::write(&current, b"now").unwrap();

        // Newest prior file, and older than the age cap. It stays.
        let previous = dir.join(format!("{}previous.log", log_prefix(Target::Api)));
        touch(&previous, Duration::from_secs(1));

        let young: Vec<_> = (0..6)
            .map(|i| {
                let path = dir.join(format!("{}young-{i}.log", log_prefix(Target::Api)));
                touch(&path, Duration::from_secs(60 * (i as u64 + 2)));
                path
            })
            .collect();
        let excess = dir.join(format!("{}excess.log", log_prefix(Target::Api)));
        touch(&excess, Duration::from_secs(60 * 60));
        let ancient = dir.join(format!("{}ancient.log", log_prefix(Target::Api)));
        touch(&ancient, Duration::from_secs(10 * 24 * 60 * 60));
        fs::write(dir.join("notes.txt"), b"leave me").unwrap();
        let other_prefix = if cfg!(debug_assertions) {
            PREFIX
        } else {
            DEV_PREFIX
        };
        let other = dir.join(format!("{other_prefix}other.log"));
        touch(&other, Duration::from_secs(10 * 24 * 60 * 60));
        // Another process's log, aged past the cap. A TUI run leaves it alone.
        let tui = dir.join("tui-other.log");
        touch(&tui, Duration::from_secs(10 * 24 * 60 * 60));

        prune_logs(&dir, &current, Target::Api, now).unwrap();

        assert!(current.exists());
        assert!(previous.exists());
        for path in &young {
            assert!(path.exists(), "missing {}", path.display());
        }
        assert!(!excess.exists());
        assert!(!ancient.exists());
        assert!(dir.join("notes.txt").exists());
        assert!(other.exists());
        assert!(tui.exists());

        let only = std::env::temp_dir().join(format!("robi-logs-old-{}", std::process::id()));
        let _ = fs::remove_dir_all(&only);
        fs::create_dir_all(&only).unwrap();
        let current_only = only.join(format!("{}current.log", log_prefix(Target::Api)));
        fs::write(&current_only, b"now").unwrap();
        let old_previous = only.join(format!("{}old.log", log_prefix(Target::Api)));
        touch(&old_previous, Duration::from_secs(30 * 24 * 60 * 60));
        let also_old = only.join(format!("{}also-old.log", log_prefix(Target::Api)));
        touch(&also_old, Duration::from_secs(40 * 24 * 60 * 60));
        prune_logs(&only, &current_only, Target::Api, SystemTime::now()).unwrap();
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
