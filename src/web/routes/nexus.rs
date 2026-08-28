//! Nexus Mods discovery: a "paste a mod URL/ID" lookup and a "trending on
//! Nexus" panel — v3 has no keyword-search endpoint, unlike Thunderstore.
//! Both map their results onto the existing `ModSearchResult` DTO so the
//! dashboard's install flow (search card, install-on-instances dialog) is
//! reused unchanged for a Nexus result.

use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;

use crate::mods::source;
use crate::mods::{self, nexus};
use crate::web::error::{ApiResult, run_blocking};
use crate::web::routes::mods::ModSearchResult;
use crate::web::state::AppState;

pub async fn trending_mods(
    State(_state): State<AppState>,
) -> ApiResult<Json<Vec<ModSearchResult>>> {
    // No API key needed: `/trending-mods` is a public endpoint per Nexus's
    // OpenAPI spec (`security: []`).
    let results = run_blocking(|| {
        let trending = nexus::fetch_trending()?;
        Ok(trending
            .into_iter()
            .filter_map(|m| {
                let game_scoped_id = nexus::parse_mod_reference(&m.mod_page_url).ok()?;
                Some(ModSearchResult {
                    mod_id: format!("{}{game_scoped_id}", source::NEXUS_PREFIX),
                    name: m.name,
                    owner: m.author.unwrap_or_default(),
                    // Trending entries carry no version — the actual version
                    // is only known once a file is resolved at install time.
                    version: "latest".to_string(),
                    description: m.summary.unwrap_or_default(),
                    icon: m.picture_url,
                    downloads: 0,
                })
            })
            .collect())
    })
    .await?;
    Ok(Json(results))
}

#[derive(Deserialize)]
pub struct LookupQuery {
    pub query: String,
}

pub async fn lookup_mod(
    State(state): State<AppState>,
    Query(params): Query<LookupQuery>,
) -> ApiResult<Json<ModSearchResult>> {
    let db = state.db.clone();
    let result = run_blocking(move || {
        let api_key = mods::nexus_api_key(&db)?;
        let game_scoped_id = nexus::parse_mod_reference(&params.query)?;
        let nexus_mod = nexus::fetch_mod(&api_key, &game_scoped_id)?;
        Ok(ModSearchResult {
            mod_id: format!("{}{}", source::NEXUS_PREFIX, nexus_mod.game_scoped_id),
            name: nexus_mod
                .name
                .unwrap_or_else(|| format!("Nexus mod {game_scoped_id}")),
            owner: String::new(),
            version: "latest".to_string(),
            description: String::new(),
            icon: None,
            downloads: 0,
        })
    })
    .await?;
    Ok(Json(result))
}
