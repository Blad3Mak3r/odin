use std::path::Path;

use anyhow::{Context, Result};

use super::thunderstore::{self, BEPINEX_MOD_ID, ModRef};
use super::{CleanupDir, effective_source_root, extract_zip_to_dir};
use crate::paths::Paths;

/// Downloads and unpacks the BepInEx pack for Valheim directly into the
/// instance directory (the pack's own layout mirrors the game's install root).
pub fn bootstrap(paths: &Paths, instance_dir: &Path) -> Result<String> {
    let index = thunderstore::fetch_index(paths)?;
    let mod_ref = ModRef::parse(BEPINEX_MOD_ID)?;
    let (_package, version) = thunderstore::resolve(&mod_ref, &index)?;

    let tmp_dir = instance_dir.join(".bepinex-install-tmp");
    std::fs::create_dir_all(&tmp_dir)?;
    let _cleanup = CleanupDir(&tmp_dir);

    let zip_path = thunderstore::download_zip(&version.download_url, &tmp_dir)?;
    let extract_dir = tmp_dir.join("extracted");
    extract_zip_to_dir(&zip_path, &extract_dir)?;

    let source_root = effective_source_root(&extract_dir)?;
    super::copy_dir_contents_excluding_metadata(&source_root, instance_dir)
        .context("failed to install BepInEx pack into instance directory")?;

    Ok(version.version_number.clone())
}
