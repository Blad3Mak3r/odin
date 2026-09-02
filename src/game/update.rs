//! Steam build comparison shared by statically-supported games.

use anyhow::{Context, Result};
use serde::Serialize;

use crate::db::Db;
use crate::game::{GameId, GameModule, module};
use crate::paths::Paths;
use crate::steamcmd::{self, SteamCmd};

#[derive(Debug, Clone, Serialize)]
pub struct InstallStatus {
    pub installed_build_id: Option<u64>,
    pub latest_build_id: Option<u64>,
    pub update_available: bool,
}

pub fn check(paths: &Paths, db: &Db, game: GameId) -> Result<InstallStatus> {
    let driver = module(game);
    let installed_build_id =
        steamcmd::installed_build_id(&paths.game_install_dir(game), driver.steam_app_id());
    let latest_build_id = latest_build_id(paths, db, driver)?;
    Ok(InstallStatus {
        installed_build_id,
        latest_build_id,
        update_available: matches!((installed_build_id, latest_build_id), (Some(current), Some(latest)) if latest > current),
    })
}

fn latest_build_id(paths: &Paths, db: &Db, driver: &dyn GameModule) -> Result<Option<u64>> {
    let key = format!("{}_latest_build_id", driver.id());
    if let Some(entry) = crate::db::cache::get(db, &key)?
        && let Ok(age) = (chrono::Utc::now() - entry.fetched_at).to_std()
        && age < crate::valheim_update::CHECK_INTERVAL
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
