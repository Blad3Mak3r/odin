use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use serde::Deserialize;
use sysinfo::Disks;

use crate::db::game_instances::GameInstanceIdentity;
use crate::db::resource_samples::{self, ResourceSampleRow};
use crate::instance::Instance;
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

pub(crate) fn csv_response(filename: &str, rows: &[ResourceSampleRow]) -> Response {
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

/// Snapshot of a running containerized game: CPU and memory come from the
/// container's cgroup via Podman, readiness from its runtime's control socket.
/// A container that cannot answer yields zeros rather than an error so one
/// unresponsive container never blanks the whole telemetry tick.
pub(crate) fn container_snapshot(
    state: &AppState,
    identity: &GameInstanceIdentity,
) -> InstanceSnapshot {
    let control_socket = crate::podman::lifecycle::control_socket(&state.paths, identity);
    let ready = matches!(
        crate::supervisor::client::ping_socket_blocking(&control_socket, SUPERVISOR_PING_TIMEOUT),
        Ok(crate::supervisor::protocol::Response::Pong { ready: true, .. })
    );
    let container = crate::podman::container_name(identity.game, &identity.id);
    let (cpu_percent, memory_bytes) =
        match crate::podman::lifecycle::container_stats(&state.paths, identity) {
            Ok(Some(stats)) => (
                state.runtime.container_cpu_percent(&container, &stats),
                stats.memory_bytes,
            ),
            _ => {
                state.runtime.forget_container_cpu(&container);
                (0.0, 0)
            }
        };
    InstanceSnapshot {
        running: true,
        ready,
        cpu_percent,
        memory_bytes,
    }
}

/// Computed by `spawn_telemetry` for each currently-running instance on its
/// background tick. Blocking (Podman and control-socket calls) — call from
/// a blocking context.
pub(crate) fn compute_instance_snapshot(
    state: &AppState,
    instance: &Instance,
) -> anyhow::Result<InstanceSnapshot> {
    let identity = crate::db::game_instances::ensure_valheim_identity(
        &state.db,
        &instance.state.name,
        instance.state.created_at,
    )?;
    if !crate::podman::dispatch::is_running(
        &state.paths,
        &state.db,
        crate::game::GameId::Valheim,
        &instance.state.name,
    )? {
        state
            .runtime
            .forget_container_cpu(&crate::podman::container_name(identity.game, &identity.id));
        return Ok(InstanceSnapshot::default());
    }
    Ok(container_snapshot(state, &identity))
}
