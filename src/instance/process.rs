//! Spawns and supervises the `valheim_server.x86_64` OS process directly —
//! no tmux, no shell script. Replaces `crate::tmux`.
//!
//! Console input goes through a per-instance named pipe
//! (`paths::instance_console_fifo`) rather than an in-memory pipe: the
//! server process holds its read end open for its whole life, and any
//! process — including a fresh `odin serve` after a restart, or a
//! standalone CLI invocation — can reopen the write end by path, so
//! sending console commands doesn't depend on which process originally
//! spawned the server. See `crate::web::supervisor` for how `odin serve`
//! keeps that write end open across ticks.
//!
//! Liveness and signalling both go through `sysinfo`, keyed by
//! `(pid, pid_started_at)`: `pid_started_at` is the process's own kernel
//! start time, recorded once right after spawning, and re-checked on every
//! lookup so a reused pid (e.g. after a host reboot) reads as "not
//! running" instead of a false positive.

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use sysinfo::{Pid, ProcessesToUpdate, Signal, System};
use tokio::process::{Child, Command};

use super::Instance;
use crate::paths::{self, Paths};

/// Creates `path` as a FIFO if it doesn't already exist. Idempotent across
/// stop/start cycles of the same instance — the same pipe is reused, not
/// recreated, each time the instance starts.
///
/// SAFETY: `path` is converted to a valid, nul-terminated `CString` before
/// the call; only the integer return value is inspected afterwards.
pub fn ensure_console_fifo(path: &Path) -> Result<()> {
    let c_path = CString::new(path.as_os_str().as_bytes())
        .with_context(|| format!("instance path {} contains a nul byte", path.display()))?;
    let ret = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
    if ret != 0 {
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::AlreadyExists {
            return Err(err).with_context(|| format!("failed to create fifo {}", path.display()));
        }
    }
    Ok(())
}

/// Opens the FIFO's read end to hand to the child as its stdin. Opened
/// read+write (not read-only) specifically so this `open` never blocks: a
/// read-only open on a FIFO blocks until some writer opens it, and nobody
/// has opened the write end yet at this point — `odin serve` opens its own
/// writer immediately after spawning (see `web::supervisor`).
fn open_fifo_for_child_stdin(path: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("failed to open {} for the child's stdin", path.display()))
}

/// Opens (or reopens, after an `odin serve` restart) the persistent
/// write end used to send console commands. `O_NONBLOCK` makes this fail
/// fast with `ENXIO` if the reader is somehow already gone (a narrow race
/// between a liveness check and this call) rather than blocking a
/// reconciliation tick indefinitely.
pub async fn open_console_writer(path: &Path) -> Result<tokio::fs::File> {
    tokio::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .await
        .with_context(|| format!("failed to open console fifo {} for writing", path.display()))
}

/// Prepends `prefix` to whatever `var` odin's own process inherited,
/// mirroring how the old `run.sh` extended `LD_LIBRARY_PATH`/`LD_PRELOAD`
/// (`"prefix:${VAR:-}"`) rather than clobbering an operator's existing
/// value outright.
fn colon_prepend(var: &str, prefix: &str) -> String {
    match std::env::var(var) {
        Ok(existing) if !existing.is_empty() => format!("{prefix}:{existing}"),
        _ => prefix.to_string(),
    }
}

/// Builds the ready-to-spawn command for an instance's server process:
/// working directory, env vars (including BepInEx/Doorstop when
/// installed), stdio (stdin from the console FIFO, stdout/stderr appended
/// to `console.log`), and args — all set directly on the `Command`
/// builder, no intermediate shell script. Passing args as native argv
/// (instead of interpolating them into a shell command string, as the old
/// `run.sh` did) also removes the shell-injection risk class that existed
/// there, not just the shell process itself.
pub fn build_command(instance: &Instance, paths: &Paths) -> Result<Command> {
    let install_dir = paths.shared_install_dir();

    let fifo = paths::instance_console_fifo(&instance.dir);
    ensure_console_fifo(&fifo)?;
    let stdin = open_fifo_for_child_stdin(&fifo)?;

    let console_log = paths::instance_logs_dir(&instance.dir).join("console.log");
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&console_log)
        .with_context(|| format!("failed to open {}", console_log.display()))?;
    let stderr = stdout
        .try_clone()
        .context("failed to duplicate console.log handle for stderr")?;

    let mut cmd = Command::new(install_dir.join("valheim_server.x86_64"));
    cmd.current_dir(&install_dir)
        .env(
            "LD_LIBRARY_PATH",
            colon_prepend(
                "LD_LIBRARY_PATH",
                &install_dir.join("linux64").to_string_lossy(),
            ),
        )
        .env("SteamAppId", "892970")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        // Own process group: a Ctrl-C delivered to odin's own controlling
        // terminal (relevant only when running interactively in dev, not
        // under systemd) doesn't also land on the game server.
        .process_group(0);

    if instance.state.bepinex_installed {
        // Matches BepInExPack Valheim's own `start_server_bepinex.sh`
        // (Doorstop 4.x env var names), with absolute paths since the
        // process's cwd is the shared install dir, not this instance dir
        // where BepInEx/doorstop_libs actually live.
        let bepinex_dir = paths::instance_bepinex_dir(&instance.dir);
        let doorstop_libs_dir = instance.dir.join("doorstop_libs");
        cmd.env("DOORSTOP_ENABLED", "1")
            .env(
                "DOORSTOP_TARGET_ASSEMBLY",
                bepinex_dir.join("core/BepInEx.Preloader.dll"),
            )
            .env(
                "LD_LIBRARY_PATH",
                colon_prepend(
                    "LD_LIBRARY_PATH",
                    &format!(
                        "{}:{}",
                        doorstop_libs_dir.display(),
                        install_dir.join("linux64").display()
                    ),
                ),
            )
            .env(
                "LD_PRELOAD",
                colon_prepend("LD_PRELOAD", "libdoorstop_x64.so"),
            );
    }

    cmd.arg("-nographics")
        .arg("-batchmode")
        .arg("-name")
        .arg(&instance.state.name)
        .arg("-port")
        .arg(instance.state.port.to_string())
        .arg("-world")
        .arg(&instance.state.world_name)
        .arg("-savedir")
        .arg(paths::instance_saves_dir(&instance.dir))
        .arg("-public")
        .arg(if instance.state.public { "1" } else { "0" });
    if let Some(password) = &instance.state.password {
        cmd.arg("-password").arg(password);
    }

    tracing::info!(
        instance = %instance.state.name,
        port = instance.state.port,
        bepinex = instance.state.bepinex_installed,
        "spawning valheim_server.x86_64"
    );

    Ok(cmd)
}

