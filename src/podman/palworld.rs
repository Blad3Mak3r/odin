//! Palworld's side of the Podman lifecycle: the few facts that differ from
//! other games. The lifecycle itself lives in [`super::lifecycle`].

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::db::Db;
use crate::db::game_instances::{self, GameInstanceIdentity, GenericGameInstance};
use crate::game::GameId;
use crate::game::palworld::{PalworldLaunch, SAVED_DIR};
use crate::paths::Paths;

use super::lifecycle::ContainerGame;
use super::{Launch, Mount, write_runtime_spec};

pub(super) struct Palworld;

impl ContainerGame for Palworld {
    const GAME: GameId = GameId::Palworld;
    type Instance = GenericGameInstance;

    fn load(_paths: &Paths, db: &Db, name: &str) -> Result<GenericGameInstance> {
        game_instances::load_generic(db, GameId::Palworld, name)?
            .context("Palworld instance does not exist")
    }

    fn identity(_db: &Db, instance: &GenericGameInstance) -> Result<GameInstanceIdentity> {
        Ok(instance.identity.clone())
    }

    fn instance_dir(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
        paths.game_instance_dir(GameId::Palworld, instance.name())
    }

    fn ports(instance: &GenericGameInstance) -> Result<Vec<u16>> {
        game_instances::claimed_ports(GameId::Palworld, &instance.config)
    }

    fn prepare_layout(
        paths: &Paths,
        _db: &Db,
        identity: &GameInstanceIdentity,
        instance: &GenericGameInstance,
    ) -> Result<()> {
        let instance_dir = Self::instance_dir(paths, instance);
        for directory in [SAVED_DIR, "logs"] {
            std::fs::create_dir_all(instance_dir.join(directory))?;
        }
        std::fs::create_dir_all(paths.container_runtime_dir(GameId::Palworld, &identity.id))?;
        std::fs::create_dir_all(paths.container_socket_dir(&identity.id))?;
        write_runtime_spec(
            paths,
            identity,
            Launch::Palworld(PalworldLaunch::try_from(instance)?),
        )?;
        Ok(())
    }

    fn extra_mounts(
        _paths: &Paths,
        _db: &Db,
        _instance: &GenericGameInstance,
    ) -> Result<Vec<Mount>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::container_instances;
    use crate::podman::dispatch;
    use crate::podman::lifecycle::StopReason;
    use crate::podman::test_support::{fake_podman, fake_runtime, temp_paths, write_podman_config};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    const OPERATOR_CONFIG: &str = "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(ServerName=\"Mine\",RESTAPIEnabled=True)\n";

    fn control_socket(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
        paths
            .container_socket_dir(&instance.identity.id)
            .join(format!("palworld-{}.control.sock", instance.name()))
    }

