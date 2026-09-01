//! Durable storage for instance state, backing `crate::instance::Instance`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, Row, Transaction, params};

use super::Db;
use crate::instance::state::{InstalledMod, InstanceState};

/// Upserts an instance and wholesale-replaces its installed mods, mirroring
/// how the old file-based `InstanceState::save` just overwrote the whole
/// file. Runs in its own transaction.
pub fn save(db: &Db, state: &InstanceState) -> Result<()> {
    let mut conn = db.conn();
    let tx = conn.transaction().context("failed to start transaction")?;
    save_in_tx(&tx, state)?;
    tx.commit().context("failed to commit transaction")?;
    Ok(())
}

/// Same as [`save`], but against an already-open transaction — used by the
/// bootstrap importer so several instances land atomically in one go.
pub(super) fn save_in_tx(tx: &Transaction, state: &InstanceState) -> Result<()> {
    tx.execute(
        "INSERT INTO instances \
            (name, port, world_name, password, public, created_at, last_started_at, last_stopped_at, pid, pid_started_at, bepinex_installed, auto_restart, bepinex_version) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13) \
         ON CONFLICT(name) DO UPDATE SET \
            port = excluded.port, \
            world_name = excluded.world_name, \
            password = excluded.password, \
            public = excluded.public, \
            created_at = excluded.created_at, \
            last_started_at = excluded.last_started_at, \
            last_stopped_at = excluded.last_stopped_at, \
            pid = excluded.pid, \
            pid_started_at = excluded.pid_started_at, \
            bepinex_installed = excluded.bepinex_installed, \
            auto_restart = excluded.auto_restart, \
            bepinex_version = excluded.bepinex_version",
        params![
            state.name,
            state.port,
            state.world_name,
            state.password,
            state.public,
            state.created_at,
            state.last_started_at,
            state.last_stopped_at,
            state.pid,
            state.pid_started_at,
            state.bepinex_installed,
            state.auto_restart,
            state.bepinex_version,
        ],
    )
    .with_context(|| format!("failed to upsert instance '{}'", state.name))?;

    tx.execute(
        "DELETE FROM installed_mods WHERE instance_name = ?1",
        params![state.name],
    )
    .with_context(|| format!("failed to clear installed mods for '{}'", state.name))?;
    for m in &state.installed_mods {
        tx.execute(
            "INSERT INTO installed_mods (instance_name, mod_id, version, installed_at, enabled, pinned) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                state.name,
                m.mod_id,
                m.version,
                m.installed_at,
                m.enabled,
                m.pinned
            ],
        )
        .with_context(|| {
            format!(
                "failed to record installed mod '{}' for '{}'",
                m.mod_id, state.name
            )
        })?;
    }

    Ok(())
}

/// Returns an instance's state, or `None` if no instance with that name
/// exists.
pub fn load(db: &Db, name: &str) -> Result<Option<InstanceState>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(SELECT_INSTANCE)?;
    let mut rows = stmt.query(params![name])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let mut state = row_to_state(row)?;
    drop(rows);
    drop(stmt);
    state.installed_mods = load_installed_mods(&conn, name)?;
    Ok(Some(state))
}

/// Returns every instance's state, ordered by name.
pub fn list_all(db: &Db) -> Result<Vec<InstanceState>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(SELECT_ALL_INSTANCES)?;
    let mut states = stmt
        .query_map([], row_to_state)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for state in &mut states {
        state.installed_mods = load_installed_mods(&conn, &state.name)?;
    }
    Ok(states)
}

/// Deletes an instance's row; `installed_mods` cascades via the schema's
/// `ON DELETE CASCADE`. A no-op if the instance doesn't exist.
pub fn delete(db: &Db, name: &str) -> Result<()> {
    db.conn()
        .execute("DELETE FROM instances WHERE name = ?1", params![name])
        .with_context(|| format!("failed to delete instance '{name}'"))?;
    Ok(())
}

const SELECT_INSTANCE: &str = "SELECT name, port, world_name, password, public, created_at, \
     last_started_at, last_stopped_at, pid, pid_started_at, bepinex_installed, auto_restart, bepinex_version \
     FROM instances WHERE name = ?1";
