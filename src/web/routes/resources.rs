use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;
use sysinfo::{Disks, Pid, System};

use crate::instance::{Instance, lifecycle};
use crate::web::error::{ApiResult, run_blocking};
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct HostResources {
    pub cpu_percent: f32,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub disk_available_bytes: u64,
}

pub async fn get_host_resources(State(state): State<AppState>) -> Json<HostResources> {
    let resources = state.resources.clone();
    let (cpu_percent, memory_total_bytes, memory_used_bytes) =
        tokio::task::spawn_blocking(move || {
            let system = resources.lock().expect("resources lock poisoned");
            (
                system.global_cpu_usage(),
                system.total_memory(),
                system.used_memory(),
            )
        })
        .await
        .unwrap_or((0.0, 0, 0));

    let data_dir = state.paths.data_dir.clone();
    let (disk_total_bytes, disk_available_bytes) =
        tokio::task::spawn_blocking(move || disk_usage_for(&data_dir))
            .await
            .unwrap_or((0, 0));

    Json(HostResources {
        cpu_percent,
        memory_total_bytes,
        memory_used_bytes,
        disk_total_bytes,
        disk_available_bytes,
    })
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

#[derive(Serialize)]
pub struct InstanceResources {
    pub running: bool,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
}

pub async fn get_instance_resources(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<InstanceResources>> {
    let paths = state.paths.clone();
    let (running, root_pids) = run_blocking(move || {
        let instance = Instance::load_existing(&paths, &name)?;
        if !lifecycle::is_running(&instance)? {
            return Ok((false, Vec::new()));
        }
        Ok((true, crate::tmux::pane_pids(&instance.state.tmux_session)?))
    })
    .await?;

    if !running {
        return Ok(Json(InstanceResources {
            running: false,
            cpu_percent: 0.0,
            memory_bytes: 0,
        }));
    }

    let resources = state.resources.clone();
    let (cpu_percent, memory_bytes) = tokio::task::spawn_blocking(move || {
        let system = resources.lock().expect("resources lock poisoned");
        let mut cpu_percent = 0.0;
        let mut memory_bytes = 0;
        for pid in collect_descendants(&system, &root_pids) {
            if let Some(process) = system.process(Pid::from_u32(pid)) {
                cpu_percent += process.cpu_usage();
                memory_bytes += process.memory();
            }
        }
        (cpu_percent, memory_bytes)
    })
    .await
    .unwrap_or((0.0, 0));

    Ok(Json(InstanceResources {
        running: true,
        cpu_percent,
        memory_bytes,
    }))
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