/// Spawns `cmd`, detached at the OS level from the moment it starts:
/// `kill_on_drop` is left at its tokio default of `false`, so if `odin
/// serve` exits or is restarted while holding this `Child`, dropping it
/// does NOT kill the process — it simply reparents to PID 1 (which reaps
/// it on exit) and keeps running with its stdout/stderr/console-log
/// redirections intact at the kernel level. This is the detail that makes
/// instances survive `systemctl restart odin`; never call `.kill()` on a
/// `Child` obtained this way except as part of an explicit `stop()`.
pub async fn spawn(mut cmd: Command) -> Result<Child> {
    cmd.spawn().context("failed to spawn valheim_server.x86_64")
}

/// The process's own kernel start time — the liveness fingerprint for a
/// pid. Must be read right after spawning, before the pid could plausibly
/// have been reused by an unrelated process.
pub fn start_time_of(pid: u32) -> Result<i64> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    system
        .process(Pid::from_u32(pid))
        .map(|p| p.start_time() as i64)
        .with_context(|| {
            format!("pid {pid} not found in the process table right after spawning it")
        })
}

/// The single canonical liveness check: a targeted sysinfo refresh for
/// just this one pid, compared against the fingerprint recorded when it
/// was spawned.
pub fn is_alive(pid: u32, pid_started_at: i64) -> bool {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    system
        .process(Pid::from_u32(pid))
        .is_some_and(|p| p.start_time() as i64 == pid_started_at)
}

/// Sends `signal` to `pid`, first re-validating the fingerprint so a
/// reused pid never gets signalled by mistake. `Ok(false)` means the pid
/// is already gone or no longer matches — the caller treats that as
/// "already stopped", not an error.
pub fn send_signal(pid: u32, pid_started_at: i64, signal: Signal) -> Result<bool> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    let Some(process) = system.process(Pid::from_u32(pid)) else {
        return Ok(false);
    };
    if process.start_time() as i64 != pid_started_at {
        return Ok(false);
    }
    Ok(process.kill_with(signal).unwrap_or(false))
}

/// Polls `is_alive` until it's false or `timeout` elapses. Returns true if
/// the process exited within the timeout. Async replacement for
/// `tmux::wait_for_session_end`.
pub async fn wait_until_gone(pid: u32, pid_started_at: i64, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if !is_alive(pid, pid_started_at) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "odin-process-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn ensure_console_fifo_is_idempotent_and_creates_a_real_fifo() {
        let path = temp_path("fifo");
        ensure_console_fifo(&path).unwrap();
        ensure_console_fifo(&path).unwrap(); // second call: EEXIST, swallowed

        let file_type = std::fs::symlink_metadata(&path).unwrap().file_type();
        assert!(std::os::unix::fs::FileTypeExt::is_fifo(&file_type));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn is_alive_reflects_a_real_child_process_across_its_lifetime() {
        let mut child = std::process::Command::new("sleep")
            .arg("2")
            .spawn()
            .unwrap();
        let pid = child.id();
        let started_at = start_time_of(pid).unwrap();

        assert!(is_alive(pid, started_at));
        assert!(!is_alive(pid, started_at + 1)); // wrong fingerprint reads as not-alive

        child.kill().unwrap();
        child.wait().unwrap();

        // Reaped: pid is gone from the process table regardless of fingerprint.
        assert!(!is_alive(pid, started_at));
    }

    #[tokio::test]
    async fn wait_until_gone_returns_true_once_the_process_exits() {
        let mut child = std::process::Command::new("sleep")
            .arg("0.2")
            .spawn()
            .unwrap();
        let pid = child.id();
        let started_at = start_time_of(pid).unwrap();

        // Reap concurrently, the way `Supervisor::spawn_reaper` does in
        // production — an exited-but-unreaped process is a zombie, which
        // still shows up in the process table (so `is_alive` would never
        // flip to false) until something calls `wait()` on it.
        let reaper = tokio::task::spawn_blocking(move || child.wait());

        assert!(wait_until_gone(pid, started_at, Duration::from_secs(5)).await);
        reaper.await.unwrap().ok();
    }

    #[tokio::test]
    async fn console_writer_can_be_opened_after_child_stdin_is_attached() {
        let path = temp_path("writer");
        ensure_console_fifo(&path).unwrap();
        let _child_stdin = open_fifo_for_child_stdin(&path).unwrap(); // simulates the spawned child holding the read end

        let mut writer = open_console_writer(&path).await.unwrap();
        use tokio::io::AsyncWriteExt as _;
        writer.write_all(b"save\n").await.unwrap();

        std::fs::remove_file(&path).ok();
    }
}