    #[tokio::test]
    async fn start_creates_the_palworld_container_and_prepares_its_native_layout() {
        let paths = temp_paths("palworld-start");
        let podman_socket = paths.data_dir.join("podman.sock");
        let listener = match UnixListener::bind(&podman_socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        write_podman_config(&paths, &podman_socket);
        let db = Db::open(&paths).unwrap();
        let instance =
            game_instances::create_generic(&paths, &db, GameId::Palworld, "pals").unwrap();
        // An operator's native document keeps its own values and keys.
        let config = paths
            .game_instance_dir(GameId::Palworld, "pals")
            .join(crate::game::palworld::CONFIG_FILE);
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, OPERATOR_CONFIG).unwrap();
        let podman = fake_podman(listener);
        let runtime = fake_runtime(&control_socket(&paths, &instance));

        dispatch::start(&paths, &db, GameId::Palworld, "pals")
            .await
            .unwrap();
        assert!(container_instances::desired_running(&db, &instance.identity.id).unwrap());

        let requests = podman.join().unwrap();
        runtime.join().unwrap();
        let create = requests
            .iter()
            .find(|(line, _)| line.contains("/containers/create"))
            .unwrap();
        let body: serde_json::Value = serde_json::from_str(&create.1).unwrap();
        let id = &instance.identity.id;
        assert_eq!(body["name"], format!("odin-palworld-{id}"));
        assert_eq!(body["labels"]["io.odin.game"], "palworld");
        assert_eq!(body["netns"]["nsmode"], "host");
        let mounts: Vec<_> = body["mounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|mount| mount["destination"].as_str().unwrap())
            .collect();
        assert_eq!(
            mounts,
            [
                "/srv/odin/runtime",
                "/srv/odin/instance",
                "/run/odin",
                "/usr/local/bin/odin"
            ]
        );

        let spec: serde_json::Value = serde_json::from_slice(
            &std::fs::read(super::super::runtime_spec_path(
                &paths,
                GameId::Palworld,
                id,
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(spec["game"], "palworld");
        assert_eq!(spec["name"], "pals");
        assert_eq!(spec["port"], instance.config.port);
        assert_eq!(spec["query_port"], instance.config.query_port.unwrap());
        // Starting never rewrites the game's own document.
        assert_eq!(std::fs::read_to_string(&config).unwrap(), OPERATOR_CONFIG);
        let instance_dir = paths.game_instance_dir(GameId::Palworld, "pals");
        for directory in [SAVED_DIR, "logs"] {
            assert!(instance_dir.join(directory).is_dir());
        }
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn preparing_the_layout_writes_the_runtime_spec_and_keeps_existing_data() {
        let paths = temp_paths("palworld-layout");
        let db = Db::open(&paths).unwrap();
        let instance =
            game_instances::create_generic(&paths, &db, GameId::Palworld, "pals").unwrap();
        // An existing world is already where the container will find it.
        let world = paths
            .game_instance_dir(GameId::Palworld, "pals")
            .join(SAVED_DIR)
            .join("SaveGames/world.sav");
        std::fs::create_dir_all(world.parent().unwrap()).unwrap();
        std::fs::write(&world, "world").unwrap();

        dispatch::sync_spec(&paths, &db, GameId::Palworld, "pals").unwrap();
        dispatch::sync_spec(&paths, &db, GameId::Palworld, "pals").unwrap();

        assert!(
            super::super::runtime_spec_path(&paths, GameId::Palworld, &instance.identity.id)
                .is_file()
        );
        assert_eq!(std::fs::read_to_string(&world).unwrap(), "world");
        // Preparing the layout never creates the game's own document; the container
        // seeds it from its private install on first start.
        assert!(
            !paths
                .game_instance_dir(GameId::Palworld, "pals")
                .join(crate::game::palworld::CONFIG_FILE)
                .exists()
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[tokio::test]
    async fn stop_gives_palworld_the_time_its_world_save_needs() {
        let paths = temp_paths("palworld-stop");
        let podman_socket = paths.data_dir.join("podman.sock");
        let listener = match UnixListener::bind(&podman_socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        // The container is running when stop begins and gone once polled again.
        let podman = std::thread::spawn(move || {
            for body in [
                r#"{"State":{"Running":true,"Status":"running"}}"#,
                r#"{"State":{"Running":true,"Status":"running"}}"#,
                r#"{"State":{"Running":false,"Status":"exited"}}"#,
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut String::new())
                    .unwrap();
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .unwrap();
            }
        });
        write_podman_config(&paths, &podman_socket);
        let db = Db::open(&paths).unwrap();
        let instance =
            game_instances::create_generic(&paths, &db, GameId::Palworld, "pals").unwrap();
        let control = control_socket(&paths, &instance);
        std::fs::create_dir_all(control.parent().unwrap()).unwrap();
        let runtime_listener = UnixListener::bind(&control).unwrap();
        let runtime = std::thread::spawn(move || {
            let (mut stream, _) = runtime_listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            stream.write_all(b"{\"type\":\"stopped\"}\n").unwrap();
            request
        });
        container_instances::set_desired_running(&db, &instance.identity.id, true).unwrap();

        dispatch::stop(&paths, &db, GameId::Palworld, "pals", StopReason::Operator)
            .await
            .unwrap();

        assert!(!container_instances::desired_running(&db, &instance.identity.id).unwrap());
        let request: serde_json::Value = serde_json::from_str(&runtime.join().unwrap()).unwrap();
        assert_eq!(request["type"], "stop");
        assert_eq!(request["timeout_secs"], 120);
        podman.join().unwrap();
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
