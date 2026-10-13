use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::db::game_instances;
use crate::game::GameId;
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

#[derive(Deserialize)]
pub struct CreateClusterRequest {
    pub name: String,
    pub cluster_id: String,
}

#[derive(Deserialize)]
pub struct SetMembersRequest {
    pub members: Vec<String>,
}

pub async fn list(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<game_instances::ArkCluster>>> {
    let db = state.db.clone();
    Ok(Json(
        run_blocking(move || game_instances::list_ark_clusters(&db)).await?,
    ))
}

pub async fn create(
    State(state): State<AppState>,
    Json(request): Json<CreateClusterRequest>,
) -> ApiResult<Json<game_instances::ArkCluster>> {
    let db = state.db.clone();
    Ok(Json(
        run_blocking(move || {
            game_instances::create_ark_cluster(&db, &request.name, &request.cluster_id)
        })
        .await?,
    ))
}

pub async fn set_members(
    State(state): State<AppState>,
    Path(cluster_id): Path<String>,
    Json(request): Json<SetMembersRequest>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        let current = game_instances::list_ark_clusters(&db)?
            .into_iter()
            .find(|cluster| cluster.id == cluster_id)
            .ok_or_else(|| anyhow::anyhow!("ARK cluster does not exist"))?;
        let affected = current
            .members
            .into_iter()
            .chain(request.members.iter().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        for member in &affected {
            let identity = game_instances::identity_by_id(&db, member)?
                .ok_or_else(|| anyhow::anyhow!("instance does not exist"))?;
            if identity.game != GameId::ArkSurvivalAscended {
                return Err(BadRequest("only ARK instances can join a cluster".into()).into());
            }
            if crate::podman::dispatch::is_running(&paths, &db, identity.game, &identity.name)? {
                return Err(crate::instance::InstanceError::AlreadyRunning(
                    "stop every affected ARK instance before changing cluster membership".into(),
                )
                .into());
            }
        }
        game_instances::set_ark_cluster_members(&db, &cluster_id, &request.members)?;
        for member in affected {
            let identity = game_instances::identity_by_id(&db, &member)?
                .ok_or_else(|| anyhow::anyhow!("instance does not exist"))?;
            crate::podman::dispatch::remove(&paths, &db, identity.game, &identity.name)?;
        }
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete(
    State(state): State<AppState>,
    Path(cluster_id): Path<String>,
) -> ApiResult<StatusCode> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        let cluster = game_instances::list_ark_clusters(&db)?
            .into_iter()
            .find(|cluster| cluster.id == cluster_id)
            .ok_or_else(|| anyhow::anyhow!("ARK cluster does not exist"))?;
        for member in cluster.members {
            let identity = game_instances::identity_by_id(&db, &member)?
                .ok_or_else(|| anyhow::anyhow!("instance does not exist"))?;
            if crate::podman::dispatch::is_running(&paths, &db, identity.game, &identity.name)? {
                return Err(crate::instance::InstanceError::AlreadyRunning(
                    "stop every affected ARK instance before deleting a cluster".into(),
                )
                .into());
            }
            crate::podman::dispatch::remove(&paths, &db, identity.game, &identity.name)?;
        }
        game_instances::delete_ark_cluster(&db, &cluster_id)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
