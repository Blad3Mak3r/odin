//! Detects whether a newer Odin release is published on GitHub than the
//! version currently running, the same way `valheim_update` does for the
//! Valheim server binary itself. Backs the dashboard's "update available"
//! banner.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::db::Db;

const REPO: &str = "Blad3Mak3r/odin";
const LATEST_RELEASE_CACHE_KEY: &str = "odin_latest_release";
const LATEST_RELEASE_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub current_version: &'static str,
    pub latest_version: Option<String>,
    pub latest_release_url: Option<String>,
    pub update_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GithubRelease {
    tag: String,
    html_url: String,
}

/// Compares the running build's version against the latest GitHub release
/// tag (`vX.Y.Z`), using a cached remote lookup (see
/// `LATEST_RELEASE_CACHE_TTL`) so repeated dashboard polls don't hit the
/// GitHub API every time.
pub fn check(db: &Db) -> Result<UpdateStatus> {
    let current_version = env!("CARGO_PKG_VERSION");
    let release = latest_release(db)?;

    let update_available = release
        .as_ref()
        .is_some_and(|r| is_newer(&r.tag, current_version));

    Ok(UpdateStatus {
        current_version,
        latest_version: release.as_ref().map(|r| r.tag.clone()),
        latest_release_url: release.map(|r| r.html_url),
        update_available,
    })
}

/// Cached lookup of the latest published GitHub release for this repo.
/// Returns `Ok(None)` (rather than an error) when the repo has no releases
/// yet — a 404 from GitHub is expected until the first tag is published,
/// not a failure — and that "no releases" result is cached too, so an
/// unreleased build doesn't hit the GitHub API on every poll either.
fn latest_release(db: &Db) -> Result<Option<GithubRelease>> {
    if let Some(entry) = crate::db::cache::get(db, LATEST_RELEASE_CACHE_KEY)?
        && let Ok(age) = (chrono::Utc::now() - entry.fetched_at).to_std()
        && age < LATEST_RELEASE_CACHE_TTL
        && let Ok(cached) = serde_json::from_str::<Option<GithubRelease>>(&entry.value)
    {
        return Ok(cached);
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("odin/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(10))
        .build()
        .context("failed to build GitHub release check client")?;

    let response = client
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .send()
        .context("failed to reach GitHub releases API")?;

    let release = if response.status() == reqwest::StatusCode::NOT_FOUND {
        None
    } else {
        #[derive(Deserialize)]
        struct RawRelease {
            tag_name: String,
            html_url: String,
        }
        let raw: RawRelease = response
            .error_for_status()
            .context("GitHub releases API returned an error status")?
            .json()
            .context("failed to parse GitHub releases API response")?;
        Some(GithubRelease {
            tag: raw.tag_name,
            html_url: raw.html_url,
        })
    };

    if let Ok(cached) = serde_json::to_string(&release) {
        crate::db::cache::set(db, LATEST_RELEASE_CACHE_KEY, &cached).ok();
    }

    Ok(release)
}

/// True if `tag` (e.g. "v0.4.0") is strictly newer than `current`.
fn is_newer(tag: &str, current: &str) -> bool {
    fn parse(v: &str) -> Option<(u64, u64, u64)> {
        let mut parts = v.strip_prefix('v').unwrap_or(v).split('.');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ))
    }
    matches!((parse(tag), parse(current)), (Some(a), Some(b)) if a > b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_detects_a_higher_patch_version() {
        assert!(is_newer("v0.4.1", "0.4.0"));
    }

    #[test]
    fn is_newer_detects_a_higher_minor_or_major_version() {
        assert!(is_newer("v0.5.0", "0.4.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
    }

    #[test]
    fn is_newer_rejects_an_equal_or_lower_version() {
        assert!(!is_newer("v0.4.0", "0.4.0"));
        assert!(!is_newer("v0.3.9", "0.4.0"));
    }

    #[test]
    fn is_newer_rejects_malformed_tags() {
        assert!(!is_newer("not-a-version", "0.4.0"));
        assert!(!is_newer("v0.4", "0.4.0"));
    }

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-update-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Db::open(&crate::paths::Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        })
        .unwrap()
    }

    // Network-dependent, run manually against the real GitHub API:
    // `cargo test -- --ignored live_check_against_github`. `Blad3Mak3r/odin`
    // has no releases published yet as of writing, so this currently
    // exercises the "no releases" (404) path; once the release workflow
    // publishes a `vX.Y.Z` tag this will start reporting it.
    #[test]
    #[ignore]
    fn live_check_against_github() {
        let db = temp_db("live");
        let status = check(&db).expect("check should succeed against the live GitHub API");
        println!("{status:?}");
        assert_eq!(status.current_version, env!("CARGO_PKG_VERSION"));
    }
}