const SELECT_ALL_INSTANCES: &str = "SELECT name, port, world_name, password, public, created_at, \
     last_started_at, last_stopped_at, pid, pid_started_at, bepinex_installed, auto_restart, bepinex_version \
     FROM instances ORDER BY name";

fn row_to_state(row: &Row) -> rusqlite::Result<InstanceState> {
    Ok(InstanceState {
        name: row.get(0)?,
        port: row.get(1)?,
        world_name: row.get(2)?,
        password: row.get(3)?,
        public: row.get(4)?,
        created_at: row.get(5)?,
        last_started_at: row.get(6)?,
        last_stopped_at: row.get(7)?,
        pid: row.get(8)?,
        pid_started_at: row.get(9)?,
        bepinex_installed: row.get(10)?,
        auto_restart: row.get(11)?,
        bepinex_version: row.get(12)?,
        installed_mods: Vec::new(),
    })
}

/// Updates only BepInEx metadata, avoiding lost updates to unrelated state.
pub fn set_bepinex(db: &Db, name: &str, installed: bool, version: Option<&str>) -> Result<()> {
    db.conn()
        .execute(
            "UPDATE instances SET bepinex_installed = ?2, bepinex_version = ?3 WHERE name = ?1",
            params![name, installed, version],
        )
        .with_context(|| format!("failed to update BepInEx state for '{name}'"))?;
    Ok(())
}

/// Persists a freshly spawned process's identity fingerprint after a
/// successful `start()`. Narrower than [`save`] so it can't clobber a
/// concurrent edit to an unrelated column.
pub fn set_pid(
    db: &Db,
    name: &str,
    pid: u32,
    pid_started_at: i64,
    started_at: DateTime<Utc>,
) -> Result<()> {
    db.conn()
        .execute(
            "UPDATE instances SET pid = ?2, pid_started_at = ?3, last_started_at = ?4 WHERE name = ?1",
            params![name, pid, pid_started_at, started_at],
        )
        .with_context(|| format!("failed to record pid for instance '{name}'"))?;
    Ok(())
}

/// Clears a stopped instance's pid fingerprint and stamps `last_stopped_at`.
/// Used both by an explicit `stop()` and by reconciliation when a persisted
/// pid is found to be dead.
pub fn clear_pid(db: &Db, name: &str, stopped_at: DateTime<Utc>) -> Result<()> {
    db.conn()
        .execute(
            "UPDATE instances SET pid = NULL, pid_started_at = NULL, last_stopped_at = ?2 WHERE name = ?1",
            params![name, stopped_at],
        )
        .with_context(|| format!("failed to clear pid for instance '{name}'"))?;
    Ok(())
}

