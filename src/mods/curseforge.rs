//! CurseForge Core API client for the ARK: Survival Ascended mod picker.
//!
//! Every request needs an operator-provided key (`x-api-key`); there is no
//! anonymous tier. Odin only uses the API for discovery — the ASA server
//! downloads the mods itself from the ids in `-mods=` — so nothing here
//! touches the download endpoints.
//!
//! Endpoint and field names were checked against the live API:
//! `GET /v1/mods/search`, `POST /v1/mods` (bulk by id) and
//! `POST /v1/mods/featured`, all scoped to ASA's game id and "Mods" class.

use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use regex::Regex;
use reqwest::blocking::RequestBuilder;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use thiserror::Error;

use crate::http::CLIENT;

const API_BASE: &str = "https://api.curseforge.com/v1";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub const ASA_GAME_ID: u64 = 83374;
pub const MODS_CLASS_ID: u64 = 6072;
pub const PAGE_SIZE: u32 = 20;
/// CurseForge's `sortField` for popularity.
const SORT_POPULARITY: &str = "2";

#[derive(Debug, Error)]
pub enum CurseForgeError {
    #[error("a CurseForge API key hasn't been configured; set [curseforge] api_key in config.toml")]
    ApiKeyMissing,
    #[error("CurseForge rejected the request (status {0}); check the configured API key")]
    Rejected(u16),
    #[error("'{0}' doesn't look like a CurseForge mod id or an ARK: Survival Ascended mod URL")]
    InvalidReference(String),
    #[error("mod '{0}' was not found on CurseForge")]
    NotFound(String),
    #[error("mod {0} is not an ARK: Survival Ascended mod")]
    WrongGame(u64),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeMod {
    pub id: u64,
    pub game_id: u64,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub download_count: f64,
    #[serde(default)]
    pub logo: Option<Logo>,
    #[serde(default)]
    pub authors: Vec<Author>,
    #[serde(default)]
    pub links: Option<Links>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Logo {
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Author {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {
    #[serde(default)]
    pub website_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Featured {
    #[serde(default)]
    pub featured: Vec<CurseForgeMod>,
    #[serde(default)]
    pub popular: Vec<CurseForgeMod>,
    #[serde(default)]
    pub recently_updated: Vec<CurseForgeMod>,
}

#[derive(Debug, Clone)]
pub struct SearchPage {
    pub mods: Vec<CurseForgeMod>,
    pub total: u64,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    data: Vec<CurseForgeMod>,
    pagination: Pagination,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    total_count: u64,
}

/// What a user may paste to identify a mod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModReference {
    Id(u64),
    /// The last segment of a `curseforge.com/ark-survival-ascended/mods/…`
    /// URL, which only the search endpoint can turn into an id.
    Slug(String),
}

/// Accepts a numeric CurseForge project id or an ASA mod page URL (with or
/// without a scheme or query string).
pub fn parse_mod_reference(input: &str) -> Result<ModReference> {
    static MOD_URL_SLUG: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)curseforge\.com/ark-survival-ascended/mods/([a-z0-9][a-z0-9_-]*)")
            .expect("valid regex")
    });

    let trimmed = input.trim();
    if !trimmed.is_empty()
        && trimmed.chars().all(|c| c.is_ascii_digit())
        && let Ok(id) = trimmed.parse::<u64>()
        && id > 0
    {
        return Ok(ModReference::Id(id));
    }
    if let Some(captures) = MOD_URL_SLUG.captures(trimmed) {
        return Ok(ModReference::Slug(captures[1].to_ascii_lowercase()));
    }
    bail!(CurseForgeError::InvalidReference(input.to_string()));
}

/// Resolves a pasted id or URL to one ASA mod.
pub fn lookup(api_key: &str, reference: &ModReference) -> Result<CurseForgeMod> {
    let found = match reference {
        ModReference::Id(id) => fetch_mods(api_key, &[*id])?.into_iter().next(),
        ModReference::Slug(slug) => {
            send::<SearchResponse>(CLIENT.get(search_url(&slug_params(slug))?), api_key)?
                .data
                .into_iter()
                .next()
        }
    };
    let label = match reference {
        ModReference::Id(id) => id.to_string(),
        ModReference::Slug(slug) => slug.clone(),
    };
    let found = found.ok_or(CurseForgeError::NotFound(label))?;
    if found.game_id != ASA_GAME_ID {
        bail!(CurseForgeError::WrongGame(found.id));
    }
    Ok(found)
}

/// `GET /v1/mods/search`, most popular first. An empty query lists everything.
pub fn search(api_key: &str, query: &str, index: u32) -> Result<SearchPage> {
    let response: SearchResponse = send(
        CLIENT.get(search_url(&search_params(query, index))?),
        api_key,
    )?;
    Ok(SearchPage {
        mods: response.data,
        total: response.pagination.total_count,
    })
}

/// `POST /v1/mods`: resolves many ids in one call. Unknown ids are dropped;
/// the rest keep the order of `ids`. Not filtered by game — `lookup` does that.
pub fn fetch_mods(api_key: &str, ids: &[u64]) -> Result<Vec<CurseForgeMod>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let response: Envelope<Vec<CurseForgeMod>> = send(
        CLIENT
            .post(format!("{API_BASE}/mods"))
            .json(&json!({ "modIds": ids })),
        api_key,
    )?;
    let mut mods = response.data;
    mods.sort_by_key(|m| ids.iter().position(|id| *id == m.id));
    Ok(mods)
}

