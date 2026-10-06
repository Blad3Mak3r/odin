use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{any, delete, get, post, put};
use tower_http::trace::TraceLayer;

use crate::web::routes::{
    backups, bepinex, bulk, changelog, config_files, diagnostics, doctor, events, games, install,
    instances, jobs, lists, mods, nexus, palworld, players, resource_limits, resources,
    rust_access_lists, saves, settings, seven_days_to_die, uptime_schedules, version,
    vrising_access_lists, webhooks,
};
use crate::web::state::AppState;
use crate::web::{sse, static_files};

/// Ceiling for an uploaded mod `.zip` — generous enough for a real modpack
/// while still bounding memory/disk from a runaway or malicious upload.
/// Axum's own default body limit (2 MiB) applies to every other route.
const MOD_UPLOAD_BODY_LIMIT: usize = 512 * 1024 * 1024;

pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/version", get(version::get_version))
        .route("/changelog", get(changelog::get_changelog))
        .route("/doctor", get(doctor::get_doctor))
        .route("/install", post(install::install_server))
        .route("/install/status", get(install::get_install_status))
        .route("/games", get(games::list_games))
        .route("/games/7d2d/worlds", get(games::list_seven_days_worlds))
        .route(
            "/instances/bulk/games/{action}",
            post(bulk::bulk_games_by_id),
        )
        .route("/games/instances", get(games::list_all_instances))
        // UUID is the public identity contract. The historical bare
        // `/instances/{name}` routes have been removed; game/name endpoints
        // below are transitional internal compatibility routes only.
        .route(
            "/instances/{id}",
            get(games::get_instance_by_id).delete(games::delete_instance_by_id),
        )
        .route("/instances/{id}/start", post(games::start_instance_by_id))
        .route("/instances/{id}/stop", post(games::stop_instance_by_id))
        .route(
            "/instances/{id}/restart",
            post(games::restart_instance_by_id),
        )
        .route("/instances/{id}/tags", put(games::set_tags_by_id))
        .route(
            "/instances/{id}/resource-limits",
            get(resource_limits::get_resource_limits).put(resource_limits::set_resource_limits),
        )
        .route("/instances/{id}/rename", post(games::rename_instance_by_id))
        .route(
            "/instances/{id}/uptime-schedule",
            get(uptime_schedules::get_uptime_schedule_by_id)
                .put(uptime_schedules::set_uptime_schedule_by_id),
        )
        .route(
            "/instances/{id}/backups",
            get(games::list_backups_by_id).post(games::create_backup_by_id),
        )
        .route(
            "/instances/{id}/backups/{backup_id}/restore",
            post(games::restore_backup_by_id),
        )
        .route(
            "/instances/{id}/backups/jobs",
            post(backups::create_backup_by_id),
        )
        .route(
            "/instances/{id}/backups/{backup_id}/restore/job",
            post(backups::restore_backup_by_id),
        )
        .route(
            "/instances/{id}/backups/{backup_id}",
            delete(backups::delete_backup_by_id),
        )
        .route(
            "/instances/{id}/backup-schedule",
            get(backups::get_backup_schedule_by_id).put(backups::set_backup_schedule_by_id),
        )
        .route(
            "/instances/{id}/backup-storage",
            get(backups::get_backup_storage_by_id).put(backups::set_backup_storage_by_id),
        )
        .route(
            "/instances/{id}/config",
            get(instances::get_config_by_id).put(games::update_config_by_id),
        )
        .route(
            "/instances/{id}/config/advanced",
            get(config_files::list_advanced_config_by_id)
                .put(config_files::set_advanced_config_by_id),
        )
        .route("/instances/{id}/logs", get(games::get_logs_by_id))
        .route("/instances/{id}/logs/sse", get(sse::game_logs_sse_by_id))
        .route(
            "/instances/{id}/valheim/clone",
            post(instances::clone_instance_by_id),
        )
        .route(
            "/instances/{id}/valheim/last-exit",
            get(diagnostics::get_last_exit_by_id),
        )
        .route(
            "/instances/{id}/valheim/players",
            get(players::get_instance_players_by_id),
        )
        .route(
            "/instances/{id}/valheim/players/history",
            get(players::get_player_history_by_id),
        )
        .route(
            "/instances/{id}/valheim/lists/{kind}",
            get(lists::get_list_by_id)
                .put(lists::set_list_by_id)
                .post(lists::add_list_entry_by_id),
        )
        .route(
            "/instances/{id}/valheim/lists/{kind}/{entry_id}",
            delete(lists::remove_list_entry_by_id),
        )
        .route(
            "/instances/{id}/valheim/bepinex/config",
            get(config_files::list_config_files_by_id),
        )
        .route(
            "/instances/{id}/valheim/bepinex/config/{filename}",
            get(config_files::get_config_file_by_id).put(config_files::set_config_file_by_id),
        )
        .route(
            "/instances/{id}/valheim/bepinex/status",
            get(bepinex::status_by_id),
        )
        .route(
            "/instances/{id}/valheim/bepinex/update",
            post(bepinex::update_by_id),
        )
        .route(
            "/instances/{id}/valheim/bepinex/install",
            post(bepinex::install_by_id),
        )
        .route(
            "/instances/{id}/valheim/mods",
            get(mods::list_mods_by_id).post(mods::add_mod_by_id),
        )
        .route(
            "/instances/{id}/valheim/mods/update",
            post(mods::update_mods_by_id),
        )
        .route(
            "/instances/{id}/valheim/mods/modpack",
            get(mods::download_modpack_by_id),
        )
        .route(
            "/instances/{id}/valheim/mods/upload",
            post(mods::upload_mod_by_id).layer(DefaultBodyLimit::max(MOD_UPLOAD_BODY_LIMIT)),
        )
        .route(
            "/instances/{id}/7d2d/mods",
            get(seven_days_to_die::list_mods),
        )
        .route(
            "/instances/{id}/7d2d/mods/upload",
            post(seven_days_to_die::upload_mod).layer(DefaultBodyLimit::max(MOD_UPLOAD_BODY_LIMIT)),
        )
        .route(
            "/instances/{id}/7d2d/console",
            post(seven_days_to_die::execute_console),
        )
        .route(
            "/instances/{id}/7d2d/players",
            get(seven_days_to_die::list_players),
        )
        .route(
            "/instances/{id}/7d2d/players/history",
            get(seven_days_to_die::player_history),
        )
        .route(
            "/instances/{id}/7d2d/players/{player}/kick",
            post(seven_days_to_die::kick_player),
        )
        .route(
            "/instances/{id}/7d2d/players/{player}/ban",
            post(seven_days_to_die::ban_player),
        )
        .route(
            "/instances/{id}/7d2d/players/{player}/unban",
            post(seven_days_to_die::unban_player),
        )
        .route(
            "/instances/{id}/7d2d/players/{player}/admin",
            post(seven_days_to_die::add_admin),
        )
        .route(
            "/instances/{id}/7d2d/players/{player}/whitelist",
            post(seven_days_to_die::add_to_whitelist),
        )
        .route(
            "/instances/{id}/7d2d/players/{player}/admin/remove",
            post(seven_days_to_die::remove_admin),
        )
        .route(
            "/instances/{id}/7d2d/players/{player}/whitelist/remove",
            post(seven_days_to_die::remove_from_whitelist),
        )
        .route(
            "/instances/{id}/7d2d/access/{kind}",
            get(seven_days_to_die::access_list),
        )
        .route(
            "/instances/{id}/valheim/mods/{mod_id}",
            delete(mods::remove_mod_by_id),
        )
        .route(
            "/instances/{id}/valheim/mods/{mod_id}/enable",
            post(mods::enable_mod_by_id),
        )
        .route(
            "/instances/{id}/valheim/mods/{mod_id}/disable",
            post(mods::disable_mod_by_id),
        )
        .route(
            "/instances/{id}/valheim/mods/{mod_id}/version",
            put(mods::select_mod_version_by_id),
        )
        .route(
            "/instances/{id}/valheim/mods/{mod_id}/pinned",
            put(mods::set_mod_pinned_by_id),
        )
        .route("/instances/{id}/palworld/players", get(palworld::players))
        .route("/instances/{id}/palworld/metrics", get(palworld::metrics))
        .route(
            "/instances/{id}/palworld/announce",
            post(palworld::announce),
        )
        .route("/instances/{id}/palworld/save", post(palworld::save))
        .route("/instances/{id}/palworld/kick", post(palworld::kick))
        .route("/instances/{id}/palworld/ban", post(palworld::ban))
        .route("/instances/{id}/palworld/unban", post(palworld::unban))
        .route(
            "/instances/{id}/palworld/shutdown",
            post(palworld::shutdown),
        )
        .route(
            "/instances/{id}/vrising/rcon",
            post(games::execute_vrising_rcon_by_id),
        )
        .route(
            "/instances/{id}/rust/rcon",
            post(games::execute_rust_rcon_by_id),
        )
        .route(
            "/instances/{id}/rust/wipe-map",
            post(games::wipe_rust_map_by_id),
        )
        .route(
            "/instances/{id}/rust/full-wipe",
            post(games::full_wipe_rust_by_id),
        )
        .route(
            "/instances/{id}/rust/clone",
            post(games::clone_rust_instance_by_id),
        )
        .route(
            "/instances/{id}/vrising/lists/{kind}",
            get(vrising_access_lists::get_list_by_id)
                .post(vrising_access_lists::add_list_entry_by_id),
        )
        .route(
            "/instances/{id}/vrising/lists/{kind}/{entry_id}",
            delete(vrising_access_lists::remove_list_entry_by_id),
        )
        .route(
            "/instances/{id}/rust/lists/{kind}",
            get(rust_access_lists::get_list_by_id)
                .put(rust_access_lists::set_list_by_id)
                .post(rust_access_lists::add_list_entry_by_id),
        )
        .route(
            "/instances/{id}/rust/lists/{kind}/{entry_id}",
            delete(rust_access_lists::remove_list_entry_by_id),
        )
        .route(
            "/instances/{id}/rust/resources",
            get(games::get_rust_resources_by_id),
        )
        .route(
            "/instances/{id}/rust/resources/history",
            get(games::get_rust_resource_history_by_id),
        )
        .route(
            "/instances/{id}/rust/resources/history/export",
            get(games::export_rust_resource_history_by_id),
        )
        .route("/instances/{id}/resources", get(games::get_resources_by_id))
        .route(
            "/instances/{id}/resources/history",
            get(games::get_resource_history_by_id),
        )
        .route(
            "/instances/{id}/resources/history/export",
            get(games::export_resource_history_by_id),
        )
        .route("/instances/{id}/saves", get(saves::list_save_files_by_id))
        .route(
            "/instances/{id}/saves/{*path}",
            get(saves::download_save_file_by_id),
        )
        .route("/games/{game}/install", post(games::install_game))
        .route(
            "/games/{game}/install/status",
            get(games::get_install_status),
        )
        .route(
            "/games/{game}/instances",
            get(games::list_instances).post(games::create_instance),
        )
        .route(
            "/instances",
            get(instances::list_instances).post(instances::create_instance),
        )
        .route("/instances/bulk/start", post(bulk::bulk_start))
        .route("/instances/bulk/stop", post(bulk::bulk_stop))
        .route("/instances/bulk/restart", post(bulk::bulk_restart))
        .route("/instances/bulk/mods/update", post(bulk::bulk_update_mods))
        .route(
            "/instances/bulk/bepinex/update",
            post(bulk::bulk_update_bepinex),
        )
        .route("/mods/search", get(mods::search_mods))
        .route("/mods/nexus/trending", get(nexus::trending_mods))
        .route("/mods/nexus/lookup", get(nexus::lookup_mod))
        .route("/mods", get(mods::list_global_mods))
        .route("/mods/{mod_id}", delete(mods::prune_mod))
        .route(
            "/mods/{mod_id}/versions/{version}",
            delete(mods::prune_mod_version),
        )
        .route("/settings", get(settings::get_settings))
        .route(
            "/settings/instance-defaults",
            put(settings::set_instance_defaults),
        )
        .route(
            "/settings/nexus-api-key",
            put(settings::set_nexus_api_key).delete(settings::clear_nexus_api_key),
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
            "/webhooks",
            get(webhooks::list_webhooks).post(webhooks::create_webhook),
        )
        .route(
            "/webhooks/{id}",
            delete(webhooks::delete_webhook).put(webhooks::update_webhook),
        )
        .route("/webhooks/{id}/enable", post(webhooks::enable_webhook))
        .route("/webhooks/{id}/disable", post(webhooks::disable_webhook))
        .route("/webhooks/{id}/test", post(webhooks::test_webhook))
        // Never let the dashboard's SPA fallback turn a retired or misspelled
        // API URL into a successful HTML response.
        .fallback(|| async { axum::http::StatusCode::NOT_FOUND })
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .route("/api/{*path}", any(api_not_found))
        .route("/", get(static_files::serve_index))
        .route("/{*path}", get(static_files::serve_asset))
        .layer(TraceLayer::new_for_http())
}

