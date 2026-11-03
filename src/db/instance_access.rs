//! Which instances a viewer or operator may see. Administrators ignore this
//! table and reach everything.

use std::collections::HashSet;

use anyhow::Result;
use rusqlite::params;

use super::Db;
use super::users::UserError;
use crate::game::GameId;

/// What a restricted account may reach, by both identities an instance has:
/// the UUID used in API paths and the `(game, name)` pair that jobs,
/// activity and telemetry are keyed by.
#[derive(Debug, Default, Clone)]
pub struct Granted {
    pub ids: HashSet<String>,
    pub names: HashSet<(GameId, String)>,
}

pub fn granted(db: &Db, email: &str) -> Result<Granted> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT g.id, g.game, g.name FROM user_instance_access a \
         JOIN game_instances g ON g.id = a.instance_id WHERE a.user_email = ?1",
    )?;
    let rows = stmt.query_map(params![email], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut granted = Granted::default();
    for row in rows {
        let (id, game, name) = row?;
        granted.ids.insert(id);
        if let Ok(game) = game.parse::<GameId>() {
            granted.names.insert((game, name));
        }
    }
    Ok(granted)
}

/// Instance ids granted to one account, sorted.
pub fn instance_ids(db: &Db, email: &str) -> Result<Vec<String>> {
    let mut ids: Vec<String> = granted(db, email)?.ids.into_iter().collect();
    ids.sort();
    Ok(ids)
}

/// Replaces the account's grants. Fails, changing nothing, when an id is not
/// an existing instance.
pub fn replace(db: &Db, email: &str, instance_ids: &[String]) -> Result<()> {
    let mut conn = db.conn();
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM user_instance_access WHERE user_email = ?1",
        params![email],
    )?;
    for id in instance_ids {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM game_instances WHERE id = ?1)",
            params![id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(UserError::UnknownInstance(id.clone()).into());
        }
        tx.execute(
            "INSERT OR IGNORE INTO user_instance_access (user_email, instance_id) VALUES (?1, ?2)",
            params![email, id],
        )?;
    }
    tx.commit()?;
    Ok(())
}
