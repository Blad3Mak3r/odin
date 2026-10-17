//! Valheim's side of the Podman lifecycle: the few facts that differ from
//! other games. The lifecycle itself lives in [`super::lifecycle`].

use std::path::PathBuf;

use anyhow::Result;

use crate::db::Db;
use crate::db::game_instances::{self, GameInstanceIdentity};
use crate::game::GameId;
use crate::instance::Instance;
use crate::paths::{self, Paths};

use super::lifecycle::{self, ContainerGame, StopReason};
use super::{Launch, Mount, bind, write_runtime_spec};

pub(super) struct Valheim;

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
        _db: &Db,
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

    fn extra_mounts(paths: &Paths, _db: &Db, _instance: &Instance) -> Result<Vec<Mount>> {
        // Existing BepInEx plugin links point at this host-absolute store.
        // Mounting it at that exact path preserves those links without
        // rewriting an operator's mod state.
        Ok(vec![bind(&paths.mods_dir(), &paths.mods_dir())])
    }
}

pub fn is_running(paths: &Paths, db: &Db, instance: &Instance) -> Result<bool> {
    lifecycle::is_running::<Valheim>(paths, db, instance)
}

pub async fn start(paths: &Paths, db: &Db, name: &str) -> Result<Instance> {
    lifecycle::start::<Valheim>(paths, db, name).await
}

pub async fn stop(paths: &Paths, db: &Db, name: &str, reason: StopReason) -> Result<()> {
    lifecycle::stop::<Valheim>(paths, db, name, reason).await
}

pub async fn restart(paths: &Paths, db: &Db, name: &str) -> Result<Instance> {
    lifecycle::restart::<Valheim>(paths, db, name).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::podman::dispatch;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    fn write_podman_config(paths: &Paths, socket: std::path::PathBuf) {
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        let config = crate::config::GlobalConfig {
            data_dir: None,
            curseforge: None,
            podman: Some(crate::config::PodmanConfig {
                socket,
                runtime_image: None,
                base_image: None,
                proton_image: None,
                odin_binary: None,
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

        dispatch::repair(&paths, &db, GameId::Valheim, "valheim")
            .await
            .unwrap();

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
    fn remove_deletes_the_container_and_the_private_runtime_volume() {
        let root =
            std::env::temp_dir().join(format!("odin-runtime-remove-{}", uuid::Uuid::new_v4()));
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
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .unwrap();
            request
        });
        write_podman_config(&paths, podman_socket);
        let db = crate::db::Db::open(&paths).unwrap();
        let instance = Instance::create(&paths, &db, "valheim").unwrap();
        let identity = Valheim::identity(&db, &instance).unwrap();
        let runtime = paths.container_runtime_dir(GameId::Valheim, &identity.id);
        std::fs::create_dir_all(runtime.join("valheim")).unwrap();
        std::fs::write(runtime.join("valheim/payload"), "game").unwrap();
        let saves = paths::instance_saves_dir(&instance.dir);
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::write(saves.join("world.db"), "save").unwrap();

        dispatch::remove(&paths, &db, GameId::Valheim, "valheim").unwrap();

        assert!(server.join().unwrap().starts_with("DELETE "));
        assert!(!runtime.exists());
        assert!(saves.join("world.db").is_file());
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
        Instance::create(&paths, &db, "valheim").unwrap();

        let error = dispatch::repair(&paths, &db, GameId::Valheim, "valheim")
            .await
            .unwrap_err();

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
