//! Tracks the console-command write end for every instance this `odin
//! serve` process currently knows how to talk to, plus reaping for the
//! ones it spawned itself this boot.
//!
//! This is deliberately *not* the source of truth for "is this instance
//! running" — that's always a live `(pid, pid_started_at)` check against
//! the OS process table (`instance::lifecycle::is_running`), persisted in
//! SQLite and therefore correct even before this process has reconciled
//! anything. `Supervisor` only ever adds capability on top of that: a
//! writable handle to the instance's console FIFO, kept open for as long
//! as this boot has one, and (only for instances spawned by this exact
//! boot) a background task reaping the `Child` so it never zombies while
//! `odin serve` is alive.
//!
//! Crucially, `Supervisor` never calls `Child::kill()` and never sets
//! `kill_on_drop(true)`. On a daemon restart, the reaper task and its
//! `Child` are simply abandoned along with the rest of the process's
//! state — the OS process reparents to PID 1 (which reaps it whenever it
//! eventually exits) and keeps running, console FIFO and all. That's the
//! detail that makes an instance survive `systemctl restart odin`; the
//! reconciliation loop in `web::mod` then reopens a fresh writer for it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use thiserror::Error;
use tokio::io::AsyncWriteExt as _;
use tokio::sync::Mutex as TokioMutex;

use crate::activity::ActivityLog;
use crate::db::Db;

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("instance '{0}' has no console writer registered on this boot yet")]
    NotSupervised(String),
    #[error("failed to write to console: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone)]
pub struct Supervisor {
    writers: Arc<StdMutex<HashMap<String, Arc<TokioMutex<tokio::fs::File>>>>>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self {
            writers: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    pub fn has_writer(&self, name: &str) -> bool {
        self.writers
            .lock()
            .expect("supervisor lock poisoned")
            .contains_key(name)
    }

    pub fn register_writer(&self, name: &str, file: tokio::fs::File) {
        self.writers
            .lock()
            .expect("supervisor lock poisoned")
            .insert(name.to_string(), Arc::new(TokioMutex::new(file)));
    }

    /// Removes and closes the writer for `name`, if any — called on an
    /// explicit `stop()` and whenever reconciliation finds the pid no
    /// longer alive.
    pub fn forget(&self, name: &str) {
        self.writers
            .lock()
            .expect("supervisor lock poisoned")
            .remove(name);
    }

    pub async fn send_command(&self, name: &str, line: &str) -> Result<(), SupervisorError> {
        let handle = {
            self.writers
                .lock()
                .expect("supervisor lock poisoned")
                .get(name)
                .cloned()
        };
        let Some(handle) = handle else {
            return Err(SupervisorError::NotSupervised(name.to_string()));
        };
        let mut file = handle.lock().await;
        file.write_all(format!("{line}\n").as_bytes()).await?;
        Ok(())
    }

    /// Takes ownership of a freshly spawned child: reaps it (`child.wait()`
    /// — the actual `waitpid(2)`, preventing a zombie for as long as this
    /// boot is alive) in a background task, then clears its pid/writer on
    /// exit. Must be called only for a `Child` this exact boot spawned —
    /// never for one adopted during reconciliation, since there's no
    /// `Child` handle to reap in that case.
    pub fn spawn_reaper(
        &self,
        name: String,
        mut child: tokio::process::Child,
        db: Arc<Db>,
        activity: ActivityLog,
    ) {
        let supervisor = self.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            tracing::info!(instance = %name, ?status, "instance process exited");
            let _ = crate::db::instances::clear_pid(&db, &name, chrono::Utc::now());
            supervisor.forget(&name);
            activity.record(crate::activity::ActivityKind::InstanceStopped, Some(name));
        });
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}
