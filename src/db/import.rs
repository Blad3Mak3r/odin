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
    import_instances(db, paths)?;
    import_backups(db, paths)?;
    import_global_mod_versions(db, paths)?;
    import_access_lists(db, paths)
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

/// Imports every instance's `backups/*.zip` metadata found on disk, if the
/// `backups` table is still empty. Only runs once instances exist in the
/// database (backups reference them via a foreign key), so this always
/// follows `import_instances`.
fn import_backups(db: &Db, paths: &Paths) -> Result<()> {
    let already_populated: bool =
        db.conn()
            .query_row("SELECT EXISTS(SELECT 1 FROM backups)", [], |row| row.get(0))?;
    if already_populated {
        return Ok(());
    }

    let instances = super::instances::list_all(db)?;
    let mut count = 0;
    for state in &instances {
        let instance_dir = paths.instance_dir(&state.name);
        let entries = crate::backup::list_from_disk(&instance_dir)?;
        for entry in entries {
            super::backups::insert(db, &state.name, &entry)?;
            count += 1;
        }
    }

    if count > 0 {
        tracing::info!(count, "imported existing backup metadata into the database");
    }
    Ok(())
}

/// Imports every `.odin-version` marker file found under the global mod
/// store, if the `global_mods` table is still empty — otherwise a fresh
/// upgrade would forget which version of each already-installed mod is on
/// disk and re-download all of them on the next `mods update`.
fn import_global_mod_versions(db: &Db, paths: &Paths) -> Result<()> {
    let already_populated: bool =
        db.conn()
            .query_row("SELECT EXISTS(SELECT 1 FROM global_mods)", [], |row| {
                row.get(0)
            })?;
    if already_populated {
        return Ok(());
    }

    let mods_dir = paths.mods_dir();
    if !mods_dir.is_dir() {
        return Ok(());
    }

    let mut count = 0;
    for entry in std::fs::read_dir(&mods_dir)
        .with_context(|| format!("failed to read {}", mods_dir.display()))?
    {
        let entry = entry?;
        let mod_id = entry.file_name().to_string_lossy().into_owned();
        if mod_id.starts_with('.') || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(marker) = std::fs::read_to_string(entry.path().join(".odin-version")) else {
            continue;
        };
        super::global_mods::set_version(db, &mod_id, marker.trim())?;
        count += 1;
    }

    if count > 0 {
        tracing::info!(
            count,
            "imported existing global mod version markers into the database"
        );
    }
    Ok(())
}

/// Imports every instance's `adminlist.txt`/`bannedlist.txt`/
/// `permittedlist.txt` found on disk, if the `access_list_entries` table is
/// still empty. An id that fails Odin's own SteamID64 validation (possible
/// for a hand-edited file — Odin itself never wrote an invalid one) is
/// logged and skipped rather than aborting the import; the file gets
/// regenerated to match on the next write made through Odin.
fn import_access_lists(db: &Db, paths: &Paths) -> Result<()> {
    use crate::instance::lists::ListKind;

    let already_populated: bool = db.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM access_list_entries)",
        [],
        |row| row.get(0),
    )?;
    if already_populated {
        return Ok(());
    }

    let instances = super::instances::list_all(db)?;
    let mut count = 0;
    for state in &instances {
        let instance_dir = paths.instance_dir(&state.name);
        for kind in [ListKind::Admin, ListKind::Banned, ListKind::Permitted] {
            let ids = crate::instance::lists::read_from_disk(&instance_dir, kind)?;
            for id in ids {
                if crate::instance::lists::validate_steam_id64(&id).is_err() {
                    tracing::warn!(
                        instance = %state.name,
                        id,
                        "skipping invalid SteamID64 while importing access list"
                    );
                    continue;
                }
                super::lists::insert(db, &state.name, kind.db_value(), &id)?;
                count += 1;
            }
        }
    }

    if count > 0 {
        tracing::info!(
            count,
            "imported existing access list entries into the database"
        );
    }
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

    #[test]
    fn opening_a_fresh_db_imports_legacy_backup_zips() {
        let paths = temp_paths("import-backups");
        write_legacy_state_json(&paths, "my-server");
        let backups_dir = paths.instance_dir("my-server").join("backups");
        std::fs::create_dir_all(&backups_dir).unwrap();
        std::fs::write(backups_dir.join("20260101T000000Z.zip"), b"fake zip").unwrap();

        let db = Db::open(&paths).unwrap();

        let entries = crate::db::backups::list(&db, "my-server").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "20260101T000000Z");
        assert_eq!(entries[0].size_bytes, 8);
    }

    #[test]
    fn opening_a_fresh_db_imports_legacy_version_markers() {
        let paths = temp_paths("import-global-mods");
        let mod_dir = paths.mod_dir("owner-mod");
        std::fs::create_dir_all(&mod_dir).unwrap();
        std::fs::write(mod_dir.join(".odin-version"), "1.2.3").unwrap();

        let db = Db::open(&paths).unwrap();

        assert_eq!(
            crate::db::global_mods::current_version(&db, "owner-mod").unwrap(),
            Some("1.2.3".to_string())
        );
    }

    #[test]
    fn opening_a_fresh_db_imports_legacy_access_lists() {
        use crate::instance::lists::ListKind;

        let paths = temp_paths("import-lists");
        write_legacy_state_json(&paths, "my-server");
        let saves_dir = paths.instance_dir("my-server").join("saves");
        std::fs::create_dir_all(&saves_dir).unwrap();
        std::fs::write(
            saves_dir.join("adminlist.txt"),
            "76561197960287930\nnot-a-steamid\n",
        )
        .unwrap();

        let db = Db::open(&paths).unwrap();

        let instance = crate::instance::Instance {
            dir: paths.instance_dir("my-server"),
            state: crate::db::instances::load(&db, "my-server")
                .unwrap()
                .unwrap(),
        };
        let ids = crate::instance::lists::read(&db, &instance, ListKind::Admin).unwrap();
        assert_eq!(ids, vec!["76561197960287930".to_string()]);
    }
}