/// `POST /v1/mods/featured`.
pub fn featured(api_key: &str) -> Result<Featured> {
    let response: Envelope<Featured> = send(
        CLIENT
            .post(format!("{API_BASE}/mods/featured"))
            .json(&json!({ "gameId": ASA_GAME_ID, "excludedModIds": [] })),
        api_key,
    )?;
    Ok(response.data)
}

fn search_url(params: &[(&'static str, String)]) -> Result<reqwest::Url> {
    reqwest::Url::parse_with_params(&format!("{API_BASE}/mods/search"), params)
        .context("failed to build the CurseForge search URL")
}

fn search_params(query: &str, index: u32) -> Vec<(&'static str, String)> {
    let mut params = vec![
        ("gameId", ASA_GAME_ID.to_string()),
        ("classId", MODS_CLASS_ID.to_string()),
        ("sortField", SORT_POPULARITY.to_string()),
        ("sortOrder", "desc".to_string()),
        ("pageSize", PAGE_SIZE.to_string()),
        ("index", index.to_string()),
    ];
    let query = query.trim();
    if !query.is_empty() {
        params.push(("searchFilter", query.to_string()));
    }
    params
}

fn slug_params(slug: &str) -> Vec<(&'static str, String)> {
    vec![
        ("gameId", ASA_GAME_ID.to_string()),
        ("classId", MODS_CLASS_ID.to_string()),
        ("slug", slug.to_string()),
    ]
}

fn send<T: DeserializeOwned>(request: RequestBuilder, api_key: &str) -> Result<T> {
    let response = request
        .header("x-api-key", api_key)
        .header("Accept", "application/json")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .context("failed to reach CurseForge")?;

    let status = response.status();
    if !status.is_success() {
        match status.as_u16() {
            code @ (401 | 403) => bail!(CurseForgeError::Rejected(code)),
            404 => bail!(CurseForgeError::NotFound(response.url().path().to_string())),
            _ => bail!("CurseForge request failed with status {status}"),
        }
    }
    response
        .json()
        .context("failed to parse the CurseForge response")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOD_JSON: &str = r#"{
        "id": 940975, "gameId": 83374, "name": "Cybers Structures QoL+ (Crossplay)",
        "slug": "cybers-structures",
        "links": {"websiteUrl": "https://www.curseforge.com/ark-survival-ascended/mods/cybers-structures", "wikiUrl": null},
        "summary": "Adding support for some Quality of Life to structures",
        "downloadCount": 29724965, "classId": 6072,
        "authors": [{"id": 1, "name": "CyberAngel", "url": "https://x"}],
        "logo": {"id": 2, "thumbnailUrl": "https://media.forgecdn.net/t.png", "url": "https://media.forgecdn.net/f.png"},
        "latestFiles": [], "screenshots": []
    }"#;

    #[test]
    fn parses_bare_numeric_ids() {
        assert_eq!(
            parse_mod_reference(" 940975 ").unwrap(),
            ModReference::Id(940975)
        );
    }

    #[test]
    fn parses_mod_page_urls_into_slugs() {
        for input in [
            "https://www.curseforge.com/ark-survival-ascended/mods/Cybers-Structures",
            "curseforge.com/ark-survival-ascended/mods/cybers-structures?tab=files",
            "https://www.curseforge.com/ark-survival-ascended/mods/cybers-structures/files/1",
        ] {
            assert_eq!(
                parse_mod_reference(input).unwrap(),
                ModReference::Slug("cybers-structures".into()),
                "{input}"
            );
        }
    }

    #[test]
    fn rejects_other_input() {
        for input in [
            "",
            "0",
            "12a",
            "99999999999999999999999",
            "https://www.curseforge.com/minecraft/mc-mods/jei",
            "structures",
        ] {
            let err = parse_mod_reference(input).unwrap_err();
            assert!(
                matches!(
                    err.downcast_ref::<CurseForgeError>(),
                    Some(CurseForgeError::InvalidReference(_))
                ),
                "{input}"
            );
        }
    }

    #[test]
    fn search_scopes_to_asa_mods_and_skips_blank_queries() {
        let params = search_params("  structures ", 40);
        let get = |key: &str| {
            params
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("gameId"), Some("83374"));
        assert_eq!(get("classId"), Some("6072"));
        assert_eq!(get("searchFilter"), Some("structures"));
        assert_eq!(get("index"), Some("40"));
        assert_eq!(get("pageSize"), Some("20"));
        assert_eq!(get("sortOrder"), Some("desc"));

        assert!(
            search_params("   ", 0)
                .iter()
                .all(|(k, _)| *k != "searchFilter")
        );

        let url = search_url(&search_params("a&b c", 0)).unwrap();
        assert!(url.as_str().contains("searchFilter=a%26b+c"), "{url}");
    }

    #[test]
    fn parses_search_responses() {
        let body = format!(
            r#"{{"data": [{MOD_JSON}], "pagination": {{"index": 0, "pageSize": 20, "resultCount": 1, "totalCount": 319}}}}"#
        );
        let response: SearchResponse = serde_json::from_str(&body).unwrap();
        assert_eq!(response.pagination.total_count, 319);
        let found = &response.data[0];
        assert_eq!(found.id, 940975);
        assert_eq!(found.game_id, ASA_GAME_ID);
        assert_eq!(found.authors[0].name, "CyberAngel");
        assert_eq!(
            found.logo.as_ref().unwrap().thumbnail_url.as_deref(),
            Some("https://media.forgecdn.net/t.png")
        );
        assert_eq!(
            found.links.as_ref().unwrap().website_url.as_deref(),
            Some("https://www.curseforge.com/ark-survival-ascended/mods/cybers-structures")
        );
    }

    #[test]
    fn parses_featured_responses_and_tolerates_missing_optional_fields() {
        let minimal = r#"{"id": 1, "gameId": 83374, "name": "Bare"}"#;
        let body = format!(
            r#"{{"data": {{"featured": [{MOD_JSON}], "popular": [{minimal}], "recentlyUpdated": []}}}}"#
        );
        let response: Envelope<Featured> = serde_json::from_str(&body).unwrap();
        assert_eq!(response.data.featured.len(), 1);
        assert_eq!(response.data.popular[0].name, "Bare");
        assert!(response.data.popular[0].authors.is_empty());
        assert!(response.data.recently_updated.is_empty());
    }

    /// Hits the real API: `CURSE_FORGE_API_KEY=… cargo test curseforge::tests::live -- --ignored`.
    #[test]
    #[ignore = "needs CURSE_FORGE_API_KEY and network access"]
    fn live_api_round_trip() {
        let key = std::env::var(crate::config::CURSEFORGE_API_KEY_ENV).expect("key in env");

        let page = search(&key, "structures", 0).unwrap();
        assert!(page.total > 0 && !page.mods.is_empty());
        assert!(page.mods.iter().all(|m| m.game_id == ASA_GAME_ID));

        let by_slug = lookup(&key, &ModReference::Slug("cybers-structures".into())).unwrap();
        assert_eq!(by_slug.id, 940975);
        let by_id = lookup(&key, &ModReference::Id(by_slug.id)).unwrap();
        assert_eq!(by_id.name, by_slug.name);

        let bulk = fetch_mods(&key, &[940975, 929420]).unwrap();
        assert_eq!(
            bulk.iter().map(|m| m.id).collect::<Vec<_>>(),
            [940975, 929420]
        );

        let featured = featured(&key).unwrap();
        assert!(!featured.popular.is_empty());

        let missing = lookup(&key, &ModReference::Slug("no-such-mod-odin-test".into()));
        assert!(matches!(
            missing.unwrap_err().downcast_ref::<CurseForgeError>(),
            Some(CurseForgeError::NotFound(_))
        ));
        let wrong_game = lookup(&key, &ModReference::Id(238222)); // JEI (Minecraft)
        assert!(matches!(
            wrong_game.unwrap_err().downcast_ref::<CurseForgeError>(),
            Some(CurseForgeError::WrongGame(_))
        ));
        let bad_key = search("not-a-key", "x", 0).unwrap_err();
        assert!(matches!(
            bad_key.downcast_ref::<CurseForgeError>(),
            Some(CurseForgeError::Rejected(403))
        ));
    }
}
