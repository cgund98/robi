//! Run a command in its own process group, with a timeout and an output cap.

use std::path::PathBuf;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::CommandOutput;

pub struct Request {
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
    pub output_limit: usize,
    pub cancel: CancellationToken,
}

pub async fn run_command(request: Request) -> Result<CommandOutput, String> {
    let Some((program, args)) = request.argv.split_first() else {
        return Err("command is required".to_owned());
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(&request.cwd)
        .env_clear()
        .envs(
            request
                .env
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            format!("sandbox could not be applied: {program} was not found")
        } else {
            format!("sandbox could not be applied: {err}")
        }
    })?;
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let limit = request.output_limit / 2;
    let stdout_task = tokio::spawn(read_capped(stdout, limit));
    let stderr_task = tokio::spawn(read_capped(stderr, limit));

    let status = tokio::select! {
        _ = request.cancel.cancelled() => {
            kill_group(pid);
            let _ = child.wait().await;
            None
        }
        _ = tokio::time::sleep(request.timeout) => {
            kill_group(pid);
            let _ = child.wait().await;
            None
        }
        status = child.wait() => status.ok(),
    };
    let (stdout, stdout_truncated) = stdout_task.await.unwrap_or_default();
    let (mut stderr, stderr_truncated) = stderr_task.await.unwrap_or_default();
    let killed = status.is_none();
    if killed {
        if !stderr.is_empty() && !stderr.ends_with('\n') {
            stderr.push('\n');
        }
        stderr.push_str("command was killed");
    }
    if stderr.starts_with("sandbox-exec:") || stderr.starts_with("bwrap:") {
        return Err(format!("sandbox could not be applied: {stderr}"));
    }
    Ok(CommandOutput {
        exit_code: status.and_then(|status| status.code()).unwrap_or(-1),
        stdout,
        stderr,
        truncated: stdout_truncated || stderr_truncated,
        killed,
    })
}

async fn read_capped(reader: Option<impl AsyncRead + Unpin>, limit: usize) -> (String, bool) {
    let Some(mut reader) = reader else {
        return (String::new(), false);
    };
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut tmp).await.unwrap_or(0);
        if read == 0 {
            break;
        }
        if buf.len() >= limit {
            truncated = true;
            continue;
        }
        let room = limit - buf.len();
        let take = read.min(room);
        buf.extend_from_slice(&tmp[..take]);
        if take < read {
            truncated = true;
        }
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    (text, truncated)
}

fn kill_group(pid: Option<u32>) {
    let Some(pid) = pid else {
        return;
    };
    #[cfg(unix)]
    unsafe {
        kill_process(-(pid as i32), 9);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "kill"]
    fn kill_process(pid: i32, sig: i32) -> i32;
}
