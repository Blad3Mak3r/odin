use axum::Json;
use axum::extract::State;

use crate::doctor::{self, CheckResult};
use crate::web::error::ApiResult;
use crate::web::state::AppState;

pub async fn get_doctor(State(state): State<AppState>) -> ApiResult<Json<Vec<CheckResult>>> {
    let paths = state.paths.clone();
    let checks = crate::web::error::run_blocking(move || Ok(doctor::run_checks(&paths))).await?;
    Ok(Json(checks))
}
