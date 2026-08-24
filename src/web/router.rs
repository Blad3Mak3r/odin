use axum::Router;
use axum::routing::{delete, get, post};
use tower_http::trace::TraceLayer;

use crate::web::routes::{doctor, install, instances, jobs, lists, mods, resources};
use crate::web::state::AppState;
use crate::web::{static_files, ws};

pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/doctor", get(doctor::get_doctor))
        .route("/install", post(install::install_server))
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
        .route("/instances/{name}/console/ws", get(ws::console_ws))
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
        .route("/mods/search", get(mods::search_mods))
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
        .route("/system/resources", get(resources::get_host_resources))
        .route(
            "/instances/{name}/resources",
            get(resources::get_instance_resources),
        )
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .route("/", get(static_files::serve_index))
        .route("/{*path}", get(static_files::serve_asset))
        .layer(TraceLayer::new_for_http())
}
