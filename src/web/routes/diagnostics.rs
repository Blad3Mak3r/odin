use axum::Json;
use axum::extract::{Path, State};

use crate::supervisor::client;
use crate::supervisor::protocol::{LastExitInfo, Response};
use crate::web::state::AppState;

/// The most recent exit of this instance's Valheim child — deliberate or
/// not, including one already superseded by an in-place automatic restart
/// — so a crash-loop is debuggable without digging through raw
/// `console.log`. `null` if the instance has no reachable supervisor (it
/// only ever tracks its own child, so there's nothing to fall back to) or
/// hasn't exited since its current supervisor started.
pub async fn get_last_exit(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Json<Option<LastExitInfo>> {
    let info = match client::last_exit(&state.paths, &name).await {
        Ok(Response::LastExit { info }) => info,
        _ => None,
    };
    Json(info)
}
