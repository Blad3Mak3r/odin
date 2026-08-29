use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use serde::Deserialize;
use sysinfo::{Disks, Pid};

use crate::db::resource_samples::{self, ResourceSampleRow};
use crate::instance::{Instance, lifecycle};
use crate::web::error::{ApiResult, run_blocking};
use crate::web::runtime::{HostSnapshot, InstanceSnapshot, ResourceSample};
use crate::web::state::AppState;

/// A range beyond what `RuntimeRegistry`'s in-memory buffer covers (~6
/// minutes) — present means "read from the database", absent keeps today's
/// fast in-memory path for the live chart.
#[derive(Deserialize)]
pub struct HistoryQuery {
    pub hours: Option<u32>,
}

const DEFAULT_EXPORT_HOURS: u32 = 24 * 7;

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
    Query(query): Query<HistoryQuery>,
) -> ApiResult<Json<Vec<ResourceSample>>> {
    match query.hours {
        Some(hours) => {
            let db = state.db.clone();
            let since = Utc::now() - chrono::Duration::hours(hours as i64);
            let rows = run_blocking(move || resource_samples::range(&db, None, since)).await?;
            Ok(Json(rows.into_iter().map(Into::into).collect()))
        }
        None => Ok(Json(state.runtime.host_history())),
    }
}

pub async fn export_host_resources_history(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> ApiResult<Response> {
    let hours = query.hours.unwrap_or(DEFAULT_EXPORT_HOURS);
    let db = state.db.clone();
    let since = Utc::now() - chrono::Duration::hours(hours as i64);
    let rows = run_blocking(move || resource_samples::range(&db, None, since)).await?;
    Ok(csv_response("host-resources.csv", &rows))
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
    Query(query): Query<HistoryQuery>,
) -> ApiResult<Json<Vec<ResourceSample>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let load_name = name.clone();
    run_blocking(move || Instance::load_existing(&paths, &db, &load_name)).await?;

    match query.hours {
        Some(hours) => {
            let db = state.db.clone();
            let since = Utc::now() - chrono::Duration::hours(hours as i64);
            let rows =
                run_blocking(move || resource_samples::range(&db, Some(&name), since)).await?;
            Ok(Json(rows.into_iter().map(Into::into).collect()))
        }
        None => Ok(Json(state.runtime.instance_history(&name))),
    }
}

pub async fn export_instance_resources_history(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> ApiResult<Response> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let load_name = name.clone();
    run_blocking(move || Instance::load_existing(&paths, &db, &load_name)).await?;

    let hours = query.hours.unwrap_or(DEFAULT_EXPORT_HOURS);
    let db = state.db.clone();
    let since = Utc::now() - chrono::Duration::hours(hours as i64);
    let export_name = name.clone();
    let rows =
        run_blocking(move || resource_samples::range(&db, Some(&export_name), since)).await?;
    Ok(csv_response(&format!("{name}-resources.csv"), &rows))
}

fn csv_response(filename: &str, rows: &[ResourceSampleRow]) -> Response {
    let mut csv = String::from("at,cpu_percent,memory_bytes\n");
    for row in rows {
        csv.push_str(&format!(
            "{},{},{}\n",
            row.at.to_rfc3339(),
            row.cpu_percent,
            row.memory_bytes
        ));
    }
    (
        [
            (header::CONTENT_TYPE, "text/csv".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        csv,
    )
        .into_response()
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
    let supervisor_ping = crate::supervisor::client::ping_blocking(
        &state.paths,
        &instance.state.name,
        SUPERVISOR_PING_TIMEOUT,
    );
    let running = match &supervisor_ping {
        Ok(_) => true,
        Err(_) => lifecycle::is_running(instance)?,
    };
    if !running {
        return Ok(InstanceSnapshot::default());
    }

    // Whether Valheim itself is done loading, not just "the process/socket
    // exists" — only a reachable supervisor can tell us this (it's the one
    // parsing `console.log` for the readiness marker). No host-side
    // fallback exists for it, unlike CPU/memory, so an unreachable
    // supervisor just falls back to treating "running" as "ready" — the
    // same experience the dashboard already had before this distinction
    // existed.
    let ready = match &supervisor_ping {
        Ok(crate::supervisor::protocol::Response::Pong { ready, .. }) => *ready,
        _ => running,
    };

    // A reachable supervisor already tracks its own child's resource usage
    // (a background refresh loop scoped to just that one process tree, no
    // need to guess from odin serve's own host-wide process table) — prefer
    // its answer. `Response::Error` here means either an old supervisor that
    // doesn't understand `Stats`, or a fresh one that hasn't completed its
    // first refresh yet; either way, fall through to the host-side walk.
    if supervisor_ping.is_ok()
        && let Ok(crate::supervisor::protocol::Response::Stats {
            cpu_percent,
            memory_bytes,
        }) = crate::supervisor::client::stats_blocking(
            &state.paths,
            &instance.state.name,
            SUPERVISOR_PING_TIMEOUT,
        )
    {
        return Ok(InstanceSnapshot {
            running: true,
            ready,
            cpu_percent,
            memory_bytes,
        });
    }

    let root_pids: Vec<u32> = instance.state.pid.into_iter().collect();

    let system = state.resources.lock().expect("resources lock poisoned");
    let mut cpu_percent = 0.0;
    let mut memory_bytes = 0;
    for pid in crate::instance::process::descendant_pids(&system, &root_pids) {
        if let Some(process) = system.process(Pid::from_u32(pid)) {
            cpu_percent += process.cpu_usage();
            memory_bytes += process.memory();
        }
    }

    Ok(InstanceSnapshot {
        running: true,
        ready,
        cpu_percent,
        memory_bytes,
    })
}
