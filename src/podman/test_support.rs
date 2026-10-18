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
        curseforge: None,
        podman: Some(crate::config::PodmanConfig {
            socket: socket.to_path_buf(),
            runtime_image: Some("example/odin-srv:1".into()),
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

/// Answers each Podman request on a fresh connection until the container
/// is started, returning `(request line, body)` for every request.
pub(crate) fn fake_podman(
    listener: UnixListener,
) -> std::thread::JoinHandle<Vec<(String, String)>> {
    fake_podman_with_stopped_container(listener, false)
}

/// A minimal Podman API that can optionally expose a stopped container before
/// accepting a replacement container.
pub(crate) fn fake_podman_with_stopped_container(
    listener: UnixListener,
    initially_stopped: bool,
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
            } else if line.contains("/json") && initially_stopped {
                ("200 OK", r#"{"State":{"Running":false,"Status":"exited"}}"#)
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

/// Points `paths` at a Podman API that reports the containers of the given
/// instance ids as running and every other container as missing, so a test
/// can make an instance count as running. The server thread lives until the
/// test process exits. Returns `false` when the sandbox forbids Unix sockets,
/// in which case the test should skip itself.
pub(crate) fn podman_reporting_running(paths: &Paths, running_ids: &[&str]) -> bool {
    let socket = paths.data_dir.join("podman.sock");
    std::fs::create_dir_all(&paths.data_dir).unwrap();
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    let listener = match UnixListener::bind(&socket) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return false,
        Err(error) => panic!("failed to bind fake Podman socket: {error}"),
    };
    write_podman_config(paths, &socket);
    let running_ids: Vec<String> = running_ids.iter().map(ToString::to_string).collect();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                    break;
                }
            }
            let (status, body) = if line.contains("/version") {
                ("200 OK", r#"{"Version":"5.0.0","ApiVersion":"1.41"}"#)
            } else if running_ids.iter().any(|id| line.contains(id.as_str())) {
                ("200 OK", r#"{"State":{"Running":true,"Status":"running"}}"#)
            } else {
                ("404 Not Found", r#"{"cause":"no such container"}"#)
            };
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
    });
    true
}

/// A Podman API for a container that dies right after it is started: inspect
/// reports it `exited` with `exit_code` once `/start` was served, and `/logs`
/// returns `log_output` as a stdout frame. The server thread lives until the
/// test process exits.
pub(crate) fn fake_podman_crashing_container(
    listener: UnixListener,
    exit_code: i64,
    log_output: &str,
) {
    let mut frame = vec![1, 0, 0, 0];
    frame.extend((log_output.len() as u32).to_be_bytes());
    frame.extend(log_output.as_bytes());
    std::thread::spawn(move || {
        let mut created = false;
        let mut started = false;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut length = 0;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                    break;
                }
                if let Some((name, value)) = header.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse().unwrap();
                }
            }
            reader.read_exact(&mut vec![0; length]).unwrap();
            let exited = format!(
                r#"{{"State":{{"Running":false,"Status":"exited","ExitCode":{exit_code},"OOMKilled":false}}}}"#
            );
            let (status, reply): (&str, Vec<u8>) = if line.contains("/version") {
                (
                    "200 OK",
                    br#"{"Version":"5.0.0","ApiVersion":"1.41"}"#.to_vec(),
                )
            } else if line.contains("/exists") {
                ("204 No Content", Vec::new())
            } else if line.contains("/containers/create") {
                created = true;
                ("201 Created", br#"{"Id":"abc"}"#.to_vec())
            } else if line.contains("/logs") {
                ("200 OK", frame.clone())
            } else if line.contains("/json") && started {
                ("200 OK", exited.into_bytes())
            } else if line.contains("/json") && created {
                (
                    "200 OK",
                    br#"{"State":{"Running":false,"Status":"created"}}"#.to_vec(),
                )
            } else if line.contains("/json") {
                (
                    "404 Not Found",
                    br#"{"cause":"no such container"}"#.to_vec(),
                )
            } else {
                ("204 No Content", Vec::new())
            };
            started |= line.contains("/start");
            let head = format!(
                "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                reply.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&reply);
        }
    });
}
