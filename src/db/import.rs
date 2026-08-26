//! One-time import of an existing file-based Odin installation into the
//! database. Each data domain's phase adds its own import step here, run
//! from `bootstrap_if_empty` only when that domain's table is still empty
//! — so upgrading an existing install just means running any `odin`
//! command once, no manual migration step or flag.
//!
//! The old files are deliberately left in place afterwards (not deleted or
//! renamed): they're small, and keeping them is cheap insurance against an
//! import bug.

use anyhow::Result;

use super::Db;
use crate::paths::Paths;

pub(super) fn bootstrap_if_empty(_db: &Db, _paths: &Paths) -> Result<()> {
    Ok(())
}
