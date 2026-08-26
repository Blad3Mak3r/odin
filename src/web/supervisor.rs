//! Reaps a `Child` spawned by this exact `odin serve` boot, so it never
//! zombies while the daemon stays alive.
//!
//! This is deliberately *not* the source of truth for "is this instance
//! running" — that's always a live `(pid, pid_started_at)` check against
//! the OS process table (`instance::lifecycle::is_running`), persisted in
//! SQLite and therefore correct even before this process has reconciled
//! anything. `Supervisor` only ever adds the reaping behavior on top of
//! that, and only for instances spawned by this exact boot.
//!
//! Crucially, `Supervisor` never calls `Child::kill()` and never sets
//! `kill_on_drop(true)`. On a daemon restart, the reaper task and its
//! `Child` are simply abandoned along with the rest of the process's
//! state — the OS process reparents to PID 1 (which reaps it whenever it
//! eventually exits) and keeps running. That's the detail that makes an
//! instance survive `systemctl restart odin`.

use std::sync::Arc;

use crate::activity::ActivityLog;
use crate::db::Db;

#[derive(Clone, Default)]
pub struct Supervisor;

impl Supervisor {
    pub fn new() -> Self {
        Self
    }

    /// Takes ownership of a freshly spawned child: reaps it (`child.wait()`
    /// — the actual `waitpid(2)`, preventing a zombie for as long as this
    /// boot is alive) in a background task, then clears its pid on exit.
    /// Must be called only for a `Child` this exact boot spawned — never
    /// for one adopted during reconciliation, since there's no `Child`
    /// handle to reap in that case.
    pub fn spawn_reaper(
        &self,
        name: String,
        mut child: tokio::process::Child,
        db: Arc<Db>,
        activity: ActivityLog,
    ) {
        tokio::spawn(async move {
            let status = child.wait().await;
            tracing::info!(instance = %name, ?status, "instance process exited");
            let _ = crate::db::instances::clear_pid(&db, &name, chrono::Utc::now());
            activity.record(crate::activity::ActivityKind::InstanceStopped, Some(name));
        });
    }
}
