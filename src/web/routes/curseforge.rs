//! CurseForge discovery for the ARK: Survival Ascended mod picker: status,
//! lookup by id/URL, featured lists, text search and bulk id resolution.
//! The API key is operator configuration (`config.toml` or the
//! `CURSE_FORGE_API_KEY` environment variable), never a dashboard setting.

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};

use crate::config::GlobalConfig;
use crate::mods::curseforge::{self, CurseForgeError, CurseForgeMod};
use crate::paths::Paths;
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

/// Mirrors the largest mod list ARK accepts (`ArkAsaOptions::validate`).
const MAX_RESOLVE_IDS: usize = 64;

#[derive(Serialize)]
pub struct CurseForgeModView {
    pub id: u64,
    pub name: String,
    pub author: String,
    pub summary: String,
    pub icon: Option<String>,
    pub downloads: u64,
    pub url: Option<String>,
}

impl From<CurseForgeMod> for CurseForgeModView {
    fn from(m: CurseForgeMod) -> Self {
        Self {
            id: m.id,
            name: m.name,
            author: m
                .authors
                .into_iter()
                .next()
                .map(|a| a.name)
                .unwrap_or_default(),
            summary: m.summary,
            icon: m.logo.and_then(|logo| logo.thumbnail_url),
            downloads: m.download_count.max(0.0) as u64,
            url: m.links.and_then(|links| links.website_url),
        }
    }
}

fn views(mods: Vec<CurseForgeMod>) -> Vec<CurseForgeModView> {
    mods.into_iter().map(Into::into).collect()
}

fn api_key(paths: &Paths) -> anyhow::Result<String> {
    GlobalConfig::load(paths)?
        .curseforge_api_key()
        .ok_or_else(|| CurseForgeError::ApiKeyMissing.into())
}

#[derive(Serialize)]
pub struct StatusView {
    pub configured: bool,
}

pub async fn status(State(state): State<AppState>) -> ApiResult<Json<StatusView>> {
    let paths = state.paths.clone();
    let configured =
        run_blocking(move || Ok(GlobalConfig::load(&paths)?.curseforge_api_key().is_some()))
            .await?;
    Ok(Json(StatusView { configured }))
}

#[derive(Deserialize)]
pub struct LookupQuery {
    pub query: String,
}

pub async fn lookup_mod(
    State(state): State<AppState>,
    Query(params): Query<LookupQuery>,
) -> ApiResult<Json<CurseForgeModView>> {
    let paths = state.paths.clone();
    let found = run_blocking(move || {
        let reference = curseforge::parse_mod_reference(&params.query)?;
        curseforge::lookup(&api_key(&paths)?, &reference)
    })
    .await?;
    Ok(Json(found.into()))
}

#[derive(Serialize)]
pub struct FeaturedView {
    pub featured: Vec<CurseForgeModView>,
    pub popular: Vec<CurseForgeModView>,
    pub recently_updated: Vec<CurseForgeModView>,
}

pub async fn featured_mods(State(state): State<AppState>) -> ApiResult<Json<FeaturedView>> {
    let paths = state.paths.clone();
    let featured = run_blocking(move || curseforge::featured(&api_key(&paths)?)).await?;
    Ok(Json(FeaturedView {
        featured: views(featured.featured),
        popular: views(featured.popular),
        recently_updated: views(featured.recently_updated),
    }))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub index: u32,
}

#[derive(Serialize)]
pub struct SearchView {
    pub mods: Vec<CurseForgeModView>,
    pub total: u64,
}

pub async fn search_mods(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> ApiResult<Json<SearchView>> {
    let paths = state.paths.clone();
    let page = run_blocking(move || curseforge::search(&api_key(&paths)?, &params.q, params.index))
        .await?;
    Ok(Json(SearchView {
        mods: views(page.mods),
        total: page.total,
    }))
}

#[derive(Deserialize)]
pub struct ResolveRequest {
    pub ids: Vec<u64>,
}

pub async fn resolve_mods(
    State(state): State<AppState>,
    Json(request): Json<ResolveRequest>,
) -> ApiResult<Json<Vec<CurseForgeModView>>> {
    if request.ids.len() > MAX_RESOLVE_IDS {
        return Err(BadRequest(format!(
            "at most {MAX_RESOLVE_IDS} ids can be resolved at once"
        ))
        .into());
    }
    let paths = state.paths.clone();
    let mods =
        run_blocking(move || curseforge::fetch_mods(&api_key(&paths)?, &request.ids)).await?;
    Ok(Json(views(mods)))
}
