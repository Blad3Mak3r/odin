use axum::Json;
use axum::extract::{Path, State};

use crate::instance::Instance;
use crate::web::error::{ApiResult, run_blocking};
use crate::web::players::PlayerInfo;
use crate::web::state::AppState;

/// REST fallback for the initial page load / a lost WebSocket connection —
/// the live view otherwise comes from the `resources` tick on `/events/ws`.
pub async fn get_instance_players(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<Json<Vec<PlayerInfo>>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let load_name = name.clone();
    run_blocking(move || Instance::load_existing(&paths, &db, &load_name)).await?;
    Ok(Json(state.players.snapshot(&name)))
}