fn load_installed_mods(conn: &Connection, instance_name: &str) -> Result<Vec<InstalledMod>> {
    let mut stmt = conn.prepare(
        "SELECT mod_id, version, installed_at, enabled, pinned FROM installed_mods \
         WHERE instance_name = ?1 ORDER BY mod_id",
    )?;
    let mods = stmt
        .query_map(params![instance_name], |row| {
            Ok(InstalledMod {
                mod_id: row.get(0)?,
                version: row.get(1)?,
                installed_at: row.get(2)?,
                enabled: row.get(3)?,
                pinned: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(mods)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use chrono::Utc;

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-instances-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Db::open(&Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        })
        .unwrap()
    }

    fn sample(name: &str) -> InstanceState {
        let mut state = InstanceState::new(name, 2456);
        state.installed_mods.push(InstalledMod {
            mod_id: "owner-mod".to_string(),
            version: "1.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: true,
            pinned: false,
        });
        state
    }

    #[test]
    fn save_then_load_round_trips() {
        let db = temp_db("roundtrip");
        let original = sample("my-server");
        save(&db, &original).unwrap();

        let loaded = load(&db, "my-server").unwrap().unwrap();
        assert_eq!(loaded.name, original.name);
        assert_eq!(loaded.port, original.port);
        assert_eq!(loaded.installed_mods.len(), 1);
        assert_eq!(loaded.installed_mods[0].mod_id, "owner-mod");
    }

    #[test]
    fn bepinex_state_round_trips_known_unknown_and_absent_versions() {
        let db = temp_db("bepinex-version");
        let mut known = sample("known");
        known.bepinex_installed = true;
        known.bepinex_version = Some("5.4.2305".to_string());
        save(&db, &known).unwrap();

        let mut unknown = sample("unknown");
        unknown.bepinex_installed = true;
        save(&db, &unknown).unwrap();
        save(&db, &sample("absent")).unwrap();

        let known = load(&db, "known").unwrap().unwrap();
        assert!(known.bepinex_installed);
        assert_eq!(known.bepinex_version.as_deref(), Some("5.4.2305"));
        let unknown = load(&db, "unknown").unwrap().unwrap();
        assert!(unknown.bepinex_installed);
        assert_eq!(unknown.bepinex_version, None);
        let absent = load(&db, "absent").unwrap().unwrap();
        assert!(!absent.bepinex_installed);
        assert_eq!(absent.bepinex_version, None);
    }

    #[test]
    fn narrow_bepinex_update_does_not_overwrite_other_state() {
        let db = temp_db("narrow-bepinex-update");
        let original = sample("my-server");
        save(&db, &original).unwrap();
        set_bepinex(&db, "my-server", true, Some("5.4.2305")).unwrap();
        let loaded = load(&db, "my-server").unwrap().unwrap();
        assert_eq!(loaded.port, original.port);
        assert_eq!(loaded.installed_mods.len(), 1);
        assert_eq!(loaded.bepinex_version.as_deref(), Some("5.4.2305"));
    }

    #[test]
    fn set_pid_then_clear_pid_round_trips() {
        let db = temp_db("set-clear-pid");
        save(&db, &sample("my-server")).unwrap();

        set_pid(&db, "my-server", 4242, 1_700_000_000, Utc::now()).unwrap();
        let running = load(&db, "my-server").unwrap().unwrap();
        assert_eq!(running.pid, Some(4242));
        assert_eq!(running.pid_started_at, Some(1_700_000_000));
        assert!(running.last_started_at.is_some());

        clear_pid(&db, "my-server", Utc::now()).unwrap();
        let stopped = load(&db, "my-server").unwrap().unwrap();
        assert_eq!(stopped.pid, None);
        assert_eq!(stopped.pid_started_at, None);
        assert!(stopped.last_stopped_at.is_some());
    }

    #[test]
    fn load_missing_instance_is_none() {
        let db = temp_db("missing");
        assert!(load(&db, "nope").unwrap().is_none());
    }

    #[test]
    fn saving_twice_replaces_installed_mods_rather_than_appending() {
        let db = temp_db("replace");
        let mut state = sample("my-server");
        save(&db, &state).unwrap();

        state.installed_mods.clear();
        state.installed_mods.push(InstalledMod {
            mod_id: "other-mod".to_string(),
            version: "2.0.0".to_string(),
            installed_at: Utc::now(),
            enabled: false,
            pinned: true,
        });
        save(&db, &state).unwrap();

        let loaded = load(&db, "my-server").unwrap().unwrap();
        assert_eq!(loaded.installed_mods.len(), 1);
        assert_eq!(loaded.installed_mods[0].mod_id, "other-mod");
    }

    #[test]
    fn list_all_returns_every_instance_ordered_by_name() {
        let db = temp_db("list");
        save(&db, &sample("bravo")).unwrap();
        save(&db, &sample("alpha")).unwrap();

        let names: Vec<String> = list_all(&db).unwrap().into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["alpha".to_string(), "bravo".to_string()]);
    }

    #[test]
    fn delete_removes_instance_and_cascades_installed_mods() {
        let db = temp_db("delete");
        save(&db, &sample("my-server")).unwrap();

        delete(&db, "my-server").unwrap();

        assert!(load(&db, "my-server").unwrap().is_none());
        let mod_count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM installed_mods", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mod_count, 0);
    }
}
