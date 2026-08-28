//! Nexus Mods v3 API client, mirroring `thunderstore.rs`'s shape. Details
//! below were confirmed by downloading the real OpenAPI spec from
//! `https://api.nexusmods.com/openapi.yaml` (linked from
//! `https://api-docs.nexusmods.com/`) rather than guessed — the rendered
//! docs page alone truncates most response examples.
//!
//! Unlike Thunderstore, v3 has no keyword-search endpoint: discovery is
//! limited to [`fetch_trending`] (public, top 5 per game) and [`fetch_mod`]
//! (look up a mod by the id/URL a user pastes in). There's also no bulk
//! index to cache — every call hits the network.
//!
//! Downloading a file is a multi-hop chain
//! (`/mods/{id}/files` -> `/mod-files/{id}/versions` ->
//! `/mod-file-versions/{id}/download-repacked`), isolated in
//! [`resolve_download`]. Whether that last, `Experimental`-flagged endpoint
//! works for a non-Premium personal API key is unconfirmed by the docs; any
//! rejection there is surfaced as [`NexusError::DownloadUnavailable`] so
//! callers can point the user at the manual-upload fallback instead of a
//! bare failure.

use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::http::CLIENT;

const NEXUS_API_BASE: &str = "https://api.nexusmods.com/v3";
pub const GAME_DOMAIN: &str = "valheim";

#[derive(Debug, Error)]
pub enum NexusError {
    #[error("a Nexus Mods API key hasn't been configured; add one from the Settings page")]
    ApiKeyMissing,
    #[error("Nexus Mods rejected the configured API key")]
    Unauthorized,
    #[error("'{0}' doesn't look like a Nexus Mods mod URL or id")]
    InvalidReference(String),
    #[error("mod '{0}' was not found on Nexus Mods")]
    ModNotFound(String),
    #[error(
        "'{0}' can't be downloaded automatically from Nexus Mods right now; \
         download the file manually from the mod's page and upload the .zip instead"
    )]
    DownloadUnavailable(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct NexusMod {
    /// Nexus's internal mod id — used for the `/mods/{id}/files` lookup, not
    /// the same as `game_scoped_id`.
    pub id: String,
    /// The id that appears in the mod's URL, e.g. `1234` from
    /// `nexusmods.com/valheim/mods/1234` — this is what Odin's own
    /// `nexus:{game_scoped_id}` mod ids are built from.
    pub game_scoped_id: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrendingMod {
    pub name: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub picture_url: Option<String>,
    /// The trending feed carries no mod/game-scoped id at all — only this
    /// canonical page URL, which `parse_mod_reference` can extract an id
    /// from just like a user-pasted URL.
    pub mod_page_url: String,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Deserialize)]
struct TrendingModsResponse {
    mods: Vec<TrendingMod>,
}

#[derive(Deserialize)]
struct ModFileWithAggregates {
    id: String,
    #[serde(default)]
    is_active: bool,
}

#[derive(Deserialize)]
struct ModFilesResponse {
    mod_files: Vec<ModFileWithAggregates>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModFileVersion {
    id: String,
    version: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    is_primary: bool,
    /// Kept as a raw ISO-8601 string rather than parsed into a `DateTime` —
    /// same-format timestamps still sort correctly as strings, and this
    /// avoids a hard parse failure over a field only used for ranking.
    #[serde(default)]
    uploaded_at: String,
}

#[derive(Deserialize)]
struct ModFileVersionsResponse {
    versions: Vec<ModFileVersion>,
}

#[derive(Deserialize)]
struct DownloadRepackedResponse {
    download_url: String,
}

/// Accepts either a bare numeric mod id or a Nexus mod page URL (any game
/// domain, with or without a scheme/query string — e.g.
/// `https://www.nexusmods.com/valheim/mods/1234?tab=files`) and returns the
/// `game_scoped_id`.
pub fn parse_mod_reference(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Ok(trimmed.to_string());
    }

    static MOD_URL_ID: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"/mods/(\d+)").expect("valid regex"));
    if let Some(captures) = MOD_URL_ID.captures(trimmed) {
        return Ok(captures[1].to_string());
    }

    bail!(NexusError::InvalidReference(input.to_string()));
}

/// `GET /games/{game_domain}/trending-mods` — public, no API key required.
pub fn fetch_trending() -> Result<Vec<TrendingMod>> {
    let url = format!("{NEXUS_API_BASE}/games/{GAME_DOMAIN}/trending-mods");
    let response: TrendingModsResponse = get_envelope(&url, None)?;
    Ok(response.mods)
}

