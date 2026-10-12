//! Fakes shared by the per-game container lifecycle tests: a Podman API that
//! answers over a Unix socket and a runtime that answers Ping.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;

use crate::paths::Paths;

pub(crate) fn temp_paths(label: &str) -> Paths {
    let root = std::env::temp_dir().join(format!("odin-podman-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    Paths {
        config_dir: root.clone(),
        data_dir: root,
    }
}

pub(crate) fn write_podman_config(paths: &Paths, socket: &std::path::Path) {
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
pub(crate) fn fake_podman(
    listener: UnixListener,
) -> std::thread::JoinHandle<Vec<(String, String)>> {
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
pub(crate) fn fake_runtime(socket: &std::path::Path) -> std::thread::JoinHandle<()> {
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
