//! Counter-Strike 2's side of the Podman lifecycle: the few facts that differ
//! from other games. The lifecycle itself lives in [`super::lifecycle`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::db::Db;
use crate::db::game_instances::{self, GameInstanceIdentity, GenericGameInstance};
use crate::game::GameId;
use crate::paths::Paths;

use super::lifecycle::ContainerGame;
use super::shared_install;
use super::{Launch, Mount, bind, write_runtime_spec, write_runtime_spec_with_install_dir};

pub(super) struct Cs2;

impl ContainerGame for Cs2 {
    const GAME: GameId = GameId::Cs2;
    type Instance = GenericGameInstance;

    fn load(_paths: &Paths, db: &Db, name: &str) -> Result<GenericGameInstance> {
        game_instances::load_generic(db, GameId::Cs2, name)?
            .context("Counter-Strike 2 instance does not exist")
    }

    fn identity(_db: &Db, instance: &GenericGameInstance) -> Result<GameInstanceIdentity> {
        Ok(instance.identity.clone())
    }

    fn instance_dir(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
        paths.game_instance_dir(GameId::Cs2, instance.name())
    }

    fn ports(instance: &GenericGameInstance) -> Result<Vec<u16>> {
        game_instances::claimed_ports(GameId::Cs2, &instance.config)
    }

    fn ensure_ready_to_start(paths: &Paths, db: &Db, instance: &GenericGameInstance) -> Result<()> {
        shared_install::ensure_ready(paths, db, &instance.identity)
    }

    fn prepare_layout(
        paths: &Paths,
        db: &Db,
        identity: &GameInstanceIdentity,
        instance: &GenericGameInstance,
    ) -> Result<()> {
        let instance_dir = Self::instance_dir(paths, instance);
        std::fs::create_dir_all(instance_dir.join("cfg"))?;
        std::fs::create_dir_all(instance_dir.join("logs"))?;
        std::fs::create_dir_all(paths.container_runtime_dir(GameId::Cs2, &identity.id))?;
        std::fs::create_dir_all(paths.container_socket_dir(&identity.id))?;
        let launch = Launch::Cs2(game_instances::cs2_launch(db, instance)?);
        if game_instances::is_shared_install(db, &identity.id)? {
            // The shared files are mounted read-only with this instance's
            // document on top, and a mount needs its source to exist. The
            // shared installation holds the template to start it from.
            crate::game::config_documents::initialize(paths, instance)?;
            write_runtime_spec_with_install_dir(
                paths,
                identity,
                launch,
                Some(PathBuf::from(shared_install::MOUNT_DIR)),
            )?;
        } else {
            write_runtime_spec(paths, identity, launch)?;
        }
        Ok(())
    }

