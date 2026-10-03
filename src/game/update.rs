//! Steam build comparison shared by statically-supported games.

use anyhow::{Context, Result};
use serde::Serialize;
use std::time::Duration;

use crate::db::Db;
use crate::game::{GameDriver, GameId, driver};
use crate::paths::Paths;
use crate::steamcmd::{self, SteamCmd};

pub const CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Serialize)]
pub struct InstallStatus {
    pub installed_build_id: Option<u64>,
    pub latest_build_id: Option<u64>,
    pub update_available: bool,
}

pub fn check(paths: &Paths, db: &Db, game: GameId) -> Result<InstallStatus> {
    let driver = driver(game);
    let installed_build_id =
        steamcmd::installed_build_id(&paths.game_install_dir(game), driver.steam_app_id());
    let latest_build_id = latest_build_id(paths, db, driver)?;
    Ok(InstallStatus {
        installed_build_id,
        latest_build_id,
        update_available: matches!((installed_build_id, latest_build_id), (Some(current), Some(latest)) if latest > current),
    })
}

fn latest_build_id(paths: &Paths, db: &Db, driver: &dyn GameDriver) -> Result<Option<u64>> {
    let key = format!("{}_latest_build_id", driver.id());
    if let Some(entry) = crate::db::cache::get(db, &key)?
        && let Ok(age) = (chrono::Utc::now() - entry.fetched_at).to_std()
        && age < CHECK_INTERVAL
        && let Ok(build_id) = entry.value.parse()
    {
        return Ok(Some(build_id));
    }

    let app_info = SteamCmd::new(paths.steamcmd_dir()).app_info_print(driver.steam_app_id())?;
    let branches = steamcmd::find_vdf_block(&app_info, "branches").context("no branches block")?;
    let public = steamcmd::find_vdf_block(branches, "public").context("no public branch")?;
    let build_id =
        steamcmd::extract_quoted_u64_field(public, "buildid").context("no public build id")?;
    crate::db::cache::set(db, &key, &build_id.to_string()).ok();
    Ok(Some(build_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_env(label: &str) -> (Paths, Db) {
        let dir = std::env::temp_dir().join(format!(
            "odin-game-update-test-{label}-{}-{}",
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

    // Network-dependent, run manually against the real Steam backend.
    #[test]
    #[ignore]
    fn live_check_against_real_steamcmd() {
        let (paths, db) = temp_env("live");
        let status = check(&paths, &db, GameId::Valheim)
            .expect("check should succeed against the live backend");
        assert!(status.installed_build_id.is_none());
        assert!(status.latest_build_id.unwrap() > 0);
        assert!(!status.update_available);
    }

    #[test]
    #[ignore]
    fn live_check_flags_an_outdated_install() {
        let (paths, db) = temp_env("live-outdated");
        let steamapps = paths.game_install_dir(GameId::Valheim).join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        std::fs::write(
            steamapps.join("appmanifest_896660.acf"),
            "\"AppState\"\n{\n\t\"buildid\"\t\t\"1\"\n}\n",
        )
        .unwrap();

        let status = check(&paths, &db, GameId::Valheim)
            .expect("check should succeed against the live backend");
        assert_eq!(status.installed_build_id, Some(1));
        assert!(status.latest_build_id.unwrap() > 1);
        assert!(status.update_available);
    }
}
