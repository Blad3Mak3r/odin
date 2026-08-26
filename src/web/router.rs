use axum::Router;
use axum::routing::{delete, get, post};
use tower_http::trace::TraceLayer;

use crate::web::routes::{
    config_files, doctor, events, install, instances, jobs, lists, mods, players, resources,
    version,
};
use crate::web::state::AppState;
use crate::web::{static_files, ws};

pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/version", get(version::get_version))
        .route("/doctor", get(doctor::get_doctor))
        .route("/install", post(install::install_server))
        .route("/install/status", get(install::get_install_status))
        .route(
            "/instances",
            get(instances::list_instances).post(instances::create_instance),
        )
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
        .route("/instances/{name}/logs/ws", get(ws::logs_ws))
        .route(
            "/instances/{name}/mods",
            get(mods::list_mods).post(mods::add_mod),
        )
        .route("/instances/{name}/mods/update", post(mods::update_mods))
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
            "/instances/{name}/bepinex/config",
            get(config_files::list_config_files),
        )
        .route(
            "/instances/{name}/bepinex/config/{filename}",
            get(config_files::get_config_file).put(config_files::set_config_file),
        )
        .route("/mods/search", get(mods::search_mods))
        .route("/mods", get(mods::list_global_mods))
        .route("/mods/{mod_id}", delete(mods::prune_mod))
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
        .route("/jobs/{id}/ws", get(jobs::job_ws))
        .route("/events/ws", get(events::events_ws))
        .route("/system/resources", get(resources::get_host_resources))
        .route(
            "/system/resources/history",
            get(resources::get_host_resources_history),
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
            "/instances/{name}/players",
            get(players::get_instance_players),
        )
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .route("/", get(static_files::serve_index))
        .route("/{*path}", get(static_files::serve_asset))
        .layer(TraceLayer::new_for_http())
}
