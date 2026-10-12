//! Resource sampling shared by the runtime that owns a container's game
//! process.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use sysinfo::{Pid, System};

use crate::instance::process;

pub(crate) const STATS_REFRESH_INTERVAL: Duration = Duration::from_secs(3);

/// `None` until the first background refresh completes, then the latest
/// `(cpu_percent, memory_bytes)` snapshot.
pub(crate) type StatsHandle = Arc<Mutex<Option<(f32, u64)>>>;

/// Refreshes `stats` on `STATS_REFRESH_INTERVAL`: sums `memory()`/
/// `cpu_usage()` over the game child and its true (non-thread) descendants.
/// Runs forever; the caller aborts it once the child exits.
pub(crate) async fn refresh_stats_periodically(pid: u32, stats: StatsHandle) {
    let mut system = System::new_all();
    loop {
        system = tokio::task::spawn_blocking(move || {
            system.refresh_all();
            system
        })
        .await
        .unwrap_or_else(|_| System::new_all());

        let mut cpu_percent = 0.0;
        let mut memory_bytes = 0;
        for descendant in process::descendant_pids(&system, &[pid]) {
            if let Some(p) = system.process(Pid::from_u32(descendant)) {
                cpu_percent += p.cpu_usage();
                memory_bytes += p.memory();
            }
        }
        *stats.lock().expect("stats lock poisoned") = Some((cpu_percent, memory_bytes));

        tokio::time::sleep(STATS_REFRESH_INTERVAL).await;
    }
}
