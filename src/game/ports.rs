//! Port allocation and conflict checks shared by the compiled game drivers.

use std::collections::HashSet;

use anyhow::{Context, Result};

use super::{GameId, driver};

/// Returns every consecutive port claimed by an instance starting at
/// `base_port`. Drivers whose ports cease to be consecutive can reserve their
/// explicit ports alongside this block when loading existing configuration.
pub fn block(game: GameId, base_port: u16) -> Result<Vec<u16>> {
    let count = driver(game).port_requirements().count;
    (0..count)
        .map(|offset| {
            base_port
                .checked_add(offset)
                .context("no available game port block remains")
        })
        .collect()
}

/// Refuses to start a managed instance if any of its ports are already held
/// by another live Odin-managed game server. This complements allocation:
/// saved configuration can be edited independently after an instance exists.
pub fn ensure_available(
    db: &crate::db::Db,
    game: GameId,
    name: &str,
    requested_ports: impl IntoIterator<Item = u16>,
) -> Result<()> {
    let requested_ports: HashSet<_> = requested_ports.into_iter().collect();
    for instance in crate::db::instances::list_all(db)? {
        if game == GameId::Valheim && instance.name == name {
            continue;
        }
        let running = matches!(
            (instance.pid, instance.pid_started_at),
            (Some(pid), Some(started_at)) if crate::instance::process::is_alive(pid, started_at)
        );
        if !running {
            continue;
        }
        let occupied = block(GameId::Valheim, instance.port)?;
        if let Some(port) = occupied
            .into_iter()
            .find(|port| requested_ports.contains(port))
        {
            anyhow::bail!(
                "port {port} is already in use by running Valheim instance '{}'",
                instance.name
            );
        }
    }

    for instance in crate::db::game_instances::list_rust(db)? {
        if game == GameId::Rust && instance.name() == name {
            continue;
        }
        if !instance.is_running() {
            continue;
        }
        for port in [instance.config.port, instance.config.query_port] {
            if requested_ports.contains(&port) {
                anyhow::bail!(
                    "port {port} is already in use by running Rust instance '{}'",
                    instance.name()
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    #[test]
    fn drivers_declare_their_required_port_blocks() {
        assert_eq!(
            block(GameId::Valheim, 2456).unwrap(),
            vec![2456, 2457, 2458]
        );
        assert_eq!(block(GameId::Rust, 28015).unwrap(), vec![28015, 28016]);
    }

    #[test]
    fn running_servers_reserve_ports_across_games() {
        let dir = std::env::temp_dir().join(format!("odin-port-check-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = crate::db::Db::open(&paths).unwrap();
        let mut valheim = crate::instance::Instance::create(&paths, &db, "valheim-server").unwrap();
        valheim.state.port = 28015;
        valheim.state.pid = Some(std::process::id());
        valheim.state.pid_started_at =
            Some(crate::instance::process::start_time_of(std::process::id()).unwrap());
        valheim.save(&db).unwrap();

        let error = ensure_available(
            &db,
            GameId::Rust,
            "rust-server",
            block(GameId::Rust, 28015).unwrap(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("28015"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
