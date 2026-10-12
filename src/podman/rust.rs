//! Rust's side of the Podman lifecycle: the few facts that differ from other
//! games. The lifecycle itself lives in [`super::lifecycle`].

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::db::Db;
use crate::db::game_instances::{self, GameInstanceIdentity, RustInstance};
use crate::game::GameId;
use crate::game::rust::{self, RustLaunch};
use crate::paths::Paths;

use super::lifecycle::{self, ContainerGame, RuntimeHealth, StopReason};
use super::{Launch, Mount, write_runtime_spec};

struct Rust;

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

    fn extra_mounts(_paths: &Paths) -> Vec<Mount> {
        Vec::new()
    }

    fn running_directly(instance: &RustInstance) -> Result<bool> {
        Ok(rust::is_running_directly(instance))
    }
}

pub fn migrate(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    lifecycle::migrate::<Rust>(paths, db, name)
}

pub async fn runtime_health(paths: &Paths, db: &Db, name: &str) -> Result<RuntimeHealth> {
    lifecycle::runtime_health::<Rust>(paths, db, name).await
}

pub async fn repair(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    lifecycle::repair::<Rust>(paths, db, name).await
}

pub async fn update(paths: &Paths, db: &Db, name: &str) -> Result<()> {
    lifecycle::update::<Rust>(paths, db, name).await
}

pub fn mark_new_instance(db: &Db, instance: &RustInstance) -> Result<()> {
    lifecycle::mark_new_instance::<Rust>(db, instance)
}

pub fn is_migrated(db: &Db, instance: &RustInstance) -> Result<bool> {
    lifecycle::is_migrated::<Rust>(db, instance)
}

pub fn is_running(paths: &Paths, db: &Db, instance: &RustInstance) -> Result<bool> {
    lifecycle::is_running::<Rust>(paths, db, instance)
}

pub async fn start(paths: &Paths, db: &Db, name: &str) -> Result<RustInstance> {
    lifecycle::start::<Rust>(paths, db, name).await
}

pub async fn stop(paths: &Paths, db: &Db, name: &str, reason: StopReason) -> Result<()> {
    lifecycle::stop::<Rust>(paths, db, name, reason).await
}

pub async fn restart(paths: &Paths, db: &Db, name: &str) -> Result<RustInstance> {
    lifecycle::restart::<Rust>(paths, db, name).await
}

pub fn remove(paths: &Paths, db: &Db, instance: &RustInstance) -> Result<()> {
    lifecycle::remove::<Rust>(paths, db, instance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::container_instances;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixListener;

    fn temp_paths(label: &str) -> Paths {
        let root =
            std::env::temp_dir().join(format!("odin-podman-rust-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        Paths {
            config_dir: root.clone(),
            data_dir: root,
        }
    }

    fn write_podman_config(paths: &Paths, socket: &std::path::Path) {
        let config = crate::config::GlobalConfig {
            data_dir: None,
            podman: Some(crate::config::PodmanConfig {
                socket: socket.to_path_buf(),
                runtime_image: Some("example/odin-srv:1".into()),
            }),
        };
        std::fs::write(
            paths.config_file(),
            toml::to_string_pretty(&config).unwrap(),
        )
        .unwrap();
    }

    /// Answers each Podman request on a fresh connection until the container
    /// is started, returning `(request line, body)` for every request.
    fn fake_podman(listener: UnixListener) -> std::thread::JoinHandle<Vec<(String, String)>> {
        std::thread::spawn(move || {
            let mut requests = Vec::new();
            let mut created = false;
            loop {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let (status, reply) = if line.contains("/version") {
                    ("200 OK", r#"{"Version":"5.0.0","ApiVersion":"1.41"}"#)
                } else if line.contains("/exists") {
                    ("204 No Content", "")
                } else if line.contains("/containers/create") {
                    created = true;
                    ("201 Created", r#"{"Id":"abc"}"#)
                } else if line.contains("/json") && created {
                    (
                        "200 OK",
                        r#"{"State":{"Running":false,"Status":"created"}}"#,
                    )
                } else if line.contains("/json") {
                    ("404 Not Found", r#"{"cause":"no such container"}"#)
                } else {
                    ("204 No Content", "")
                };
                let started = line.contains("/start");
                requests.push((line.trim().to_string(), String::from_utf8(body).unwrap()));
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{reply}",
                            reply.len()
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                if started {
                    return requests;
                }
            }
        })
    }

    /// A runtime that answers Ping like a started `odin run`.
    fn fake_runtime(socket: &std::path::Path) -> std::thread::JoinHandle<()> {
        std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
        let listener = UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            let pong = serde_json::to_string(&crate::supervisor::protocol::Response::Pong {
                pid: 1,
                pid_started_at: 1,
                started_at: chrono::Utc::now(),
                protocol_version: crate::supervisor::protocol::PROTOCOL_VERSION,
                odin_version: Some("test".into()),
                ready: true,
            })
            .unwrap();
            stream.write_all(format!("{pong}\n").as_bytes()).unwrap();
        })
    }

    #[tokio::test]
    async fn start_creates_the_rust_container_and_prepares_its_native_layout() {
        let paths = temp_paths("start");
        let podman_socket = paths.data_dir.join("podman.sock");
        let listener = match UnixListener::bind(&podman_socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        write_podman_config(&paths, &podman_socket);
        let db = Db::open(&paths).unwrap();
        let instance = game_instances::create_rust(&paths, &db, "rusty").unwrap();
        mark_new_instance(&db, &instance).unwrap();
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

        start(&paths, &db, "rusty").await.unwrap();

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
    fn a_container_decides_whether_a_migrated_instance_is_running() {
        let paths = temp_paths("running");
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
        assert!(!is_running(&paths, &db, &instance).unwrap());

        container_instances::mark_podman(&db, &instance.identity.id).unwrap();

        assert!(is_running(&paths, &db, &instance).unwrap());
        let line = server.join().unwrap();
        assert!(line.contains(&format!("odin-rust-{}", instance.identity.id)));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn migration_prepares_the_layout_before_marking_the_instance() {
        let paths = temp_paths("migrate");
        let db = Db::open(&paths).unwrap();
        let instance = game_instances::create_rust(&paths, &db, "rusty").unwrap();
        assert!(!is_migrated(&db, &instance).unwrap());

        migrate(&paths, &db, "rusty").unwrap();
        migrate(&paths, &db, "rusty").unwrap();

        assert!(is_migrated(&db, &instance).unwrap());
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

    #[test]
    fn migration_refuses_a_server_still_running_on_the_host() {
        let paths = temp_paths("migrate-running");
        let db = Db::open(&paths).unwrap();
        let instance = game_instances::create_rust(&paths, &db, "rusty").unwrap();
        game_instances::set_rust_pid(
            &db,
            "rusty",
            std::process::id(),
            crate::instance::process::start_time_of(std::process::id()).unwrap(),
            chrono::Utc::now(),
        )
        .unwrap();

        assert!(migrate(&paths, &db, "rusty").is_err());
        assert!(!is_migrated(&db, &instance).unwrap());
        assert!(!runtime_spec_exists(&paths, &instance));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[tokio::test]
    async fn stop_gives_rust_the_time_its_world_save_needs() {
        let paths = temp_paths("stop");
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
        mark_new_instance(&db, &instance).unwrap();
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

        stop(&paths, &db, "rusty", StopReason::Operator)
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
