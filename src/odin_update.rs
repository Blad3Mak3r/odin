//! Detects whether a newer Odin release is published on GitHub than the
//! version currently running, the same way `valheim_update` does for the
//! Valheim server binary itself. Backs the dashboard's "update available"
//! banner.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::db::Db;

const REPO: &str = "Blad3Mak3r/odin";
const LATEST_RELEASE_CACHE_KEY: &str = "odin_latest_release";
const LATEST_RELEASE_CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const SYSTEM_UPDATE_UNIT: &str = "odin-update.service";
const SYSTEM_UPDATE_UNIT_PATH: &str = "/usr/lib/systemd/system/odin-update.service";

#[derive(Debug, Clone, Copy, Serialize)]
pub struct SystemUpdateStatus {
    pub supported: bool,
    pub running: bool,
}

#[derive(Debug, Error)]
pub enum SystemUpdateError {
    #[error("Odin dashboard updates are available only for .deb/.rpm package installs")]
    Unsupported,
    #[error("An Odin update is already running")]
    AlreadyRunning,
}

#[derive(Debug, Clone)]
pub struct SystemUpdate {
    unit_path: PathBuf,
    systemctl_path: PathBuf,
}

impl SystemUpdate {
    pub fn packaged() -> Self {
        Self {
            unit_path: PathBuf::from(SYSTEM_UPDATE_UNIT_PATH),
            systemctl_path: PathBuf::from("systemctl"),
        }
    }

    #[cfg(test)]
    pub(crate) fn new(unit_path: PathBuf, systemctl_path: PathBuf) -> Self {
        Self {
            unit_path,
            systemctl_path,
        }
    }

    pub fn status(&self) -> Result<SystemUpdateStatus> {
        if !self.unit_path.is_file() {
            return Ok(SystemUpdateStatus {
                supported: false,
                running: false,
            });
        }
        let output = Command::new(&self.systemctl_path)
            .args(["is-active", "--quiet", SYSTEM_UPDATE_UNIT])
            .output()
            .context("failed to query Odin update service")?;
        Ok(SystemUpdateStatus {
            supported: true,
            running: output.status.success(),
        })
    }

    pub fn start(&self) -> Result<()> {
        let status = self.status()?;
        if !status.supported {
            return Err(SystemUpdateError::Unsupported.into());
        }
        if status.running {
            return Err(SystemUpdateError::AlreadyRunning.into());
        }
        let status = Command::new(&self.systemctl_path)
            .args(["start", "--no-block", SYSTEM_UPDATE_UNIT])
            .status()
            .context("failed to start Odin update service")?;
        if !status.success() {
            bail!("Odin update service could not be started");
        }
        Ok(())
    }
}

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

    let response = crate::http::CLIENT
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .header(
            reqwest::header::USER_AGENT,
            concat!("odin/", env!("CARGO_PKG_VERSION")),
        )
        .timeout(Duration::from_secs(10))
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
pub(crate) fn is_newer(tag: &str, current: &str) -> bool {
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
    use std::os::unix::fs::PermissionsExt;

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

    fn fake_systemctl(label: &str, active: bool) -> (PathBuf, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "odin-system-update-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let unit = dir.join("odin-update.service");
        let marker = dir.join("args");
        let systemctl = dir.join("systemctl");
        std::fs::write(&unit, "[Service]\n").unwrap();
        std::fs::write(
            &systemctl,
            format!(
                "#!/bin/sh\nif [ \"$1\" = is-active ]; then exit {}; fi\nprintf '%s\\n' \"$@\" > '{}'\n",
                if active { 0 } else { 1 },
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&systemctl, std::fs::Permissions::from_mode(0o755)).unwrap();
        (unit, systemctl, marker)
    }

    #[test]
    fn packaged_updater_is_unsupported_without_its_unit() {
        let dir = std::env::temp_dir().join(format!(
            "odin-system-update-missing-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let updater = SystemUpdate::new(dir.join("odin-update.service"), PathBuf::from("false"));

        assert!(!updater.status().unwrap().supported);
    }

    #[test]
    fn packaged_updater_starts_the_update_unit_without_waiting() {
        let (unit, systemctl, marker) = fake_systemctl("start", false);
        let updater = SystemUpdate::new(unit, systemctl);

        updater.start().unwrap();

        assert_eq!(
            std::fs::read_to_string(marker).unwrap(),
            "start\n--no-block\nodin-update.service\n"
        );
    }

    #[test]
    fn packaged_updater_rejects_a_running_update() {
        let (unit, systemctl, _) = fake_systemctl("running", true);
        let updater = SystemUpdate::new(unit, systemctl);

        assert!(matches!(
            updater
                .start()
                .unwrap_err()
                .downcast_ref::<SystemUpdateError>(),
            Some(SystemUpdateError::AlreadyRunning)
        ));
    }

    #[test]
    fn package_metadata_includes_the_updater_assets() {
        let manifest = include_str!("../Cargo.toml");

        assert_eq!(manifest.matches("packaging/odin-update.service").count(), 2);
        assert_eq!(manifest.matches("[\"install.sh\"").count(), 1);
        assert_eq!(manifest.matches("source = \"install.sh\"").count(), 1);
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