async fn api_not_found() -> axum::http::StatusCode {
    axum::http::StatusCode::NOT_FOUND
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::*;
    use crate::db::Db;
    use crate::instance::Instance;
    use crate::paths::Paths;
    use std::io::{Cursor, Write};
    use std::process::Command;
    use std::sync::Arc;

    fn seven_days_mod_zip() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut archive = zip::ZipWriter::new(cursor);
        archive
            .start_file(
                "Example_Mod/ModInfo.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(b"<xml><Name value=\"Example_Mod\"/><DisplayName value=\"Example Mod\"/><Version value=\"1.0.0\"/></xml>")
            .unwrap();
        archive.finish().unwrap().into_inner()
    }

    fn multipart_file_body(boundary: &str, bytes: Vec<u8>) -> Vec<u8> {
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"mod.zip\"\r\nContent-Type: application/zip\r\n\r\n"
        )
        .into_bytes();
        body.extend(bytes);
        body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
        body
    }

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

    #[tokio::test]
    async fn valheim_config_route_uses_the_durable_uuid() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-valheim-module-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let instance = Instance::create(&paths, &db, "meadows").unwrap();
        let id = crate::db::game_instances::ensure_valheim_identity(
            &db,
            "meadows",
            instance.state.created_at,
        )
        .unwrap()
        .id;
        let app = build_router(AppState::new(paths, db));
        let request = Request::builder()
            .uri(format!("/api/instances/{id}/config"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn seven_days_mod_routes_list_mods_and_clean_up_conflicting_uploads() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-7d2d-mods-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let instance = crate::db::game_instances::create_generic(
            &paths,
            &db,
            crate::game::GameId::SevenDaysToDie,
            "undead",
        )
        .unwrap();
        let installed = paths
            .game_instance_dir(crate::game::GameId::SevenDaysToDie, "undead")
            .join("Mods/Example_Mod");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(
            installed.join("ModInfo.xml"),
            "<xml><Name value=\"Example_Mod\"/><DisplayName value=\"Example Mod\"/><Version value=\"1.0.0\"/></xml>",
        )
        .unwrap();
        let active = crate::db::game_instances::create_generic(
            &paths,
            &db,
            crate::game::GameId::SevenDaysToDie,
            "active-undead",
        )
        .unwrap();
        let pid = std::process::id();
        crate::db::game_instances::set_generic_pid(
            &db,
            crate::game::GameId::SevenDaysToDie,
            "active-undead",
            pid,
            crate::instance::process::start_time_of(pid).unwrap(),
            chrono::Utc::now(),
        )
        .unwrap();
        let app = build_router(AppState::new(paths.clone(), db));

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/instances/{}/7d2d/mods", instance.identity.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()[0]["name"],
            "Example_Mod"
        );

        let boundary = "odin-test-boundary";
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/instances/{}/7d2d/mods/upload",
                        instance.identity.id
                    ))
                    .header(
                        "content-type",
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(multipart_file_body(
                        boundary,
                        seven_days_mod_zip(),
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(
            std::fs::read_dir(installed.parent().unwrap())
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".odin-upload-"))
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/instances/{}/7d2d/mods/upload",
                        active.identity.id
                    ))
                    .header(
                        "content-type",
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(multipart_file_body(
                        boundary,
                        seven_days_mod_zip(),
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[tokio::test]
    async fn valheim_instance_route_keeps_the_full_dashboard_view() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-valheim-status-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let instance = Instance::create(&paths, &db, "meadows").unwrap();
        let id = crate::db::game_instances::ensure_valheim_identity(
            &db,
            "meadows",
            instance.state.created_at,
        )
        .unwrap()
        .id;
        let app = build_router(AppState::new(paths, db));
        let request = Request::builder()
            .uri(format!("/api/instances/{id}"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn bepinex_install_route_rejects_a_running_valheim_instance() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-bepinex-install-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let instance = Instance::create(&paths, &db, "meadows").unwrap();
        let id = crate::db::game_instances::ensure_valheim_identity(
            &db,
            "meadows",
            instance.state.created_at,
        )
        .unwrap()
        .id;
        let mut child = Command::new("sleep").arg("60").spawn().unwrap();
        let pid = child.id();
        let started_at = crate::instance::process::start_time_of(pid).unwrap();
        crate::db::instances::set_pid(&db, "meadows", pid, started_at, chrono::Utc::now()).unwrap();

        let app = build_router(AppState::new(paths.clone(), db));
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/instances/{id}/valheim/bepinex/install"))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
        child.kill().unwrap();
        child.wait().unwrap();
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[tokio::test]
    async fn bepinex_install_route_rejects_an_existing_installation() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-bepinex-existing-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let instance = Instance::create(&paths, &db, "meadows").unwrap();
        let id = crate::db::game_instances::ensure_valheim_identity(
            &db,
            "meadows",
            instance.state.created_at,
        )
        .unwrap()
        .id;
        crate::db::instances::set_bepinex(&db, "meadows", true, Some("5.4.0")).unwrap();

        let app = build_router(AppState::new(paths.clone(), db));
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/instances/{id}/valheim/bepinex/install"))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[tokio::test]
    async fn valheim_game_name_api_routes_are_not_registered() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-valheim-retired-route-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        Instance::create(&paths, &db, "meadows").unwrap();
        let app = build_router(AppState::new(paths, db));
        let request = Request::builder()
            .method("POST")
            .uri("/api/games/valheim/instances/meadows/rename")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"new_name":"mistlands"}"#))
            .unwrap();

        assert_eq!(
            app.oneshot(request).await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn rust_config_route_updates_rust_specific_configuration() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-rust-config-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let rust_id = crate::db::game_instances::create_rust(&paths, &db, "rusty")
            .unwrap()
            .identity
            .id;
        let app = build_router(AppState::new(paths.clone(), db.clone()));
        let request = Request::builder()
            .method("PUT")
            .uri(format!("/api/instances/{rust_id}/config"))
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"hostname":"Rusty Server","max_players":50,"port":29000,"query_port":30000,"rcon_port":31000,"rcon_password":"rcon-secret"}"#,
            ))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let view: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(view["config"]["port"], 29000);
        assert_eq!(view["config"]["query_port"], 30000);
        assert_eq!(view["config"]["rcon_port"], 31000);
        assert_eq!(view["config"]["rcon_password"], "rcon-secret");

        for (body, expected) in [
            (r#"{"port":0}"#, StatusCode::BAD_REQUEST),
            (r#"{"query_port":0}"#, StatusCode::BAD_REQUEST),
            (r#"{"rcon_port":0}"#, StatusCode::BAD_REQUEST),
            (r#"{"port":30000}"#, StatusCode::BAD_REQUEST),
            (r#"{"query_port":29000}"#, StatusCode::BAD_REQUEST),
            (r#"{"rcon_port":29000}"#, StatusCode::BAD_REQUEST),
            (r#"{"rcon_password":"   "}"#, StatusCode::BAD_REQUEST),
            (r#"{"port":65536}"#, StatusCode::BAD_REQUEST),
            (r#"{"query_port":-1}"#, StatusCode::BAD_REQUEST),
            (r#"{"rcon_port":29000.5}"#, StatusCode::BAD_REQUEST),
            (r#"{"port":29000.5}"#, StatusCode::BAD_REQUEST),
            (r#"{"seed":42}"#, StatusCode::OK),
        ] {
            let request = Request::builder()
                .method("PUT")
                .uri(format!("/api/instances/{rust_id}/config"))
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap();
            assert_eq!(
                app.clone().oneshot(request).await.unwrap().status(),
                expected,
                "{body}"
            );
            let saved = crate::db::game_instances::load_rust(&db, "rusty")
                .unwrap()
                .unwrap();
            assert_eq!(saved.config.port, 29000);
            assert_eq!(saved.config.query_port, 30000);
            assert_eq!(saved.config.rcon_port, 31000);
        }

        crate::db::game_instances::set_rust_pid(
            &db,
            "rusty",
            std::process::id(),
            crate::instance::process::start_time_of(std::process::id()).unwrap(),
            chrono::Utc::now(),
        )
        .unwrap();
        let request = Request::builder()
            .method("PUT")
            .uri(format!("/api/instances/{rust_id}/config"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"port":31000}"#))
            .unwrap();
        assert_eq!(
            app.oneshot(request).await.unwrap().status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            crate::db::game_instances::load_rust(&db, "rusty")
                .unwrap()
                .unwrap()
                .config
                .port,
            29000
        );
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[tokio::test]
    async fn rust_rcon_route_rejects_an_empty_command_without_connecting() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-rust-rcon-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let rust_id = crate::db::game_instances::create_rust(&paths, &db, "rusty")
            .unwrap()
            .identity
            .id;
        let app = build_router(AppState::new(paths.clone(), db));
        let request = Request::builder()
            .method("POST")
            .uri(format!("/api/instances/{rust_id}/rust/rcon"))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"command":"   "}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[tokio::test]
    async fn rust_wipe_map_route_requires_confirmation_and_records_activity() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-rust-wipe-map-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let instance = crate::db::game_instances::create_rust(&paths, &db, "rusty").unwrap();
        let rust_id = instance.identity.id.clone();
        let source = crate::game::rust::backup_source(&paths, &instance);
        std::fs::create_dir_all(&source).unwrap();
        let save = source.join("world.sav");
        std::fs::write(&save, "world").unwrap();

        let state = AppState::new(paths.clone(), db);
        let app = build_router(state.clone());
        let rejected = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/instances/{rust_id}/rust/wipe-map"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"confirmation":"wrong"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        assert!(save.is_file());

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/instances/{rust_id}/rust/wipe-map"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"confirmation":"rusty"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let job_id = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();

        let status = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let snapshot = state.jobs.get(&job_id).unwrap();
                if !matches!(
                    snapshot.status,
                    crate::web::jobs::JobStatus::Queued | crate::web::jobs::JobStatus::Running
                ) {
                    return snapshot.status;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("wipe job should finish");
        assert!(
            matches!(status, crate::web::jobs::JobStatus::Succeeded),
            "wipe job failed: {status:?}"
        );
        assert!(!save.exists());
        assert!(state.activity.subscribe().0.into_iter().any(|event| {
            matches!(event.kind, crate::activity::ActivityKind::MapWiped)
                && event.instance.as_deref() == Some("rusty")
        }));

        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[tokio::test]
    async fn rust_access_lists_route_edits_owner_entries() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-rust-access-lists-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let rust_id = crate::db::game_instances::create_rust(&paths, &db, "rusty")
            .unwrap()
            .identity
            .id;
        let app = build_router(AppState::new(paths.clone(), db));

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/instances/{rust_id}/rust/lists/owner"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"id":"76561197960287930"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/instances/{rust_id}/rust/lists/owner"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["ids"],
            serde_json::json!(["76561197960287930"])
        );

        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/instances/{rust_id}/rust/lists/invalid"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        std::fs::remove_dir_all(paths.data_dir).unwrap();
    }

    #[tokio::test]
    async fn canonical_delete_removes_only_the_selected_game_instance() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-game-delete-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        Instance::create(&paths, &db, "shared").unwrap();
        let rust_id = crate::db::game_instances::create_rust(&paths, &db, "shared")
            .unwrap()
            .identity
            .id;
        let app = build_router(AppState::new(paths.clone(), db.clone()));
        let request = Request::builder()
            .method("DELETE")
            .uri(format!("/api/instances/{rust_id}"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(Instance::load(&paths, &db, "shared").unwrap().is_some());
        assert!(
            crate::db::game_instances::load_rust(&db, "shared")
                .unwrap()
                .is_none()
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[tokio::test]
    async fn rust_resources_route_returns_a_snapshot() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-rust-resources-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let rust_id = crate::db::game_instances::create_rust(&paths, &db, "rusty")
            .unwrap()
            .identity
            .id;
        let app = build_router(AppState::new(paths, db));
        let request = Request::builder()
            .uri(format!("/api/instances/{rust_id}/resources"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rust_resource_history_route_returns_a_series() {
        let dir = std::env::temp_dir().join(format!(
            "odin-router-rust-resource-history-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Arc::new(Db::open(&paths).unwrap());
        let rust_id = crate::db::game_instances::create_rust(&paths, &db, "rusty")
            .unwrap()
            .identity
            .id;
        let app = build_router(AppState::new(paths, db));
        let request = Request::builder()
            .uri(format!("/api/instances/{rust_id}/resources/history"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
