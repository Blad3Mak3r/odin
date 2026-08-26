use axum::Json;
use axum::extract::{Path, State};
use sysinfo::{Disks, Pid, System};

use crate::instance::{Instance, lifecycle};
use crate::web::error::{ApiResult, run_blocking};
use crate::web::runtime::{HostSnapshot, InstanceSnapshot, ResourceSample};
use crate::web::state::AppState;

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
    let load_name = name.clone();
    run_blocking(move || Instance::load_existing(&paths, &load_name)).await?;
    Ok(Json(state.runtime.instance_snapshot(&name)))
}

pub async fn get_instance_resources_history(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Vec<ResourceSample>>> {
    let paths = state.paths.clone();
    let load_name = name.clone();
    run_blocking(move || Instance::load_existing(&paths, &load_name)).await?;
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
/// background tick. Blocking (tmux + `sysinfo` process walk) — call from a
/// blocking context.
pub(crate) fn compute_instance_snapshot(
    state: &AppState,
    instance: &Instance,
) -> anyhow::Result<InstanceSnapshot> {
    if !lifecycle::is_running(instance)? {
        return Ok(InstanceSnapshot::default());
    }
    let root_pids = crate::tmux::pane_pids(&instance.state.tmux_session)?;

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

/// A tmux pane's process normally *is* the Valheim server (the generated
/// `run.sh` ends in `exec`, so no fork happens), but this also walks any
/// descendants so resource usage stays correct if that ever changes.
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
