use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post, put};
use tower_http::trace::TraceLayer;

use crate::web::routes::{
    backups, bulk, changelog, config_files, diagnostics, doctor, events, install, instances, jobs,
    lists, mods, nexus, players, resources, settings, version, webhooks,
};
use crate::web::state::AppState;
use crate::web::{sse, static_files};

/// Ceiling for an uploaded mod `.zip` — generous enough for a real modpack
/// while still bounding memory/disk from a runaway or malicious upload.
/// Axum's own default body limit (2 MiB) applies to every other route.
const MOD_UPLOAD_BODY_LIMIT: usize = 500 * 1024 * 1024;

pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/version", get(version::get_version))
        .route("/changelog", get(changelog::get_changelog))
        .route("/doctor", get(doctor::get_doctor))
        .route("/install", post(install::install_server))
        .route("/install/status", get(install::get_install_status))
        .route(
            "/instances",
            get(instances::list_instances).post(instances::create_instance),
        )
        .route("/instances/bulk/start", post(bulk::bulk_start))
        .route("/instances/bulk/stop", post(bulk::bulk_stop))
        .route("/instances/bulk/restart", post(bulk::bulk_restart))
        .route("/instances/bulk/mods/update", post(bulk::bulk_update_mods))
        .route(
            "/instances/{name}",
            get(instances::get_instance).delete(instances::delete_instance),
        )
        .route("/instances/{name}/start", post(instances::start_instance))
        .route("/instances/{name}/stop", post(instances::stop_instance))
        .route(
            "/instances/{name}/restart",
            post(instances::restart_instance),
        )
        .route("/instances/{name}/rename", post(instances::rename_instance))
        .route(
            "/instances/{name}/config",
            get(instances::get_config).put(instances::set_config),
        )
        .route("/instances/{name}/logs", get(instances::get_logs))
        .route("/instances/{name}/logs/sse", get(sse::logs_sse))
        .route(
            "/instances/{name}/last-exit",
            get(diagnostics::get_last_exit),
        )
        .route(
            "/instances/{name}/mods",
            get(mods::list_mods).post(mods::add_mod),
        )
        .route("/instances/{name}/mods/update", post(mods::update_mods))
        .route(
            "/instances/{name}/mods/upload",
            post(mods::upload_mod).layer(DefaultBodyLimit::max(MOD_UPLOAD_BODY_LIMIT)),
        )
        .route("/instances/{name}/mods/{mod_id}", delete(mods::remove_mod))
        .route(
            "/instances/{name}/mods/{mod_id}/enable",
            post(mods::enable_mod),
        )
        .route(
            "/instances/{name}/mods/{mod_id}/disable",
            post(mods::disable_mod),
        )
        .route(
            "/instances/{name}/backups",
            get(backups::list_backups).post(backups::create_backup),
        )
        .route(
            "/instances/{name}/backups/{id}/restore",
            post(backups::restore_backup),
        )
        .route(
            "/instances/{name}/backups/{id}",
            delete(backups::delete_backup),
        )
        .route(
            "/instances/{name}/backup-schedule",
            get(backups::get_backup_schedule).put(backups::set_backup_schedule),
        )
        .route(
            "/instances/{name}/backup-storage",
            get(backups::get_backup_storage).put(backups::set_backup_storage),
        )
        .route(
            "/instances/{name}/bepinex/config",
            get(config_files::list_config_files),
        )
        .route(
            "/instances/{name}/bepinex/config/{filename}",
            get(config_files::get_config_file).put(config_files::set_config_file),
        )
        .route("/mods/search", get(mods::search_mods))
        .route("/mods/nexus/trending", get(nexus::trending_mods))
        .route("/mods/nexus/lookup", get(nexus::lookup_mod))
        .route("/mods", get(mods::list_global_mods))
        .route("/mods/{mod_id}", delete(mods::prune_mod))
        .route("/settings", get(settings::get_settings))
        .route(
            "/settings/nexus-api-key",
            put(settings::set_nexus_api_key).delete(settings::clear_nexus_api_key),
        )
        .route(
            "/instances/{name}/lists/{kind}",
            get(lists::get_list)
                .put(lists::set_list)
                .post(lists::add_list_entry),
        )
        .route(
            "/instances/{name}/lists/{kind}/{id}",
            delete(lists::remove_list_entry),
        )
        .route("/jobs", get(jobs::list_jobs))
        .route("/jobs/{id}", get(jobs::get_job))
        .route("/jobs/{id}/sse", get(jobs::job_sse))
        .route("/events/sse", get(events::events_sse))
        .route("/system/resources", get(resources::get_host_resources))
        .route(
            "/system/resources/history",
            get(resources::get_host_resources_history),
        )
        .route(
            "/system/resources/history/export",
            get(resources::export_host_resources_history),
        )
        .route(
            "/instances/{name}/resources",
            get(resources::get_instance_resources),
        )
        .route(
            "/instances/{name}/resources/history",
            get(resources::get_instance_resources_history),
        )
        .route(
            "/instances/{name}/resources/history/export",
            get(resources::export_instance_resources_history),
        )
        .route(
            "/instances/{name}/players",
            get(players::get_instance_players),
        )
        .route(
            "/webhooks",
            get(webhooks::list_webhooks).post(webhooks::create_webhook),
        )
        .route("/webhooks/{id}", delete(webhooks::delete_webhook))
        .route("/webhooks/{id}/enable", post(webhooks::enable_webhook))
        .route("/webhooks/{id}/disable", post(webhooks::disable_webhook))
        .route("/webhooks/{id}/test", post(webhooks::test_webhook))
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .route("/", get(static_files::serve_index))
        .route("/{*path}", get(static_files::serve_asset))
        .layer(TraceLayer::new_for_http())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::paths::Paths;
    use std::sync::Arc;

    // `Router::route` panics at registration time if two routes' path
    // shapes are ambiguous — e.g. a literal segment landing where another
    // route already has a `{param}` at the same depth (`/instances/bulk/...`
    // vs. `/instances/{name}/...`). This is the only place that would
    // surface, since nothing else calls `build_router` outside `odin serve`.
    #[test]
    fn router_builds_without_panicking_on_overlapping_route_shapes() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let state = AppState::new(paths, db);
        let _ = build_router(state);
    }
}
