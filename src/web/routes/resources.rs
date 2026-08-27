use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use sysinfo::{Disks, Pid, System};

use crate::instance::{Instance, lifecycle};
use crate::web::error::{ApiResult, run_blocking};
use crate::web::runtime::{HostSnapshot, InstanceSnapshot, ResourceSample};
use crate::web::state::AppState;

/// Kept short deliberately: this runs inside the telemetry tick's
/// `spawn_blocking` context, once per instance per tick — a wedged
/// supervisor should read as "unreachable" quickly, not tie up a blocking
/// thread.
const SUPERVISOR_PING_TIMEOUT: Duration = Duration::from_millis(300);

/// Reads whatever `spawn_telemetry`'s background tick last cached — cheap,
/// no `sysinfo`/`tmux` work on the request path.
pub async fn get_host_resources(State(state): State<AppState>) -> Json<HostSnapshot> {
    Json(state.runtime.host_snapshot())
}

pub async fn get_host_resources_history(
    State(state): State<AppState>,
) -> Json<Vec<ResourceSample>> {
    Json(state.runtime.host_history())
}

pub async fn get_instance_resources(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<InstanceSnapshot>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let load_name = name.clone();
    run_blocking(move || Instance::load_existing(&paths, &db, &load_name)).await?;
    Ok(Json(state.runtime.instance_snapshot(&name)))
}

pub async fn get_instance_resources_history(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Vec<ResourceSample>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let load_name = name.clone();
    run_blocking(move || Instance::load_existing(&paths, &db, &load_name)).await?;
    Ok(Json(state.runtime.instance_history(&name)))
}

/// Computed by `spawn_telemetry` on its background tick — refreshes
/// `state.resources` and the host disk usage, without touching any
/// per-instance/tmux state.
pub(crate) fn compute_host_snapshot(state: &AppState) -> HostSnapshot {
    let (cpu_percent, memory_total_bytes, memory_used_bytes) = {
        let system = state.resources.lock().expect("resources lock poisoned");
        (
            system.global_cpu_usage(),
            system.total_memory(),
            system.used_memory(),
        )
    };
    let (disk_total_bytes, disk_available_bytes) = disk_usage_for(&state.paths.data_dir);
    HostSnapshot {
        cpu_percent,
        memory_total_bytes,
        memory_used_bytes,
        disk_total_bytes,
        disk_available_bytes,
    }
}

/// Usage for the most specific mounted filesystem containing `path` (i.e.
/// the mount point with the longest matching prefix), since `data_dir`
/// holds everything Odin manages (steamcmd, the game install, every
/// instance's saves).
fn disk_usage_for(path: &std::path::Path) -> (u64, u64) {
    let disks = Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|disk| path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| (disk.total_space(), disk.available_space()))
        .unwrap_or((0, 0))
}

/// Computed by `spawn_telemetry` for each currently-running instance on its
/// background tick. Blocking (`sysinfo` process walk) — call from a
/// blocking context.
pub(crate) fn compute_instance_snapshot(
    state: &AppState,
    instance: &Instance,
) -> anyhow::Result<InstanceSnapshot> {
    // Ping the live supervisor first — it's a more direct signal than the
    // sysinfo/pid-fingerprint check (no reliance on a periodic full-process
    // refresh), and correctly reports "running" even in the moment right
    // after a start/restart before this tick's own sysinfo refresh has
    // necessarily seen the new pid. Fall back to the pid check for an
    // instance with no reachable supervisor (started by a pre-upgrade
    // binary, or whose supervisor has crashed but Valheim itself survived).
    let running = match crate::supervisor::client::ping_blocking(
        &state.paths,
        &instance.state.name,
        SUPERVISOR_PING_TIMEOUT,
    ) {
        Ok(_) => true,
        Err(_) => lifecycle::is_running(instance)?,
    };
    if !running {
        return Ok(InstanceSnapshot::default());
    }
    let root_pids: Vec<u32> = instance.state.pid.into_iter().collect();

    let system = state.resources.lock().expect("resources lock poisoned");
    let mut cpu_percent = 0.0;
    let mut memory_bytes = 0;
    for pid in collect_descendants(&system, &root_pids) {
        if let Some(process) = system.process(Pid::from_u32(pid)) {
            cpu_percent += process.cpu_usage();
            memory_bytes += process.memory();
        }
    }

    Ok(InstanceSnapshot {
        running: true,
        cpu_percent,
        memory_bytes,
    })
}

/// `pid` normally *is* the Valheim server process directly (it's spawned
/// straight from `process::build_command`, no intermediate shell), but this
/// also walks any descendants so resource usage stays correct regardless.
fn collect_descendants(system: &System, roots: &[u32]) -> Vec<u32> {
    let mut children_by_parent: std::collections::HashMap<u32, Vec<u32>> =
        std::collections::HashMap::new();
    for (candidate_pid, process) in system.processes() {
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
