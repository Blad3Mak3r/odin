//! Valheim's side of the Podman lifecycle: the few facts that differ from
//! other games. The lifecycle itself lives in [`super::lifecycle`].

use std::path::PathBuf;

use anyhow::Result;

use crate::db::Db;
use crate::db::game_instances::{self, GameInstanceIdentity};
use crate::game::GameId;
use crate::instance::{Instance, lifecycle as direct};
use crate::paths::{self, Paths};

use super::lifecycle::{self, ContainerGame};
use super::{Launch, Mount, bind, write_runtime_spec};

pub use super::lifecycle::RuntimeHealth;

struct Valheim;

impl ContainerGame for Valheim {
    const GAME: GameId = GameId::Valheim;
    type Instance = Instance;

    fn load(paths: &Paths, db: &Db, name: &str) -> Result<Instance> {
        Instance::load_existing(paths, db, name)
    }

    fn identity(db: &Db, instance: &Instance) -> Result<GameInstanceIdentity> {
        game_instances::ensure_valheim_identity(db, &instance.state.name, instance.state.created_at)
    }

    fn instance_dir(_paths: &Paths, instance: &Instance) -> PathBuf {
        instance.dir.clone()
    }

    fn ports(instance: &Instance) -> Result<Vec<u16>> {
        crate::game::ports::block(GameId::Valheim, instance.state.port)
    }

    fn prepare_layout(
        paths: &Paths,
        identity: &GameInstanceIdentity,
        instance: &Instance,
    ) -> Result<()> {
        std::fs::create_dir_all(paths::instance_saves_dir(&instance.dir))?;
        std::fs::create_dir_all(paths::instance_logs_dir(&instance.dir))?;
        std::fs::create_dir_all(paths.container_runtime_dir(GameId::Valheim, &identity.id))?;
        std::fs::create_dir_all(paths.container_socket_dir(&identity.id))?;
        std::fs::create_dir_all(paths.mods_dir())?;
        write_runtime_spec(
            paths,
            identity,
            Launch::Valheim {
                instance: instance.state.clone(),
            },
        )?;
        Ok(())
    }

    fn extra_mounts(paths: &Paths) -> Vec<Mount> {
        // Existing BepInEx plugin links point at this host-absolute store.
        // Mounting it at that exact path preserves those links without
        // rewriting an operator's mod state.
        vec![bind(&paths.mods_dir(), &paths.mods_dir())]
    }

    fn running_directly(instance: &Instance) -> Result<bool> {
        direct::is_running(instance)
    }
}

pub fn migrate(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    lifecycle::migrate::<Valheim>(paths, db, name)
}

pub fn mark_new_instance(db: &Db, instance: &Instance) -> Result<()> {
    lifecycle::mark_new_instance::<Valheim>(db, instance)
}

pub fn is_migrated(db: &Db, instance: &Instance) -> Result<bool> {
    lifecycle::is_migrated::<Valheim>(db, instance)
}

pub async fn runtime_health(paths: &Paths, db: &Db, name: &str) -> Result<RuntimeHealth> {
    lifecycle::runtime_health::<Valheim>(paths, db, name).await
}

pub async fn repair(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    lifecycle::repair::<Valheim>(paths, db, name).await
}

pub fn sync_spec(paths: &Paths, db: &Db, instance: &Instance) -> Result<()> {
    lifecycle::sync_spec::<Valheim>(paths, db, instance)
}

pub fn is_running(paths: &Paths, db: &Db, instance: &Instance) -> Result<bool> {
    lifecycle::is_running::<Valheim>(paths, db, instance)
}

pub async fn is_running_async(paths: &Paths, db: &Db, instance: &Instance) -> Result<bool> {
    lifecycle::is_running_async::<Valheim>(paths, db, instance).await
}

pub async fn start(paths: &Paths, db: &Db, name: &str) -> Result<Instance> {
    lifecycle::start::<Valheim>(paths, db, name).await
}

pub async fn stop(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    lifecycle::stop::<Valheim>(paths, db, name).await
}

pub async fn restart(paths: &Paths, db: &Db, name: &str) -> Result<Instance> {
    lifecycle::restart::<Valheim>(paths, db, name).await
}

pub async fn update(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    lifecycle::update::<Valheim>(paths, db, name).await
}

pub async fn update_limits(paths: &Paths, db: &Db, instance_id: &str) -> Result<()> {
    lifecycle::update_limits(paths, db, GameId::Valheim, instance_id).await
}

