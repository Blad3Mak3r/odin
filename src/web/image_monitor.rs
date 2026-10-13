//! Watches the registry for new builds of Odin's moving runtime-image tags.
//!
//! Only the default `odin-srv:base` / `:proton` references are checked; any
//! other reference (immutable tag, digest, other registry) is pinned and never
//! contacts a registry. The monitor only records the remote digest — pulling
//! stays an explicit operator action.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::config::GlobalConfig;
use crate::db::{Db, cache};
use crate::game::RuntimeImage;
use crate::paths::Paths;
use crate::web::state::AppState;

const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const REGISTRY: &str = "ghcr.io";
const REPOSITORY: &str = "blad3mak3r/odin-srv";
const MANIFEST_ACCEPT: &str = "application/vnd.oci.image.index.v1+json, \
    application/vnd.docker.distribution.manifest.list.v2+json, \
    application/vnd.oci.image.manifest.v1+json, \
    application/vnd.docker.distribution.manifest.v2+json";

/// Latest known registry state for one reference.
pub struct RemoteDigest {
    pub digest: String,
    pub checked_at: DateTime<Utc>,
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            let tick_state = state.clone();
            let result = tokio::task::spawn_blocking(move || {
                check(&tick_state.paths, &tick_state.db, false)
            })
            .await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(%error, "runtime image check failed"),
                Err(error) => tracing::warn!(%error, "runtime image check task failed"),
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

/// Checks every moving reference. Without `force`, references checked within
/// the last interval are skipped, so restarting Odin does not hit the registry.
/// A failure on one image does not stop the others; the first error is returned.
pub fn check(paths: &Paths, db: &Db, force: bool) -> Result<()> {
    let Some(podman) = GlobalConfig::load(paths)?.podman else {
        return Ok(());
    };
    let mut first_error = None;
    for role in RuntimeImage::all() {
        let reference = podman.image(role);
        let Some(tag) = moving_tag(&reference) else {
            continue;
        };
        if !force && !is_stale(cached(db, &reference)?.map(|entry| entry.checked_at)) {
            continue;
        }
        let result = fetch_digest(tag).and_then(|digest| {
            cache::set(db, &cache_key(&reference), &digest)?;
            Ok(())
        });
        if let Err(error) = result {
            tracing::warn!(%reference, %error, "failed to check runtime image");
            first_error.get_or_insert(error);
        }
    }
    first_error.map_or(Ok(()), Err)
}

pub fn cached(db: &Db, reference: &str) -> Result<Option<RemoteDigest>> {
    Ok(
        cache::get(db, &cache_key(reference))?.map(|entry| RemoteDigest {
            digest: entry.value,
            checked_at: entry.fetched_at,
        }),
    )
}

fn cache_key(reference: &str) -> String {
    format!("runtime_image_digest:{reference}")
}

/// Returns the tag when `reference` is one of Odin's moving default tags.
pub fn moving_tag(reference: &str) -> Option<&str> {
    let tag = reference.strip_prefix(&format!("{REGISTRY}/{REPOSITORY}:"))?;
    RuntimeImage::all()
        .iter()
        .any(|role| role.as_str() == tag)
        .then_some(tag)
}

fn is_stale(checked_at: Option<DateTime<Utc>>) -> bool {
    checked_at
        .is_none_or(|at| Utc::now() - at >= chrono::Duration::from_std(CHECK_INTERVAL).unwrap())
}

/// True when the registry digest is not among the local image's repo digests.
/// An image that is not pulled yet is not an "update".
pub fn update_available(local_digests: &[String], remote: &str) -> bool {
    !local_digests.is_empty()
        && !local_digests
            .iter()
            .any(|digest| digest.rsplit('@').next() == Some(remote))
}

#[derive(Deserialize)]
struct TokenResponse {
    token: String,
}

fn fetch_digest(tag: &str) -> Result<String> {
    let client = &*crate::http::CLIENT;
    let token: TokenResponse = client
        .get(format!(
            "https://{REGISTRY}/token?service={REGISTRY}&scope=repository:{REPOSITORY}:pull"
        ))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .and_then(|response| response.error_for_status())
        .context("failed to obtain a registry token")?
        .json()
        .context("invalid registry token response")?;
    let response = client
        .head(format!(
            "https://{REGISTRY}/v2/{REPOSITORY}/manifests/{tag}"
        ))
        .bearer_auth(token.token)
        .header(reqwest::header::ACCEPT, MANIFEST_ACCEPT)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .and_then(|response| response.error_for_status())
        .with_context(|| format!("failed to read the manifest for {REPOSITORY}:{tag}"))?;
    let digest = response
        .headers()
        .get("docker-content-digest")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    match digest {
        Some(digest) if digest.starts_with("sha256:") => Ok(digest),
        _ => bail!("registry did not return a Docker-Content-Digest for {tag}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_default_moving_tags_are_monitored() {
        assert_eq!(moving_tag("ghcr.io/blad3mak3r/odin-srv:base"), Some("base"));
        assert_eq!(
            moving_tag("ghcr.io/blad3mak3r/odin-srv:proton"),
            Some("proton")
        );
    }

    #[test]
    fn pinned_references_are_skipped() {
        for reference in [
            "ghcr.io/blad3mak3r/odin-srv:v1.2.0",
            "ghcr.io/blad3mak3r/odin-srv@sha256:abc",
            "ghcr.io/blad3mak3r/odin-srv",
            "example/odin-srv:base",
            "ghcr.io/other/odin-srv:base",
        ] {
            assert_eq!(moving_tag(reference), None, "{reference}");
        }
    }

    #[test]
    fn check_ttl_expires_after_the_interval() {
        let now = Utc::now();
        assert!(is_stale(None));
        assert!(!is_stale(Some(now - chrono::Duration::hours(5))));
        assert!(is_stale(Some(now - chrono::Duration::hours(6))));
    }

    #[test]
    fn digest_comparison_matches_any_local_repo_digest() {
        let local = vec![
            "ghcr.io/blad3mak3r/odin-srv@sha256:old".to_string(),
            "ghcr.io/blad3mak3r/odin-srv@sha256:new".to_string(),
        ];
        assert!(!update_available(&local, "sha256:new"));
        assert!(update_available(&local, "sha256:newer"));
        assert!(!update_available(&[], "sha256:new"));
    }
}
