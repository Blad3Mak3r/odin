//! API endpoints for game-neutral per-instance CPU and memory limits.

use axum::Json;
use axum::extract::{Path, State};

use crate::db::resource_limits::{self, ResourceLimits};
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::routes::games;
use crate::web::state::AppState;

pub async fn get_resource_limits(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ResourceLimits>> {
    games::resolve_instance_id(&state, &id).await?;
    let db = state.db.clone();
    let limits = run_blocking(move || resource_limits::load(&db, &id)).await?;
    Ok(Json(limits))
}

pub async fn set_resource_limits(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(limits): Json<ResourceLimits>,
) -> ApiResult<Json<ResourceLimits>> {
    limits
        .validate()
        .map_err(|error| BadRequest(error.to_string()))?;
    games::resolve_instance_id(&state, &id).await?;
    let db = state.db.clone();
    let saved = run_blocking(move || resource_limits::save(&db, &id, &limits)).await?;
    Ok(Json(saved))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::*;
    use crate::db::Db;
    use crate::paths::Paths;

    fn app() -> (axum::Router, String) {
        let dir = std::env::temp_dir().join(format!(
            "odin-resource-limits-route-test-{}-{}",
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
            crate::game::GameId::Palworld,
            "limit-test",
        )
        .unwrap();
        (
            crate::web::router::build_router(AppState::new(paths, db)),
            instance.identity.id,
        )
    }

    #[tokio::test]
    async fn limits_route_round_trips_for_any_game_identity() {
        let (router, id) = app();
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/instances/{id}/resource-limits"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"cpu_percent":150,"memory_max_bytes":2147483648}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = router
            .oneshot(
                Request::builder()
                    .uri(format!("/api/instances/{id}/resource-limits"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(std::str::from_utf8(&body).unwrap().contains("2147483648"));

        let (router, id) = app();
        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/instances/{id}/resource-limits"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"cpu_percent":0,"memory_max_bytes":null}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(
            std::str::from_utf8(&body)
                .unwrap()
                .contains("greater than zero")
        );
    }
}
