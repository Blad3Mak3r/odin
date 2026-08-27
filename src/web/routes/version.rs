use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::odin_update;
use crate::web::error::{ApiResult, run_blocking};
use crate::web::state::AppState;

#[derive(Serialize)]
pub struct VersionView {
    pub version: &'static str,
    pub latest_version: Option<String>,
    pub latest_release_url: Option<String>,
    pub update_available: bool,
}

pub async fn get_version(State(state): State<AppState>) -> ApiResult<Json<VersionView>> {
    let db = state.db.clone();
    let status = run_blocking(move || odin_update::check(&db)).await?;
    Ok(Json(VersionView {
        version: status.current_version,
        latest_version: status.latest_version,
        latest_release_url: status.latest_release_url,
        update_available: status.update_available,
    }))
}
