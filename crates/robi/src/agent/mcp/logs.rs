//! Per-server MCP logs under `~/.robi/logs/mcp/<server_id>/`.
//!
//! One file per connection, named `mcp-<timestamp>.log`. Each line records a
//! JSON-RPC message in either direction, one line of the child's stderr, or a
//! lifecycle note. Secret header and env values never reach this file. The
//! connection summary names the target and whether an authorization header was
//! present, never its value.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use chrono::{SecondsFormat, Utc};
use rmcp::service::{RoleClient, RxJsonRpcMessage, TxJsonRpcMessage};

/// One logged line may not exceed this many bytes. A longer protocol message is
/// cut, with a note, so a single huge result cannot dominate the file.
const MAX_LINE_BYTES: usize = 32 * 1024;

const SUFFIX: &str = ".log";

struct Inner {
    writer: Mutex<Box<dyn Write + Send>>,
    /// Flushes the background writer on drop. `None` for the in-test writer.
    _guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// One server's log. Cheap to clone; writes go through one background thread.
#[derive(Clone)]
pub struct ServerLog {
    inner: Option<Arc<Inner>>,
}

impl ServerLog {
    /// Open `~/.robi/logs/mcp/<server_id>/mcp-<timestamp>.log`.
    ///
    /// Prunes that directory first. A failure to open yields a disabled log, so
    /// logging never stops a server from starting.
    pub fn open(home: &Path, server_id: &str) -> ServerLog {
        match open_inner(home, server_id) {
            Ok(inner) => ServerLog {
                inner: Some(Arc::new(inner)),
            },
            Err(err) => {
                tracing::warn!(server = %server_id, %err, "could not open the mcp log file");
                ServerLog::disabled()
            }
        }
    }

    /// A log that writes nothing.
    pub fn disabled() -> ServerLog {
        ServerLog { inner: None }
    }

    /// Write one `<timestamp> LEVEL message` line.
    pub fn line(&self, level: &str, message: &str) {
        self.write(&format!("{level} {}", bound(message)));
    }

    /// A message this process is sending to the server.
    pub fn protocol_out(&self, message: &TxJsonRpcMessage<RoleClient>) {
        if self.inner.is_some() {
            let text =
                serde_json::to_string(message).unwrap_or_else(|_| "<unserializable>".to_owned());
            self.write(&format!("-> {}", bound(&text)));
        }
    }

    /// A message this process received from the server.
    pub fn protocol_in(&self, message: &RxJsonRpcMessage<RoleClient>) {
        if self.inner.is_some() {
            let text =
                serde_json::to_string(message).unwrap_or_else(|_| "<unserializable>".to_owned());
            self.write(&format!("<- {}", bound(&text)));
        }
    }

    /// One line of the child's standard error.
    pub fn stderr(&self, command: &str, line: &str) {
        self.write(&format!("STDERR {command} {}", bound(line)));
    }

