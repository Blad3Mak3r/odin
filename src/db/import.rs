//! One-time import of an existing file-based Odin installation into the
//! database. Each data domain's phase adds its own import step here, run
//! from `bootstrap_if_empty` only when that domain's table is still empty
//! — so upgrading an existing install just means running any `odin`
//! command once, no manual migration step or flag.
//!
//! The old files are deliberately left in place afterwards (not deleted or
//! renamed): they're small, and keeping them is cheap insurance against an
//! import bug.

use anyhow::{Context, Result};

use super::Db;
use crate::instance::state::InstanceState;
use crate::paths::{self, Paths};

pub(super) fn bootstrap_if_empty(db: &Db, paths: &Paths) -> Result<()> {
    import_instances(db, paths)
}

/// Imports every `servers/<name>/state.json` found on disk, if the
/// `instances` table is still empty. A directory with an unreadable or
/// corrupt state file is logged and skipped rather than aborting the
/// import.
fn import_instances(db: &Db, paths: &Paths) -> Result<()> {
    let already_populated: bool =
        db.conn()
            .query_row("SELECT EXISTS(SELECT 1 FROM instances)", [], |row| {
                row.get(0)
            })?;
    if already_populated {
        return Ok(());
    }

    let servers_dir = paths.servers_dir();
    if !servers_dir.is_dir() {
        return Ok(());
    }

    let entries = std::fs::read_dir(&servers_dir)
        .with_context(|| format!("failed to read {}", servers_dir.display()))?;

    let mut states = Vec::new();
    for entry in entries {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", servers_dir.display()))?;
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let state_file = paths::instance_state_file(&entry.path());
        if !state_file.is_file() {
            continue;
        }
        match InstanceState::load_from_file(&state_file) {
            Ok(state) => states.push(state),
            Err(error) => {
                tracing::warn!(
                    path = %state_file.display(),
                    %error,
                    "skipping unreadable/invalid state file during database import"
                );
            }
        }
    }

    if states.is_empty() {
        return Ok(());
    }

    let count = states.len();
    let mut conn = db.conn();
    let tx = conn
        .transaction()
        .context("failed to start import transaction")?;
    for state in &states {
        super::instances::save_in_tx(&tx, state)?;
    }
    tx.commit().context("failed to commit imported instances")?;

    tracing::info!(count, "imported existing instances into the database");
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::instance::state::InstalledMod;

    fn temp_paths(label: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-import-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        }
    }

    fn write_legacy_state_json(paths: &Paths, name: &str) {
        let instance_dir = paths.servers_dir().join(name);
        std::fs::create_dir_all(&instance_dir).unwrap();

        let mut state = InstanceState::new(name, 2456);
        state.installed_mods.push(InstalledMod {
            mod_id: "owner-mod".to_string(),
            version: "1.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: true,
        });
        let raw = serde_json::to_string_pretty(&state).unwrap();
        std::fs::write(paths::instance_state_file(&instance_dir), raw).unwrap();
    }

    #[test]
    fn opening_a_fresh_db_imports_legacy_state_json_files() {
        let paths = temp_paths("import");
        write_legacy_state_json(&paths, "my-server");

        let db = Db::open(&paths).unwrap();

        let imported = crate::db::instances::load(&db, "my-server")
            .unwrap()
            .expect("instance should have been imported");
        assert_eq!(imported.port, 2456);
        assert_eq!(imported.installed_mods.len(), 1);
        assert_eq!(imported.installed_mods[0].mod_id, "owner-mod");
    }

    #[test]
    fn reopening_an_already_imported_db_does_not_duplicate() {
        let paths = temp_paths("reimport");
        write_legacy_state_json(&paths, "my-server");

        {
            let _db = Db::open(&paths).unwrap();
        }
        let db = Db::open(&paths).unwrap();

        let count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM instances", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