/// `GET /games/{game_domain}/mods/{game_scoped_id}`.
pub fn fetch_mod(api_key: &str, game_scoped_id: &str) -> Result<NexusMod> {
    let url = format!("{NEXUS_API_BASE}/games/{GAME_DOMAIN}/mods/{game_scoped_id}");
    get_envelope(&url, Some(api_key))
}

/// Resolves a mod to a downloadable file: walks every active mod file's
/// versions, picks the newest `main`-category one (preferring whichever is
/// flagged `is_primary`), and requests its repacked-archive download link.
/// Returns `(download_url, version_string)`.
pub fn resolve_download(api_key: &str, mod_internal_id: &str) -> Result<(String, String)> {
    let files: ModFilesResponse = get_envelope(
        &format!("{NEXUS_API_BASE}/mods/{mod_internal_id}/files"),
        Some(api_key),
    )?;

    let mut candidates: Vec<ModFileVersion> = Vec::new();
    for file in files.mod_files.into_iter().filter(|f| f.is_active) {
        let versions: ModFileVersionsResponse = get_envelope(
            &format!("{NEXUS_API_BASE}/mod-files/{}/versions", file.id),
            Some(api_key),
        )?;
        candidates.extend(
            versions
                .versions
                .into_iter()
                .filter(|v| v.category == "main"),
        );
    }

    candidates.sort_by(|a, b| {
        b.is_primary
            .cmp(&a.is_primary)
            .then_with(|| b.uploaded_at.cmp(&a.uploaded_at))
    });
    let chosen = candidates
        .into_iter()
        .next()
        .ok_or_else(|| NexusError::DownloadUnavailable(mod_internal_id.to_string()))?;

    let download = download_repacked(api_key, &chosen.id)?;
    Ok((download.download_url, chosen.version))
}

fn download_repacked(api_key: &str, version_id: &str) -> Result<DownloadRepackedResponse> {
    let url = format!("{NEXUS_API_BASE}/mod-file-versions/{version_id}/download-repacked");
    let response = CLIENT
        .post(&url)
        .header("apikey", api_key)
        .header("Application-Name", "Odin")
        .send()
        .with_context(|| format!("failed to reach {url}"))?;

    let status = response.status();
    if status.is_success() {
        return response
            .json()
            .with_context(|| format!("failed to parse response from {url}"));
    }
    if status.as_u16() == 401 {
        bail!(NexusError::Unauthorized);
    }
    // Nexus doesn't document exactly which membership tiers can use this
    // Experimental endpoint, so a 403 (or any other unexpected status) here
    // is treated the same as "not downloadable right now" rather than a
    // hard failure, surfacing the manual-upload fallback to the caller.
    bail!(NexusError::DownloadUnavailable(version_id.to_string()));
}

fn get_envelope<T: DeserializeOwned>(url: &str, api_key: Option<&str>) -> Result<T> {
    let mut request = CLIENT.get(url);
    if let Some(key) = api_key {
        request = request.header("apikey", key);
    }
    let response = request
        .send()
        .with_context(|| format!("failed to reach {url}"))?;

    let status = response.status();
    if !status.is_success() {
        match status.as_u16() {
            401 | 403 => bail!(NexusError::Unauthorized),
            404 => bail!(NexusError::ModNotFound(url.to_string())),
            _ => bail!("Nexus Mods request to {url} failed with status {status}"),
        }
    }

    let envelope: Envelope<T> = response
        .json()
        .with_context(|| format!("failed to parse response from {url}"))?;
    Ok(envelope.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_numeric_id() {
        assert_eq!(parse_mod_reference("1234").unwrap(), "1234");
        assert_eq!(parse_mod_reference("  1234  ").unwrap(), "1234");
    }

    #[test]
    fn parses_full_mod_page_url() {
        assert_eq!(
            parse_mod_reference("https://www.nexusmods.com/valheim/mods/1234").unwrap(),
            "1234"
        );
    }

    #[test]
    fn parses_url_with_query_string() {
        assert_eq!(
            parse_mod_reference("https://www.nexusmods.com/valheim/mods/5678?tab=files").unwrap(),
            "5678"
        );
    }

    #[test]
    fn rejects_garbage_input() {
        assert!(parse_mod_reference("not a mod reference").is_err());
        assert!(parse_mod_reference("").is_err());
        assert!(parse_mod_reference("https://www.nexusmods.com/valheim").is_err());
    }
}