    fn write(&self, text: &str) {
        let Some(inner) = &self.inner else {
            return;
        };
        let stamp = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
        if let Ok(mut writer) = inner.writer.lock() {
            let _ = writeln!(writer, "{stamp} {text}");
        }
    }
}

fn open_inner(home: &Path, server_id: &str) -> std::io::Result<Inner> {
    let dir = crate::logs::mcp_logs_dir(home).join(server_id);
    fs::create_dir_all(&dir)?;
    let (path, file) = open_file(&dir)?;
    let _ = crate::logs::prune_mcp_dir(&dir, Some(&path), SystemTime::now());
    let (writer, guard) = tracing_appender::non_blocking(file);
    Ok(Inner {
        writer: Mutex::new(Box::new(writer)),
        _guard: Some(guard),
    })
}

/// A log that writes straight to `file`, for tests. No background thread and no
/// guard, so a test can read the file back immediately.
#[cfg(test)]
impl ServerLog {
    pub(crate) fn to_file(file: fs::File) -> ServerLog {
        ServerLog {
            inner: Some(Arc::new(Inner {
                writer: Mutex::new(Box::new(file)),
                _guard: None,
            })),
        }
    }
}

/// Create a new `mcp-<timestamp>.log`. A name collision gets a numeric suffix
/// rather than an append or a truncate. The file is mode `0600`: it can carry
/// tool arguments and results.
fn open_file(dir: &Path) -> std::io::Result<(PathBuf, fs::File)> {
    let stamp = Utc::now()
        .to_rfc3339_opts(SecondsFormat::Secs, true)
        .replace(':', "-");
    let mut n = 1u32;
    loop {
        let name = if n == 1 {
            format!("mcp-{stamp}{SUFFIX}")
        } else {
            format!("mcp-{stamp}-{n}{SUFFIX}")
        };
        let path = dir.join(name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
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

/// Prune `~/.robi/logs/mcp`: prune each server directory, then drop it when it
/// is empty. Called on startup so a server removed from the config still has
/// its old files expire. A directory that cannot be read is skipped.
pub(crate) fn prune_orphans(home: &Path) {
    let dir = crate::logs::mcp_logs_dir(home);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if crate::logs::prune_mcp_dir(&path, None, SystemTime::now()).is_ok() {
            // Succeeds only when nothing is left.
            let _ = fs::remove_dir(&path);
        }
    }
}

/// Cut `text` at the line bound, on a character boundary.
fn bound(text: &str) -> String {
    if text.len() <= MAX_LINE_BYTES {
        return text.to_owned();
    }
    let mut end = MAX_LINE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} … [{} bytes truncated]", &text[..end], text.len() - end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::time::Duration;

    use rmcp::model::{ClientRequest, JsonRpcMessage, PingRequest, RequestId, ServerRequest};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("robi-mcp-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path, age: Duration) {
        fs::write(path, b"log").unwrap();
        let modified = SystemTime::now() - age;
        let file = File::options().write(true).open(path).unwrap();
        file.set_modified(modified).unwrap();
    }

    #[test]
    fn lines_and_stderr_reach_the_file() {
        let home = temp("lines");
        let path = home.join("mcp-now.log");
        let log = ServerLog::to_file(File::create(&path).unwrap());

        log.line("INFO", "started");
        log.stderr("uvx", "server is not installed");

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("INFO started"), "{text}");
        assert!(
            text.contains("STDERR uvx server is not installed"),
            "{text}"
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn protocol_messages_are_written_in_both_directions() {
        let home = temp("protocol");
        let path = home.join("mcp-now.log");
        let log = ServerLog::to_file(File::create(&path).unwrap());

        let outbound: TxJsonRpcMessage<RoleClient> = JsonRpcMessage::request(
            ClientRequest::PingRequest(PingRequest::default()),
            RequestId::Number(1),
        );
        log.protocol_out(&outbound);
        let inbound: RxJsonRpcMessage<RoleClient> = JsonRpcMessage::request(
            ServerRequest::PingRequest(PingRequest::default()),
            RequestId::Number(2),
        );
        log.protocol_in(&inbound);

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"ping\""), "{text}");
        assert!(text.contains("-> "), "{text}");
        assert!(text.contains("<- "), "{text}");
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn opening_creates_the_server_directory_and_a_log_file() {
        let home = temp("open");
        let log = ServerLog::open(&home, "github");
        assert!(log.inner.is_some());

        let dir = crate::logs::mcp_logs_dir(&home).join("github");
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 1);
        assert!(names[0].starts_with("mcp-") && names[0].ends_with(".log"));

        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_disabled_log_ignores_writes() {
        let log = ServerLog::disabled();
        // None of these may panic or touch the filesystem.
        log.line("INFO", "nothing happens");
        log.stderr("uvx", "no server");
        assert!(log.inner.is_none());
    }

    #[test]
    fn the_orphan_sweep_prunes_each_server_and_drops_empty_directories() {
        let home = temp("orphan");
        let mcp = crate::logs::mcp_logs_dir(&home);

        // A live server directory: the young file stays, the ancient file goes.
        let kept_dir = mcp.join("github");
        fs::create_dir_all(&kept_dir).unwrap();
        let newest = kept_dir.join("mcp-new.log");
        touch(&newest, Duration::from_secs(60));
        let ancient = kept_dir.join("mcp-ancient.log");
        touch(&ancient, Duration::from_secs(30 * 24 * 60 * 60));
        // A file that is not an mcp log stays.
        let notes = kept_dir.join("notes.txt");
        fs::write(&notes, b"leave me").unwrap();

        // A removed server: everything is old, so the directory goes.
        let stale_dir = mcp.join("gone");
        fs::create_dir_all(&stale_dir).unwrap();
        let stale = stale_dir.join("mcp-old.log");
        touch(&stale, Duration::from_secs(40 * 24 * 60 * 60));

        prune_orphans(&home);

        assert!(newest.exists());
        assert!(!ancient.exists());
        assert!(notes.exists());
        assert!(!stale_dir.exists());
        assert!(kept_dir.exists());

        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_long_line_is_cut_at_the_bound() {
        let long = "x".repeat(MAX_LINE_BYTES + 100);
        let cut = bound(&long);
        assert!(cut.len() < long.len());
        assert!(cut.contains("bytes truncated"));
        assert_eq!(bound("short"), "short");
    }
}
