use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TmuxError {
    #[error(
        "tmux is not installed or not on PATH; install it via your package manager (e.g. `apt install tmux`)"
    )]
    BinaryNotFound,
    #[error("tmux command failed: {0}")]
    CommandFailed(String),
}

pub fn has_binary() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn require_binary() -> Result<()> {
    if !has_binary() {
        anyhow::bail!(TmuxError::BinaryNotFound);
    }
    Ok(())
}

pub fn has_session(session: &str) -> Result<bool> {
    require_binary()?;
    let status = Command::new("tmux")
        .args(["has-session", "-t", session])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("failed to invoke tmux has-session")?;
    Ok(status.success())
}

pub fn new_detached_session(session: &str, cwd: &Path, shell_cmd: &str) -> Result<()> {
    require_binary()?;
    let status = Command::new("tmux")
        .args(["new-session", "-d", "-s", session, "-c"])
        .arg(cwd)
        .arg(shell_cmd)
        .status()
        .context("failed to invoke tmux new-session")?;
    if !status.success() {
        anyhow::bail!(TmuxError::CommandFailed(format!(
            "tmux new-session -s {session} exited with {status}"
        )));
    }
    Ok(())
}

pub fn pipe_pane_to_file(session: &str, log_file: &Path) -> Result<()> {
    require_binary()?;
    let log_file_str = log_file.to_string_lossy();
    let status = Command::new("tmux")
        .args(["pipe-pane", "-o", "-t", session])
        .arg(format!("cat >> {log_file_str}"))
        .status()
        .context("failed to invoke tmux pipe-pane")?;
    if !status.success() {
        anyhow::bail!(TmuxError::CommandFailed(format!(
            "tmux pipe-pane -t {session} exited with {status}"
        )));
    }
    Ok(())
}

pub fn send_ctrl_c(session: &str) -> Result<()> {
    require_binary()?;
    let status = Command::new("tmux")
        .args(["send-keys", "-t", session, "C-c"])
        .status()
        .context("failed to invoke tmux send-keys")?;
    if !status.success() {
        anyhow::bail!(TmuxError::CommandFailed(format!(
            "tmux send-keys -t {session} exited with {status}"
        )));
    }
    Ok(())
}

/// Sends literal text followed by Enter, as if typed into the session's console.
pub fn send_keys_line(session: &str, text: &str) -> Result<()> {
    require_binary()?;
    let status = Command::new("tmux")
        .args(["send-keys", "-t", session, text, "Enter"])
        .status()
        .context("failed to invoke tmux send-keys")?;
    if !status.success() {
        anyhow::bail!(TmuxError::CommandFailed(format!(
            "tmux send-keys -t {session} exited with {status}"
        )));
    }
    Ok(())
}

pub fn kill_session(session: &str) -> Result<()> {
    require_binary()?;
    let status = Command::new("tmux")
        .args(["kill-session", "-t", session])
        .status()
        .context("failed to invoke tmux kill-session")?;
    if !status.success() {
        anyhow::bail!(TmuxError::CommandFailed(format!(
            "tmux kill-session -t {session} exited with {status}"
        )));
    }
    Ok(())
}

/// Polls until the session disappears or the timeout elapses. Returns true if
/// the session ended within the timeout.
pub fn wait_for_session_end(session: &str, timeout: Duration) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        if !has_session(session)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Replaces the current process with `tmux attach -t <session>` so terminal
/// control (Ctrl-b d to detach, etc.) behaves exactly like a normal tmux attach.
#[cfg(unix)]
pub fn attach(session: &str) -> Result<()> {
    use std::os::unix::process::CommandExt;
    require_binary()?;
    let err = Command::new("tmux").args(["attach", "-t", session]).exec();
    Err(err).context("failed to exec tmux attach")
}
