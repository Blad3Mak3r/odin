//! Port allocation and conflict checks shared by the compiled game drivers.

use std::collections::HashSet;

use anyhow::{Context, Result};

use super::{GameId, driver};
use crate::paths::Paths;

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
    paths: &Paths,
    db: &crate::db::Db,
    game: GameId,
    name: &str,
    requested_ports: impl IntoIterator<Item = u16>,
) -> Result<()> {
    let requested_ports: HashSet<_> = requested_ports.into_iter().collect();
    for instance in crate::instance::list_all(paths, db)? {
        if game == GameId::Valheim && instance.state.name == name {
            continue;
        }
        // A containerized server shares the host network, so its container
        // state is what decides whether its ports are taken. An unreachable
        // Podman must not block unrelated games from starting.
        let running = crate::podman::dispatch::is_running(paths, db, GameId::Valheim, &instance.state.name).unwrap_or_else(
            |error| {
                tracing::debug!(instance = %instance.state.name, %error, "could not tell whether Valheim is running");
                false
            },
        );
        if !running {
            continue;
        }
        let occupied = block(GameId::Valheim, instance.state.port)?;
        if let Some(port) = occupied
            .into_iter()
            .find(|port| requested_ports.contains(port))
        {
            anyhow::bail!(
                "port {port} is already in use by running Valheim instance '{}'",
                instance.state.name
            );
        }
    }

    for instance in crate::db::game_instances::list_rust(db)? {
        if game == GameId::Rust && instance.name() == name {
            continue;
        }
        let running = crate::game::rust::is_running(paths, db, &instance).unwrap_or_else(|error| {
            tracing::debug!(instance = %instance.name(), %error, "could not tell whether Rust is running");
            false
        });
        if !running {
            continue;
        }
        for port in [
            instance.config.port,
            instance.config.query_port,
            instance.config.rcon_port,
        ] {
            if requested_ports.contains(&port) {
                anyhow::bail!(
                    "port {port} is already in use by running Rust instance '{}'",
                    instance.name()
                );
            }
        }
    }
    for other_game in [
        GameId::VRising,
        GameId::Palworld,
        GameId::RunescapeDragonwilds,
        GameId::SevenDaysToDie,
    ] {
        for instance in crate::db::game_instances::list_generic(db, other_game)? {
            if game == other_game && instance.name() == name {
                continue;
            }
            if !instance.is_running() {
                continue;
            }
            for port in crate::db::game_instances::claimed_ports(other_game, &instance.config)? {
                if requested_ports.contains(&port) {
                    anyhow::bail!(
                        "port {port} is already in use by running {other_game} instance '{}'",
                        instance.name()
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drivers_declare_their_required_port_blocks() {
        assert_eq!(
            block(GameId::Valheim, 2456).unwrap(),
            vec![2456, 2457, 2458]
        );
        assert_eq!(
            block(GameId::Rust, 28015).unwrap(),
            vec![28015, 28016, 28017]
        );
        assert_eq!(
            block(GameId::SevenDaysToDie, 26900).unwrap(),
            vec![26900, 26901, 26902]
        );
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
            &paths,
            &db,
            GameId::Rust,
            "rust-server",
            block(GameId::Rust, 28015).unwrap(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("28015"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn a_running_valheim_container_reserves_its_ports() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        let dir =
            std::env::temp_dir().join(format!("odin-container-ports-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let socket = dir.join("podman.sock");
        let listener = match UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut String::new())
                .unwrap();
            let body = r#"{"State":{"Running":true,"Status":"running"}}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .unwrap();
        });
        let config = crate::config::GlobalConfig {
            data_dir: None,
            podman: Some(crate::config::PodmanConfig {
                socket,
                runtime_image: None,
            }),
        };
        std::fs::write(
            paths.config_file(),
            toml::to_string_pretty(&config).unwrap(),
        )
        .unwrap();
        let db = crate::db::Db::open(&paths).unwrap();
        // Born containerized, with no host pid to fingerprint.
        let valheim = crate::game::instances::create(&paths, &db, GameId::Valheim, "contained")
            .map(|created| match created {
                crate::game::instances::GameInstance::Valheim(instance) => instance,
                _ => unreachable!("created a Valheim instance"),
            })
            .unwrap();

        let error = ensure_available(
            &paths,
            &db,
            GameId::Rust,
            "rust-server",
            block(GameId::Valheim, valheim.state.port).unwrap(),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("running Valheim instance 'contained'")
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn running_generic_servers_reserve_ports_across_games() {
        let dir =
            std::env::temp_dir().join(format!("odin-generic-port-check-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = crate::db::Db::open(&paths).unwrap();
        let palworld = crate::db::game_instances::create_generic(
            &paths,
            &db,
            GameId::Palworld,
            "palworld-server",
        )
        .unwrap();
        let config = crate::db::game_instances::GenericGameConfig {
            port: 28015,
            ..palworld.config
        };
        crate::db::game_instances::update_generic_config(
            &db,
            GameId::Palworld,
            "palworld-server",
            &config,
        )
        .unwrap();
        crate::db::game_instances::set_generic_pid(
            &db,
            GameId::Palworld,
            "palworld-server",
            std::process::id(),
            crate::instance::process::start_time_of(std::process::id()).unwrap(),
            chrono::Utc::now(),
        )
        .unwrap();

        let error =
            ensure_available(&paths, &db, GameId::Rust, "rust-server", [28015]).unwrap_err();
        assert!(error.to_string().contains("palworld"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
