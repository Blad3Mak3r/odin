//! Rust's side of the Podman lifecycle: the few facts that differ from other
//! games. The lifecycle itself lives in [`super::lifecycle`].

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::db::Db;
use crate::db::game_instances::{self, GameInstanceIdentity, RustInstance};
use crate::game::GameId;
use crate::game::rust::{self, RustLaunch};
use crate::paths::Paths;

use super::lifecycle::{self, ContainerGame};
use super::{Launch, Mount, write_runtime_spec};

pub(super) struct Rust;

impl ContainerGame for Rust {
    const GAME: GameId = GameId::Rust;
    type Instance = RustInstance;

    fn load(_paths: &Paths, db: &Db, name: &str) -> Result<RustInstance> {
        game_instances::load_rust(db, name)?.context("Rust instance does not exist")
    }

    fn identity(_db: &Db, instance: &RustInstance) -> Result<GameInstanceIdentity> {
        Ok(instance.identity.clone())
    }

    fn instance_dir(paths: &Paths, instance: &RustInstance) -> PathBuf {
        paths.game_instance_dir(GameId::Rust, instance.name())
    }

    fn ports(instance: &RustInstance) -> Result<Vec<u16>> {
        Ok(vec![
            instance.config.port,
            instance.config.query_port,
            instance.config.rcon_port,
        ])
    }

    fn prepare_layout(
        paths: &Paths,
        _db: &Db,
        identity: &GameInstanceIdentity,
        instance: &RustInstance,
    ) -> Result<()> {
        std::fs::create_dir_all(Self::instance_dir(paths, instance).join("logs"))?;
        std::fs::create_dir_all(rust::identity_dir(paths, instance))?;
        std::fs::create_dir_all(paths.container_runtime_dir(GameId::Rust, &identity.id))?;
        std::fs::create_dir_all(paths.container_socket_dir(&identity.id))?;
        // The native document stays authoritative; only Odin-owned transport
        // keys that already exist in it are brought in line.
        crate::game::config_documents::sync_rust_operational(paths, instance)?;
        write_runtime_spec(paths, identity, Launch::Rust(RustLaunch::from(instance)))?;
        Ok(())
    }

    fn extra_mounts(_paths: &Paths, _db: &Db, _instance: &RustInstance) -> Result<Vec<Mount>> {
        Ok(Vec::new())
    }
}

pub fn is_running(paths: &Paths, db: &Db, instance: &RustInstance) -> Result<bool> {
    lifecycle::is_running::<Rust>(paths, db, instance)
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

    #[tokio::test]
    async fn start_creates_the_rust_container_and_prepares_its_native_layout() {
        let paths = temp_paths("rust-start");
        let podman_socket = paths.data_dir.join("podman.sock");
        let listener = match UnixListener::bind(&podman_socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        write_podman_config(&paths, &podman_socket);
        let db = Db::open(&paths).unwrap();
        let instance = game_instances::create_rust(&paths, &db, "rusty").unwrap();
        // An operator's native document must keep its own values and keys.
        let cfg = rust::identity_dir(&paths, &instance).join("cfg/server.cfg");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(&cfg, "server.hostname \"Mine\"\nserver.port 1\n").unwrap();
        let podman = fake_podman(listener);
        let runtime = fake_runtime(
            &paths
                .container_socket_dir(&instance.identity.id)
                .join(format!("rust-{}.control.sock", instance.name())),
        );

        dispatch::start(&paths, &db, GameId::Rust, "rusty")
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
        assert_eq!(body["name"], format!("odin-rust-{id}"));
        assert_eq!(body["image"], "example/odin-srv:1");
        assert_eq!(
            body["command"],
            serde_json::json!(["run", "--instance-id", id])
        );
        assert_eq!(body["netns"]["nsmode"], "host");
        assert_eq!(body["labels"]["io.odin.game"], "rust");
        let mounts: Vec<_> = body["mounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|mount| mount["destination"].as_str().unwrap())
            .collect();
        assert_eq!(
            mounts,
            ["/srv/odin/runtime", "/srv/odin/instance", "/run/odin"]
        );
        let spec: serde_json::Value = serde_json::from_slice(
            &std::fs::read(super::super::runtime_spec_path(&paths, GameId::Rust, id)).unwrap(),
        )
        .unwrap();
        assert_eq!(spec["game"], "rust");
        assert_eq!(spec["identity"], *id);
        assert_eq!(spec["rcon_port"], instance.config.rcon_port);
        // The container flow reconciles only the transport keys it finds.
        let document = std::fs::read_to_string(&cfg).unwrap();
        assert!(document.contains("server.hostname \"Mine\""));
        assert!(document.contains(&format!("server.port {}", instance.config.port)));
        assert!(!document.contains("rcon.port"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn a_container_decides_whether_an_instance_is_running() {
        let paths = temp_paths("rust-running");
        let podman_socket = paths.data_dir.join("podman.sock");
        let listener = match UnixListener::bind(&podman_socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
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
            line
        });
        write_podman_config(&paths, &podman_socket);
        let db = Db::open(&paths).unwrap();
        let instance = game_instances::create_rust(&paths, &db, "rusty").unwrap();

        assert!(is_running(&paths, &db, &instance).unwrap());
        let line = server.join().unwrap();
        assert!(line.contains(&format!("odin-rust-{}", instance.identity.id)));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn preparing_the_layout_writes_the_runtime_spec_and_keeps_existing_data() {
        let paths = temp_paths("rust-layout");
        let db = Db::open(&paths).unwrap();
        let instance = game_instances::create_rust(&paths, &db, "rusty").unwrap();

        dispatch::sync_spec(&paths, &db, GameId::Rust, "rusty").unwrap();
        dispatch::sync_spec(&paths, &db, GameId::Rust, "rusty").unwrap();

        assert!(runtime_spec_exists(&paths, &instance));
        assert!(
            paths
                .game_instance_dir(GameId::Rust, "rusty")
                .join("server")
                .is_dir()
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    fn runtime_spec_exists(paths: &Paths, instance: &RustInstance) -> bool {
        super::super::runtime_spec_path(paths, GameId::Rust, &instance.identity.id).is_file()
    }

    #[tokio::test]
    async fn stop_gives_rust_the_time_its_world_save_needs() {
        let paths = temp_paths("rust-stop");
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
        let instance = game_instances::create_rust(&paths, &db, "rusty").unwrap();
        let control = paths
            .container_socket_dir(&instance.identity.id)
            .join(format!("rust-{}.control.sock", instance.name()));
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

        dispatch::stop(&paths, &db, GameId::Rust, "rusty", StopReason::Operator)
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