    fn extra_mounts(paths: &Paths, db: &Db, instance: &GenericGameInstance) -> Result<Vec<Mount>> {
        // The server runs, saves nothing and stops cleanly from a read-only
        // install tree (it only fails to write `steam_appid.txt`, `boot.vcfg`
        // and empty `save/` and `rpt/` directories), so the shared files can
        // be read-only. Its `server.cfg` is the one file that must differ per
        // instance, so the instance's own document is mounted over it.
        // An instance with its own files links the document in the container.
        if !game_instances::is_shared_install(db, &instance.identity.id)? {
            return Ok(Vec::new());
        }
        let document = Self::instance_dir(paths, instance).join(crate::game::cs2::CONFIG_PATH);
        Ok(vec![
            shared_install::mount_read_only(paths, Self::GAME),
            bind(
                &document,
                &Path::new(shared_install::MOUNT_DIR).join(crate::game::cs2::CONFIG_FILE),
            ),
        ])
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

    const OPERATOR_CONFIG: &str = "hostname \"Mine\"\nsv_cheats 0\n";

    fn control_socket(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
        paths
            .container_socket_dir(&instance.identity.id)
            .join(format!("cs2-{}.control.sock", instance.name()))
    }

    fn install_shared_files(paths: &Paths) {
        let shared = paths.shared_install_dir(GameId::Cs2);
        let binary = shared.join(crate::game::cs2::SERVER_BINARY);
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(binary, b"").unwrap();
        let shipped = shared.join(crate::game::cs2::CONFIG_FILE);
        std::fs::create_dir_all(shipped.parent().unwrap()).unwrap();
        std::fs::write(shipped, "// Defaults in server_default.cfg").unwrap();
    }

    /// Starts the instance against a fake Podman and returns the create
    /// request's body.
    async fn start(
        paths: &Paths,
        db: &Db,
        instance: &GenericGameInstance,
    ) -> Option<serde_json::Value> {
        let podman_socket = paths.data_dir.join("podman.sock");
        let listener = match UnixListener::bind(&podman_socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        write_podman_config(paths, &podman_socket);
        let podman = fake_podman(listener);
        let runtime = fake_runtime(&control_socket(paths, instance));

        dispatch::start(paths, db, GameId::Cs2, instance.name())
            .await
            .unwrap();

        let requests = podman.join().unwrap();
        runtime.join().unwrap();
        let create = requests
            .iter()
            .find(|(line, _)| line.contains("/containers/create"))
            .unwrap();
        Some(serde_json::from_str(&create.1).unwrap())
    }

    fn mounts(body: &serde_json::Value) -> Vec<(String, String, String)> {
        body["mounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|mount| {
                (
                    mount["source"].as_str().unwrap().to_string(),
                    mount["destination"].as_str().unwrap().to_string(),
                    mount["options"][0].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn spec(paths: &Paths, instance: &GenericGameInstance) -> serde_json::Value {
        serde_json::from_slice(
            &std::fs::read(super::super::runtime_spec_path(
                paths,
                GameId::Cs2,
                &instance.identity.id,
            ))
            .unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn start_creates_the_cs2_container_and_leaves_the_operators_document_alone() {
        let paths = temp_paths("cs2-start");
        let db = Db::open(&paths).unwrap();
        let instance = game_instances::create_generic(&paths, &db, GameId::Cs2, "dust").unwrap();
        let document = paths
            .game_instance_dir(GameId::Cs2, "dust")
            .join(crate::game::cs2::CONFIG_PATH);
        std::fs::create_dir_all(document.parent().unwrap()).unwrap();
        std::fs::write(&document, OPERATOR_CONFIG).unwrap();

        let Some(body) = start(&paths, &db, &instance).await else {
            return;
        };

        let id = &instance.identity.id;
        assert_eq!(body["name"], format!("odin-cs2-{id}"));
        assert_eq!(body["labels"]["io.odin.game"], "cs2");
        assert_eq!(body["netns"]["nsmode"], "host");
        assert!(container_instances::desired_running(&db, id).unwrap());
        // An isolated instance links its document inside its own game files,
        // so it needs no mount beyond the shared ones.
        let destinations: Vec<_> = mounts(&body).into_iter().map(|mount| mount.1).collect();
        assert_eq!(
            destinations,
            [
                "/srv/odin/runtime",
                "/srv/odin/instance",
                "/run/odin",
                "/usr/local/bin/odin"
            ]
        );
        let spec = spec(&paths, &instance);
        assert_eq!(spec["game"], "cs2");
        assert_eq!(spec["name"], "dust");
        assert_eq!(spec["port"], 27015);
        assert_eq!(spec["tv_port"], 27020);
        assert_eq!(spec["map"], "de_dust2");
        assert_eq!(spec["game_mode"], "competitive");
        assert_eq!(spec["max_players"], 10);
        assert!(
            spec["rcon_password"]
                .as_str()
                .is_some_and(|password| !password.is_empty())
        );
        assert_eq!(spec["shared_install"], false);
        assert_eq!(std::fs::read_to_string(&document).unwrap(), OPERATOR_CONFIG);
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[tokio::test]
    async fn a_shared_instance_runs_from_read_only_shared_files_with_its_own_document() {
        let paths = temp_paths("cs2-shared-start");
        let db = Db::open(&paths).unwrap();
        let instance = game_instances::create_generic(&paths, &db, GameId::Cs2, "dust").unwrap();
        game_instances::set_install_mode(
            &db,
            &instance.identity.id,
            game_instances::InstallMode::Shared,
        )
        .unwrap();
        install_shared_files(&paths);

        let Some(body) = start(&paths, &db, &instance).await else {
            return;
        };

        let shared = paths.shared_install_dir(GameId::Cs2);
        let document = paths
            .game_instance_dir(GameId::Cs2, "dust")
            .join(crate::game::cs2::CONFIG_PATH);
        let mounts = mounts(&body);
        // The read-only shared tree comes first so the document mounted over
        // its `server.cfg` is visible.
        assert_eq!(
            &mounts[mounts.len() - 2..],
            [
                (
                    shared.display().to_string(),
                    "/srv/odin/shared".to_string(),
                    "ro".to_string()
                ),
                (
                    document.display().to_string(),
                    "/srv/odin/shared/game/csgo/cfg/server.cfg".to_string(),
                    "rw".to_string()
                ),
            ]
        );
        // The document starts from the file the shared installation ships.
        assert_eq!(
            std::fs::read_to_string(&document).unwrap(),
            "// Defaults in server_default.cfg"
        );
        let spec = spec(&paths, &instance);
        assert_eq!(spec["install_dir"], "/srv/odin/shared");
        assert_eq!(spec["shared_install"], true);
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[tokio::test]
    async fn a_shared_instance_keeps_its_own_document_when_it_starts() {
        let paths = temp_paths("cs2-shared-keep");
        let db = Db::open(&paths).unwrap();
        let instance = game_instances::create_generic(&paths, &db, GameId::Cs2, "dust").unwrap();
        game_instances::set_install_mode(
            &db,
            &instance.identity.id,
            game_instances::InstallMode::Shared,
        )
        .unwrap();
        install_shared_files(&paths);
        let document = paths
            .game_instance_dir(GameId::Cs2, "dust")
            .join(crate::game::cs2::CONFIG_PATH);
        std::fs::create_dir_all(document.parent().unwrap()).unwrap();
        std::fs::write(&document, OPERATOR_CONFIG).unwrap();

        if start(&paths, &db, &instance).await.is_none() {
            return;
        }

        assert_eq!(std::fs::read_to_string(&document).unwrap(), OPERATOR_CONFIG);
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[tokio::test]
    async fn a_shared_instance_cannot_start_before_the_files_are_installed() {
        let paths = temp_paths("cs2-shared-missing");
        let db = Db::open(&paths).unwrap();
        let instance = game_instances::create_generic(&paths, &db, GameId::Cs2, "dust").unwrap();
        game_instances::set_install_mode(
            &db,
            &instance.identity.id,
            game_instances::InstallMode::Shared,
        )
        .unwrap();

        let error = dispatch::start(&paths, &db, GameId::Cs2, "dust")
            .await
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            "Install the game files from Games › Counter-Strike 2 before starting a shared instance"
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn preparing_the_layout_writes_the_runtime_spec_and_never_creates_the_document() {
        let paths = temp_paths("cs2-layout");
        let db = Db::open(&paths).unwrap();
        let instance = game_instances::create_generic(&paths, &db, GameId::Cs2, "dust").unwrap();

        dispatch::sync_spec(&paths, &db, GameId::Cs2, "dust").unwrap();
        dispatch::sync_spec(&paths, &db, GameId::Cs2, "dust").unwrap();

        assert!(
            super::super::runtime_spec_path(&paths, GameId::Cs2, &instance.identity.id).is_file()
        );
        // The container links or seeds it from the game's own file.
        assert!(
            !paths
                .game_instance_dir(GameId::Cs2, "dust")
                .join(crate::game::cs2::CONFIG_PATH)
                .exists()
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[tokio::test]
    async fn stop_sends_the_control_request_with_the_drivers_timeout() {
        let paths = temp_paths("cs2-stop");
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
        let instance = game_instances::create_generic(&paths, &db, GameId::Cs2, "dust").unwrap();
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

        dispatch::stop(&paths, &db, GameId::Cs2, "dust", StopReason::Operator)
            .await
            .unwrap();

        assert!(!container_instances::desired_running(&db, &instance.identity.id).unwrap());
        let request: serde_json::Value = serde_json::from_str(&runtime.join().unwrap()).unwrap();
        assert_eq!(request["type"], "stop");
        assert_eq!(request["timeout_secs"], 30);
        podman.join().unwrap();
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