pub fn remove(paths: &Paths, db: &Db, instance: &Instance) -> Result<()> {
    lifecycle::remove::<Valheim>(paths, db, instance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::container_instances;
    use crate::podman::lifecycle::require_migrated;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    fn write_podman_config(paths: &Paths, socket: std::path::PathBuf) {
        std::fs::create_dir_all(&paths.config_dir).unwrap();
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
    }

    #[tokio::test]
    async fn repair_removes_only_the_stopped_container_and_ephemeral_sockets() {
        let root =
            std::env::temp_dir().join(format!("odin-runtime-repair-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = Paths {
            config_dir: root.clone(),
            data_dir: root.clone(),
        };
        let podman_socket = root.join("podman.sock");
        let listener = match UnixListener::bind(&podman_socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                std::fs::remove_dir_all(root).ok();
                return;
            }
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let (status, body) = if request.contains("/version") {
                    ("200 OK", r#"{"Version":"5.0.0","ApiVersion":"1.41"}"#)
                } else if request.contains("/json") {
                    ("200 OK", r#"{"State":{"Running":false,"Status":"exited"}}"#)
                } else {
                    ("204 No Content", "")
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                requests.push(request);
            }
            requests
        });
        write_podman_config(&paths, podman_socket.clone());

        let db = crate::db::Db::open(&paths).unwrap();
        let instance = Instance::create(&paths, &db, "valheim").unwrap();
        let identity = Valheim::identity(&db, &instance).unwrap();
        container_instances::mark_podman(&db, &identity.id).unwrap();
        let saves = paths::instance_saves_dir(&instance.dir);
        std::fs::create_dir_all(&saves).unwrap();
        let save = saves.join("world.db");
        std::fs::write(&save, "save data").unwrap();
        let backups = paths::instance_backups_dir(&instance.dir);
        std::fs::create_dir_all(&backups).unwrap();
        let backup = backups.join("world.zip");
        std::fs::write(&backup, "backup data").unwrap();
        let mods = paths.mods_dir();
        std::fs::create_dir_all(&mods).unwrap();
        let mod_file = mods.join("example-mod");
        std::fs::write(&mod_file, "mod data").unwrap();
        let socket_dir = paths.container_socket_dir(&identity.id);
        std::fs::create_dir_all(&socket_dir).unwrap();
        let stale_socket = socket_dir.join("stale.sock");
        let stale_listener = UnixListener::bind(&stale_socket).unwrap();

        repair(&paths, &db, "valheim").await.unwrap();

        assert_eq!(std::fs::read_to_string(save).unwrap(), "save data");
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "backup data");
        assert_eq!(std::fs::read_to_string(mod_file).unwrap(), "mod data");
        assert!(crate::podman::runtime_spec_path(&paths, GameId::Valheim, &identity.id).is_file());
        assert!(!stale_socket.exists());
        let requests = server.join().unwrap();
        assert!(
            requests
                .iter()
                .any(|request| request.starts_with("DELETE "))
        );
        drop(stale_listener);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn migration_gate_rejects_an_unmigrated_instance() {
        let root = std::env::temp_dir().join(format!(
            "odin-runtime-repair-unmigrated-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let paths = Paths {
            config_dir: root.clone(),
            data_dir: root.clone(),
        };
        let db = crate::db::Db::open(&paths).unwrap();
        let instance = Instance::create(&paths, &db, "valheim").unwrap();
        let identity = Valheim::identity(&db, &instance).unwrap();

        let error = require_migrated(&db, &identity).unwrap_err();

        assert!(
            error.to_string().contains("has not been migrated"),
            "unexpected migration error: {error:#}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn repair_rejects_a_running_container() {
        let root = std::env::temp_dir().join(format!(
            "odin-runtime-repair-running-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let paths = Paths {
            config_dir: root.clone(),
            data_dir: root.clone(),
        };
        let podman_socket = root.join("podman.sock");
        let listener = match UnixListener::bind(&podman_socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                std::fs::remove_dir_all(root).ok();
                return;
            }
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let body = if request.contains("/version") {
                    r#"{"Version":"5.0.0","ApiVersion":"1.41"}"#
                } else {
                    r#"{"State":{"Running":true,"Status":"running"}}"#
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                requests.push(request);
            }
            requests
        });
        write_podman_config(&paths, podman_socket);
        let db = crate::db::Db::open(&paths).unwrap();
        let instance = Instance::create(&paths, &db, "valheim").unwrap();
        let identity = Valheim::identity(&db, &instance).unwrap();
        container_instances::mark_podman(&db, &identity.id).unwrap();

        let error = repair(&paths, &db, "valheim").await.unwrap_err();

        assert!(error.to_string().contains("already running"));
        let requests = server.join().unwrap();
        assert!(
            requests
                .iter()
                .all(|request| !request.starts_with("DELETE "))
        );
        std::fs::remove_dir_all(root).ok();
    }
}
