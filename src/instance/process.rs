//! Game-neutral process liveness and signalling.

use anyhow::{Context, Result};
use sysinfo::{Pid, ProcessesToUpdate, Signal, System};

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
