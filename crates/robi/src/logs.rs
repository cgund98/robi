//! API process logs under `~/.robi/logs/`.
//!
//! Each start opens a new `robi-api-<timestamp>.log` and never truncates an
//! existing file. The newest file from the previous start is kept even when it
//! is old or the directory is over the cap, so the run you just left is still
//! there after a reopen.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{SecondsFormat, Utc};
use tracing_subscriber::prelude::*;

/// Files kept after a start, including the new file. The previous run is kept
/// even when that puts the directory over this count.
const MAX_LOG_FILES: usize = 8;

/// Files older than this are deleted, except the previous run.
const MAX_LOG_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

const PREFIX: &str = "robi-api-";
const SUFFIX: &str = ".log";

/// Install the process-wide tracing subscriber.
///
/// Writes a new log file when `~/.robi` can be resolved. Debug builds also
/// write the same events to stderr. `RUST_LOG` selects verbosity, defaulting
/// to `robi=info`.
pub fn init() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "robi=info".into());

    let file_layer = match home_logs() {
        Ok(dir) => match open_log_file(&dir) {
            Ok((path, file)) => {
                if let Err(err) = prune_logs(&dir, &path, SystemTime::now()) {
                    eprintln!("could not prune old log files: {err}");
                }
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

fn home_logs() -> io::Result<PathBuf> {
    let home = crate::adapters::settings::home_dir()
        .map_err(|err| io::Error::new(ErrorKind::NotFound, err.to_string()))?;
    let dir = home.join("logs");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Create a new log file. A name collision gets a numeric suffix rather than
/// an append or a truncate.
fn open_log_file(dir: &Path) -> io::Result<(PathBuf, File)> {
    let stamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let stamp = stamp.replace(':', "-");
    let mut n = 1u32;
    loop {
        let name = if n == 1 {
            format!("{PREFIX}{stamp}{SUFFIX}")
        } else {
            format!("{PREFIX}{stamp}-{n}{SUFFIX}")
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
    let mut older = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path == current || !is_log_file(&path) {
            continue;
        }
        let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
        older.push((modified, path));
    }
    older.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut kept = 1; // the file this start just created
    for (index, (modified, path)) in older.iter().enumerate() {
        if index == 0 {
            kept += 1;
            continue;
        }
        let aged = modified
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > MAX_LOG_AGE);
        if aged || kept >= MAX_LOG_FILES {
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
    name.starts_with(PREFIX) && name.ends_with(SUFFIX)
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
    fn keeps_the_previous_run_and_drops_old_or_excess_files() {
        let dir = std::env::temp_dir().join(format!("robi-logs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let now = SystemTime::now();
        let current = dir.join("robi-api-current.log");
        fs::write(&current, b"now").unwrap();

        // Newest prior file, and older than the age cap. It stays.
        let previous = dir.join("robi-api-previous.log");
        touch(&previous, Duration::from_secs(1));

        let young: Vec<_> = (0..6)
            .map(|i| {
                let path = dir.join(format!("robi-api-young-{i}.log"));
                touch(&path, Duration::from_secs(60 * (i as u64 + 2)));
                path
            })
            .collect();
        let excess = dir.join("robi-api-excess.log");
        touch(&excess, Duration::from_secs(60 * 60));
        let ancient = dir.join("robi-api-ancient.log");
        touch(&ancient, Duration::from_secs(10 * 24 * 60 * 60));
        fs::write(dir.join("notes.txt"), b"leave me").unwrap();

        prune_logs(&dir, &current, now).unwrap();

        assert!(current.exists());
        assert!(previous.exists());
        for path in &young {
            assert!(path.exists(), "missing {}", path.display());
        }
        assert!(!excess.exists());
        assert!(!ancient.exists());
        assert!(dir.join("notes.txt").exists());

        let only = std::env::temp_dir().join(format!("robi-logs-old-{}", std::process::id()));
        let _ = fs::remove_dir_all(&only);
        fs::create_dir_all(&only).unwrap();
        let current_only = only.join("robi-api-current.log");
        fs::write(&current_only, b"now").unwrap();
        let old_previous = only.join("robi-api-old.log");
        touch(&old_previous, Duration::from_secs(30 * 24 * 60 * 60));
        let also_old = only.join("robi-api-also-old.log");
        touch(&also_old, Duration::from_secs(40 * 24 * 60 * 60));
        prune_logs(&only, &current_only, SystemTime::now()).unwrap();
        assert!(old_previous.exists());
        assert!(!also_old.exists());
        let _ = fs::remove_dir_all(&only);

        let _ = fs::remove_dir_all(&dir);
    }
}
