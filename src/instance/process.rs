//! Game-neutral process liveness and signalling.

use std::collections::HashMap;

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

/// Given a live, freshly-refreshed `system` and one or more root pids,
/// returns every real (non-thread) process reachable by walking
/// `Process::parent()` from those roots.
///
/// sysinfo inserts one `Process` entry *per thread* on Linux, not just one
/// per real process, and a thread entry's `parent()` is its own owning
/// process (the thread-group leader), not its true OS parent. Walked
/// naively, every thread looks like a child of its own process, and
/// `Process::memory()` on a thread entry reports the whole shared
/// process's RSS again — so an unfiltered walk counts a process's memory
/// once per thread it has (a heavily-threaded Valheim server previously
/// inflated reported memory by ~59x this way). `thread_kind()` is `None`
/// for a real process and `Some(_)` for a thread pseudo-entry; skipping
/// `Some(_)` candidates fixes this while still finding genuine child
/// *processes*, if the game ever spawns any.
pub fn descendant_pids(system: &System, roots: &[u32]) -> Vec<u32> {
    let mut children_by_parent: HashMap<u32, Vec<u32>> = HashMap::new();
    for (candidate_pid, process) in system.processes() {
        if process.thread_kind().is_some() {
            continue;
        }
        if let Some(parent) = process.parent() {
            children_by_parent
                .entry(parent.as_u32())
                .or_default()
                .push(candidate_pid.as_u32());
        }
    }

    let mut result: Vec<u32> = roots.to_vec();
    let mut frontier: Vec<u32> = roots.to_vec();
    while let Some(pid) = frontier.pop() {
        for &candidate in children_by_parent.get(&pid).into_iter().flatten() {
            if !result.contains(&candidate) {
                result.push(candidate);
                frontier.push(candidate);
            }
        }
    }
    result
}
