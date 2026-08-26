//! Detects whether a newer Valheim dedicated server build is available on
//! Steam than what's currently installed, without running a full SteamCMD
//! `app_update` (which downloads/validates and can take minutes). Backs the
//! dashboard's "update available" indicator.
//!
//! Steam's `ISteamApps/UpToDateCheck` Web API — the usual way to compare a
//! local buildid against the live one without SteamCMD — doesn't have app
//! info for this particular app (`896660`), returning "Couldn't get app
//! info for the app specified" (verified by hand against the live API; it
//! works fine for other dedicated-server appids like `232130`). So instead
//! this shells out to `steamcmd +app_info_print`, which Steam's account-less
//! anonymous login can always fetch, and reads the `buildid` off the
//! `public` branch — the same value `installed_build_id` reads back out of
//! the ACF manifest after an `app_update` lands.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::db::Db;
use crate::paths::Paths;
use crate::steamcmd::{self, SteamCmd, VALHEIM_DEDICATED_SERVER_APP_ID};

const LATEST_BUILD_ID_CACHE_KEY: &str = "valheim_latest_build_id";
const LATEST_BUILD_ID_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub installed_build_id: Option<u64>,
    pub latest_build_id: Option<u64>,
    pub update_available: bool,
}

/// Compares the locally installed build (read from SteamCMD's ACF manifest)
/// against the current build Steam has live on the `public` branch, using a
/// cached remote lookup (see `LATEST_BUILD_ID_CACHE_TTL`) so repeated
/// dashboard polls don't re-run `steamcmd` every time.
pub fn check(paths: &Paths, db: &Db) -> Result<UpdateStatus> {
    let installed_build_id =
        steamcmd::installed_build_id(&paths.shared_install_dir(), VALHEIM_DEDICATED_SERVER_APP_ID);
    let latest_build_id = latest_build_id(paths, db)?;

    let update_available =
        matches!((installed_build_id, latest_build_id), (Some(i), Some(l)) if l > i);

    Ok(UpdateStatus {
        installed_build_id,
        latest_build_id,
        update_available,
    })
}

/// Cached lookup of the current live `public` branch buildid for the app.
fn latest_build_id(paths: &Paths, db: &Db) -> Result<Option<u64>> {
    if let Some(entry) = crate::db::cache::get(db, LATEST_BUILD_ID_CACHE_KEY)?
        && let Ok(age) = (chrono::Utc::now() - entry.fetched_at).to_std()
        && age < LATEST_BUILD_ID_CACHE_TTL
        && let Ok(build_id) = entry.value.parse::<u64>()
    {
        return Ok(Some(build_id));
    }

    let steamcmd = SteamCmd::new(paths.steamcmd_dir());
    let app_info = steamcmd
        .app_info_print(VALHEIM_DEDICATED_SERVER_APP_ID)
        .context("failed to fetch Steam app info")?;
    let build_id = public_branch_build_id(&app_info)
        .context("steamcmd app_info_print output didn't contain a public branch buildid")?;

    crate::db::cache::set(db, LATEST_BUILD_ID_CACHE_KEY, &build_id.to_string()).ok();

    Ok(Some(build_id))
}

fn public_branch_build_id(app_info_vdf: &str) -> Result<u64> {
    let branches =
        steamcmd::find_vdf_block(app_info_vdf, "branches").context("no \"branches\" block")?;
    let public =
        steamcmd::find_vdf_block(branches, "public").context("no \"public\" branch block")?;
    steamcmd::extract_quoted_u64_field(public, "buildid")
        .context("no \"buildid\" field in the \"public\" branch block")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_env(label: &str) -> (Paths, Db) {
        let dir = std::env::temp_dir().join(format!(
            "odin-valheim-update-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Db::open(&paths).unwrap();
        (paths, db)
    }

    // Network-dependent, run manually against the real steamcmd/Steam
    // backend: `cargo test -- --ignored live_check_against_real_steamcmd`.
    #[test]
    #[ignore]
    fn live_check_against_real_steamcmd() {
        let (paths, db) = temp_env("live");
        let status = check(&paths, &db).expect("check should succeed against the live backend");
        println!("{status:?}");
        assert!(status.installed_build_id.is_none());
        assert!(status.latest_build_id.unwrap() > 0);
        assert!(!status.update_available);
    }

    #[test]
    #[ignore]
    fn live_check_flags_an_outdated_install() {
        let (paths, db) = temp_env("live-outdated");
        let steamapps = paths.shared_install_dir().join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        std::fs::write(
            steamapps.join("appmanifest_896660.acf"),
            "\"AppState\"\n{\n\t\"buildid\"\t\t\"1\"\n}\n",
        )
        .unwrap();

        let status = check(&paths, &db).expect("check should succeed against the live backend");
        println!("{status:?}");
        assert_eq!(status.installed_build_id, Some(1));
        assert!(status.latest_build_id.unwrap() > 1);
        assert!(status.update_available);
    }
}
