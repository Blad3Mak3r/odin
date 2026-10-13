//! `odin run` implementation used inside an `odin-srv` container.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

use crate::game;
use crate::instance::process;
use crate::paths::{self, Paths};
use crate::steamcmd::SteamCmd;
use crate::supervisor::protocol::{
    ErrorCode, PROTOCOL_VERSION, Request, Response, read_frame, write_frame,
};
use crate::supervisor::stats::{StatsHandle, refresh_stats_periodically};

use super::RuntimeSpec;

enum Action {
    Stop(u64),
    Update(oneshot::Sender<Result<()>>),
}

pub async fn run(spec_path: &Path, expected_instance_id: &str) -> Result<()> {
    let spec: RuntimeSpec = serde_json::from_slice(
        &std::fs::read(spec_path)
            .with_context(|| format!("failed to read runtime spec {}", spec_path.display()))?,
    )
    .context("failed to decode runtime spec")?;
    if spec.protocol_version != RuntimeSpec::PROTOCOL_VERSION {
        bail!(
            "runtime spec protocol {} is incompatible with runtime protocol {}",
            spec.protocol_version,
            RuntimeSpec::PROTOCOL_VERSION
        );
    }
    if spec.instance_id != expected_instance_id {
        bail!("runtime spec UUID does not match --instance-id");
    }
    let paths = Paths {
        data_dir: spec.instance_dir.clone(),
        config_dir: spec.instance_dir.clone(),
    };
    let control_path =
        crate::supervisor::control_sock_path(&paths, spec.launch.game(), spec.launch.name());
    let control = bind_socket(&control_path)?;

    let result = run_loop(spec, control).await;
    let _ = std::fs::remove_file(control_path);
    result
}

fn bind_socket(path: &Path) -> Result<UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    let listener = UnixListener::bind(path)
        .with_context(|| format!("failed to bind runtime socket {}", path.display()))?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(listener)
}

fn label(spec: &RuntimeSpec) -> &'static str {
    game::driver(spec.launch.game()).display_name()
}

async fn run_loop(spec: RuntimeSpec, control: UnixListener) -> Result<()> {
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("failed to register SIGTERM handler")?;
    let (actions_tx, mut actions_rx) = mpsc::channel(4);
    // CPU needs a long-lived sampler: a `System` built per request reports
    // 0% inside a container's PID namespace.
    let stats: StatsHandle = Arc::new(Mutex::new(None));

    // The first start downloads the whole game, which can take longer than
    // the host is willing to wait for a first answer: keep serving control
    // requests (and shutdown signals) while that happens.
    let runtime_pid = std::process::id();
    let runtime_started_at = process::start_time_of(runtime_pid)?;
    let runtime_started = Utc::now();
    {
        let bootstrap = bootstrap(&spec);
        tokio::pin!(bootstrap);
        loop {
            tokio::select! {
                result = &mut bootstrap => {
                    result?;
                    break;
                }
                _ = tokio::signal::ctrl_c() => return Ok(()),
                _ = sigterm.recv() => return Ok(()),
                Ok((stream, _)) = control.accept() => {
                    tokio::spawn(handle_control(
                        stream, runtime_pid, runtime_started_at, runtime_started, false,
                        actions_tx.clone(), stats.clone(),
                    ));
                }
                Some(action) = actions_rx.recv() => match action {
                    Action::Stop(_) => return Ok(()),
                    Action::Update(reply) => {
                        let _ = reply.send(Err(anyhow!("{} is still installing", label(&spec))));
                    }
                }
            }
        }
    }

    let mut child = spawn_child(&spec).await?;
    let mut pid = child
        .id()
        .with_context(|| format!("{} child has no PID", label(&spec)))?;
    let mut started_at = Utc::now();
    let mut pid_started_at = process::start_time_of(pid)?;
    let mut stats_refresher = tokio::spawn(refresh_stats_periodically(pid, stats.clone()));

    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status
                    .with_context(|| format!("failed to wait for {} child", label(&spec)))?;
                if !status.success() {
                    bail!("{} exited with {status}", label(&spec));
                }
                return Ok(());
            }
            _ = tokio::signal::ctrl_c() => {
                stop_child(&mut child, pid, pid_started_at, stop_timeout(&spec), &spec).await?;
                return Ok(());
            }
            _ = sigterm.recv() => {
                stop_child(&mut child, pid, pid_started_at, stop_timeout(&spec), &spec).await?;
                return Ok(());
            }
            Ok((stream, _)) = control.accept() => {
                tokio::spawn(handle_control(
                    stream, pid, pid_started_at, started_at, true,
                    actions_tx.clone(), stats.clone(),
                ));
            }
            Some(action) = actions_rx.recv() => match action {
                Action::Stop(timeout) => {
                    stop_child(
                        &mut child, pid, pid_started_at, Duration::from_secs(timeout), &spec,
                    ).await?;
                    return Ok(());
                }
                Action::Update(reply) => {
                    let update = async {
                        stop_child(&mut child, pid, pid_started_at, stop_timeout(&spec), &spec)
                            .await?;
                        update_payload(&spec).await?;
                        child = spawn_child(&spec).await?;
                        pid = child
                            .id()
                            .with_context(|| format!("restarted {} child has no PID", label(&spec)))?;
                        pid_started_at = process::start_time_of(pid)?;
                        started_at = Utc::now();
                        // The old sampler still tracks the dead pid.
                        stats_refresher.abort();
                        *stats.lock().expect("stats lock poisoned") = None;
                        stats_refresher = tokio::spawn(refresh_stats_periodically(pid, stats.clone()));
                        Ok(())
                    }.await;
                    let failed = update.is_err();
                    let _ = reply.send(update);
                    if failed {
                        // A failed SteamCMD run must never leave an idle
                        // runtime looking healthy to the host.
                        bail!("{} update failed; runtime is stopping", label(&spec));
                    }
                }
            }
        }
    }
}

/// The game's own budget for a graceful stop, used whenever the runtime is
/// stopped by a signal or an update rather than an explicit timeout.
fn stop_timeout(spec: &RuntimeSpec) -> Duration {
    Duration::from_secs(game::driver(spec.launch.game()).stop_timeout_secs())
}

fn server_file(spec: &RuntimeSpec) -> PathBuf {
    spec.install_dir
        .join(game::driver(spec.launch.game()).server_binary())
}

async fn bootstrap(spec: &RuntimeSpec) -> Result<()> {
    if spec.shared_install {
        anyhow::ensure!(
            server_file(spec).is_file(),
            "Install the game files from Games › ARK: Survival Ascended before starting a shared instance"
        );
        return Ok(());
    }
    if server_file(spec).is_file() {
        return Ok(());
    }
    seed_steamcmd(&spec.steamcmd_dir)?;
    update_payload(spec).await
}

pub(crate) fn seed_steamcmd(destination: &Path) -> Result<()> {
    if destination.join("steamcmd.sh").is_file() {
        return Ok(());
    }
    let source = Path::new("/usr/local/share/odin/steamcmd");
    if !source.join("steamcmd.sh").is_file() {
        // Development builds can still use SteamCmd's normal downloader.
        return Ok(());
    }
    copy_tree(source, destination).context("failed to seed private SteamCMD runtime")
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if entry.file_type()?.is_file() {
            std::fs::copy(&source_path, &destination_path)
                .with_context(|| format!("failed to copy {}", source_path.display()))?;
            std::fs::set_permissions(
                &destination_path,
                std::fs::metadata(&source_path)?.permissions(),
            )?;
        }
    }
    Ok(())
}

async fn update_payload(spec: &RuntimeSpec) -> Result<()> {
    anyhow::ensure!(
        !spec.shared_install,
        "shared installs are updated from Games › ARK: Survival Ascended"
    );
    let steamcmd = spec.steamcmd_dir.clone();
    let install = spec.install_dir.clone();
    let log = spec.update_log.clone();
    let expected = server_file(spec);
    let app_id = game::driver(spec.launch.game()).steam_app_id();
    tokio::task::spawn_blocking(move || {
        SteamCmd::new(steamcmd).update_app_expect_file(app_id, &install, &log, expected, |_| {})
    })
    .await
    .context("runtime update task failed")?
}

async fn spawn_child(spec: &RuntimeSpec) -> Result<tokio::process::Child> {
    let logs = paths::instance_logs_dir(&spec.instance_dir);
    std::fs::create_dir_all(&logs)?;
    let console_log = logs.join("console.log");
    let mut command = spec.launch.server_command(spec, &console_log)?;
    command
        .spawn()
        .context("failed to spawn game server process")
}

async fn stop_child(
    child: &mut tokio::process::Child,
    pid: u32,
    started_at: i64,
    timeout: Duration,
    spec: &RuntimeSpec,
) -> Result<()> {
    let quit_sent = spec.launch.request_graceful_stop(&spec.instance_dir).await;
    if !quit_sent {
        let _ = process::send_signal(pid, started_at, sysinfo::Signal::Interrupt);
    }
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => {
            status.context("failed to wait for game shutdown")?;
        }
        Err(_) => {
            let _ = process::send_signal(pid, started_at, sysinfo::Signal::Kill);
            child
                .wait()
                .await
                .context("failed to reap killed game child")?;
        }
    };
    Ok(())
}

async fn handle_control(
    stream: UnixStream,
    pid: u32,
    pid_started_at: i64,
    started_at: chrono::DateTime<Utc>,
    ready: bool,
    actions: mpsc::Sender<Action>,
    stats: StatsHandle,
) {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let response = match read_frame::<Request, _>(&mut reader).await {
        Ok(Some(Request::Ping)) => Response::Pong {
            pid,
            pid_started_at,
            started_at,
            protocol_version: PROTOCOL_VERSION,
            odin_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            ready,
        },
        Ok(Some(Request::Stop { timeout_secs })) => {
            let _ = actions.send(Action::Stop(timeout_secs)).await;
            Response::Stopped
        }
        Ok(Some(Request::Update)) => {
            let (reply_tx, reply_rx) = oneshot::channel();
            if actions.send(Action::Update(reply_tx)).await.is_err() {
                Response::Error {
                    code: ErrorCode::Unknown,
                    message: "runtime is stopping".into(),
                }
            } else {
                match reply_rx.await {
                    Ok(Ok(())) => Response::Updated,
                    Ok(Err(error)) => Response::Error {
                        code: ErrorCode::Unknown,
                        message: error.to_string(),
                    },
                    Err(_) => Response::Error {
                        code: ErrorCode::Unknown,
                        message: "runtime stopped during update".into(),
                    },
                }
            }
        }
        Ok(Some(Request::Stats)) => match *stats.lock().expect("stats lock poisoned") {
            Some((cpu_percent, memory_bytes)) => Response::Stats {
                cpu_percent,
                memory_bytes,
            },
            None => Response::Error {
                code: ErrorCode::StatsNotReady,
                message: "stats not yet available".into(),
            },
        },
        Ok(None) => return,
        Err(error) => Response::Error {
            code: ErrorCode::Unknown,
            message: format!("invalid request: {error}"),
        },
    };
    let _ = write_frame(&mut write, &response).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::rust;
    use crate::podman::Launch;
    use crate::supervisor::stats::STATS_REFRESH_INTERVAL;

    async fn stats_response(stats: StatsHandle, pid: u32) -> Response {
        let (client, server) = UnixStream::pair().unwrap();
        let (actions, _receiver) = mpsc::channel(1);
        let handler = tokio::spawn(handle_control(
            server,
            pid,
            0,
            Utc::now(),
            true,
            actions,
            stats,
        ));
        let (read, mut write) = client.into_split();
        write_frame(&mut write, &Request::Stats).await.unwrap();
        let response = read_frame::<Response, _>(&mut BufReader::new(read))
            .await
            .unwrap()
            .unwrap();
        handler.await.unwrap();
        response
    }

    #[tokio::test]
    async fn stats_report_cpu_for_a_busy_child() {
        let mut child = tokio::process::Command::new("sh")
            .args(["-c", "while :; do :; done"])
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        let stats: StatsHandle = Arc::new(Mutex::new(None));
        let refresher = tokio::spawn(refresh_stats_periodically(pid, stats.clone()));

        // Past one refresh interval the sampler holds a CPU delta; a
        // freshly built `System` can never provide one.
        tokio::time::sleep(STATS_REFRESH_INTERVAL + std::time::Duration::from_secs(1)).await;
        let response = stats_response(stats, pid).await;

        refresher.abort();
        child.kill().await.unwrap();
        assert!(
            matches!(response, Response::Stats { cpu_percent, memory_bytes } if cpu_percent > 0.0 && memory_bytes > 0),
            "expected nonzero CPU and memory, got {response:?}"
        );
    }

    use std::os::unix::fs::PermissionsExt;

    fn script(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[tokio::test]
    async fn runtime_answers_while_the_game_is_still_installing() {
        let root = std::env::temp_dir().join(format!("odin-runtime-{}", uuid::Uuid::new_v4()));
        // A fake SteamCMD that takes a moment, then installs a fake server.
        script(
            &root.join("runtime/steamcmd/steamcmd.sh"),
            r#"for a in "$@"; do case "$a" in "+force_install_dir "*) d="${a#+force_install_dir }";; esac; done
sleep 2
mkdir -p "$d"
printf '#!/bin/sh\nexec sleep 30\n' > "$d/valheim_server.x86_64"
chmod +x "$d/valheim_server.x86_64""#,
        );
        let spec = RuntimeSpec {
            protocol_version: RuntimeSpec::PROTOCOL_VERSION,
            instance_id: "id".into(),
            instance_dir: root.join("instance"),
            install_dir: root.join("runtime/valheim"),
            steamcmd_dir: root.join("runtime/steamcmd"),
            update_log: root.join("instance/logs/update.log"),
            shared_install: false,
            launch: Launch::Valheim {
                instance: crate::instance::state::InstanceState::new("t", 2456),
            },
        };
        let control_path = root.join("c.sock");
        let control = bind_socket(&control_path).unwrap();
        let runtime = tokio::spawn(run_loop(spec, control));

        let installing = crate::supervisor::client::ping_socket_with_retry(
            &control_path,
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert!(matches!(installing, Response::Pong { ready: false, .. }));

        let ready = loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let pong = crate::supervisor::client::ping_socket_with_retry(
                &control_path,
                Duration::from_secs(1),
            )
            .await
            .unwrap();
            if matches!(pong, Response::Pong { ready: true, .. }) {
                break pong;
            }
        };
        assert!(matches!(ready, Response::Pong { ready: true, .. }));

        crate::supervisor::client::stop_socket(&control_path, 5)
            .await
            .unwrap();
        runtime.await.unwrap().unwrap();
        std::fs::remove_dir_all(root).ok();
    }

    fn rust_launch(rcon_port: u16) -> Launch {
        Launch::Rust(rust::RustLaunch {
            name: "rusty".into(),
            identity: "id".into(),
            port: 1,
            query_port: 2,
            rcon_port,
            rcon_password: "secret".into(),
        })
    }

    fn spec_for(launch: Launch) -> RuntimeSpec {
        RuntimeSpec {
            protocol_version: RuntimeSpec::PROTOCOL_VERSION,
            instance_id: "id".into(),
            instance_dir: PathBuf::from("/nonexistent/instance"),
            install_dir: PathBuf::from("/nonexistent/install"),
            steamcmd_dir: PathBuf::from("/nonexistent/steamcmd"),
            update_log: PathBuf::from("/nonexistent/update.log"),
            shared_install: false,
            launch,
        }
    }

    fn spawn_shell(body: &str) -> (tokio::process::Child, u32, i64) {
        let child = tokio::process::Command::new("sh")
            .args(["-c", body])
            .spawn()
            .unwrap();
        let pid = child.id().unwrap();
        (child, pid, process::start_time_of(pid).unwrap())
    }

    #[tokio::test]
    async fn rust_stop_asks_the_console_to_save_instead_of_signalling() {
        use futures_util::StreamExt;

        let flag = std::env::temp_dir().join(format!("odin-sigint-{}", uuid::Uuid::new_v4()));
        let (mut child, pid, started_at) = spawn_shell(&format!(
            "trap 'touch {}' INT; trap 'exit 0' TERM; while :; do sleep 0.1; done",
            flag.display()
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        // The fake console ends the "server" once it receives the command.
        let console = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            let message = socket.next().await.unwrap().unwrap().into_text().unwrap();
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            message.to_string()
        });

        stop_child(
            &mut child,
            pid,
            started_at,
            Duration::from_secs(10),
            &spec_for(rust_launch(port)),
        )
        .await
        .unwrap();

        let command = console.await.unwrap();
        assert!(command.contains("\"Message\":\"quit\""), "got {command}");
        assert!(
            !flag.exists(),
            "SIGINT must not be sent when quit was delivered"
        );
    }

    #[tokio::test]
    async fn rust_stop_falls_back_to_sigint_when_the_console_is_unreachable() {
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let (mut child, pid, started_at) =
            spawn_shell("trap 'exit 0' INT; while :; do sleep 0.1; done");

        let begun = std::time::Instant::now();
        stop_child(
            &mut child,
            pid,
            started_at,
            Duration::from_secs(10),
            &spec_for(rust_launch(port)),
        )
        .await
        .unwrap();

        assert!(begun.elapsed() < Duration::from_secs(5));
    }

    fn seven_days_instance_dir(telnet_port: u16) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("odin-7d2d-stop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(
            dir.join("config/serverconfig.xml"),
            format!(
                r#"<ServerSettings>
<property name="TelnetEnabled" value="true"/>
<property name="TelnetPort" value="{telnet_port}"/>
<property name="TelnetPassword" value="secret"/>
</ServerSettings>"#
            ),
        )
        .unwrap();
        dir
    }

    fn seven_days_spec(instance_dir: PathBuf) -> RuntimeSpec {
        RuntimeSpec {
            instance_dir,
            ..spec_for(Launch::SevenDaysToDie(
                crate::game::seven_days_to_die::SevenDaysLaunch {
                    name: "undead".into(),
                    port: 26900,
                    telnet_port: None,
                },
            ))
        }
    }

    #[tokio::test]
    async fn seven_days_stop_asks_the_console_to_shut_down_instead_of_signalling() {
        use std::io::{BufRead, Write};

        let flag = std::env::temp_dir().join(format!("odin-sigint-{}", uuid::Uuid::new_v4()));
        let (mut child, pid, started_at) = spawn_shell(&format!(
            "trap 'touch {}' INT; trap 'exit 0' TERM; while :; do sleep 0.1; done",
            flag.display()
        ));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // A fake console: greeting, password, then the command, which ends
        // the "server".
        let console = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            stream.write_all(b"Please enter password:\r\n").unwrap();
            let mut password = String::new();
            reader.read_line(&mut password).unwrap();
            stream.write_all(b"Logon successful.\r\n").unwrap();
            let mut command = String::new();
            reader.read_line(&mut command).unwrap();
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            (password, command)
        });
        let spec = seven_days_spec(seven_days_instance_dir(port));

        stop_child(&mut child, pid, started_at, Duration::from_secs(10), &spec)
            .await
            .unwrap();

        let (password, command) = console.join().unwrap();
        assert_eq!(password.trim(), "secret");
        assert_eq!(command.trim(), "shutdown");
        assert!(
            !flag.exists(),
            "SIGINT must not be sent when shutdown was delivered"
        );
    }

    #[tokio::test]
    async fn seven_days_stop_falls_back_to_sigint_when_the_console_is_unreachable() {
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let (mut child, pid, started_at) =
            spawn_shell("trap 'exit 0' INT; while :; do sleep 0.1; done");
        let spec = seven_days_spec(seven_days_instance_dir(port));

        let begun = std::time::Instant::now();
        stop_child(&mut child, pid, started_at, Duration::from_secs(10), &spec)
            .await
            .unwrap();

        assert!(begun.elapsed() < Duration::from_secs(8));
    }

    #[tokio::test]
    async fn seven_days_stop_without_an_enabled_console_signals_the_server() {
        let (mut child, pid, started_at) =
            spawn_shell("trap 'exit 0' INT; while :; do sleep 0.1; done");
        // No generated configuration, so there is no console to ask.
        let spec = seven_days_spec(std::env::temp_dir().join("odin-7d2d-no-config"));

        stop_child(&mut child, pid, started_at, Duration::from_secs(10), &spec)
            .await
            .unwrap();
    }

    fn palworld_instance_dir(rest_port: u16, enabled: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("odin-palworld-stop-{}", uuid::Uuid::new_v4()));
        let config = dir.join(crate::game::palworld::CONFIG_FILE);
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(
            config,
            format!(
                "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(RESTAPIEnabled={},AdminPassword=\"secret\",RESTAPIPort={rest_port})\n",
                if enabled { "True" } else { "False" }
            ),
        )
        .unwrap();
        dir
    }

    fn palworld_spec(instance_dir: PathBuf) -> RuntimeSpec {
        RuntimeSpec {
            instance_dir,
            ..spec_for(Launch::Palworld(crate::game::palworld::PalworldLaunch {
                name: "pals".into(),
                port: 8211,
                query_port: 27015,
            }))
        }
    }

    #[tokio::test]
    async fn palworld_stop_asks_the_rest_api_to_shut_down_instead_of_signalling() {
        use std::io::{BufRead, Read, Write};

        let flag = std::env::temp_dir().join(format!("odin-sigint-{}", uuid::Uuid::new_v4()));
        let (mut child, pid, started_at) = spawn_shell(&format!(
            "trap 'touch {}' INT; trap 'exit 0' TERM; while :; do sleep 0.1; done",
            flag.display()
        ));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // A fake REST API: records the request, answers, and ends the "server".
        let rest = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse().unwrap();
                }
                request.push_str(&line);
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            (request, String::from_utf8(body).unwrap())
        });
        let spec = palworld_spec(palworld_instance_dir(port, true));

        stop_child(&mut child, pid, started_at, Duration::from_secs(10), &spec)
            .await
            .unwrap();

        let (request, body) = rest.join().unwrap();
        assert!(
            request.starts_with("POST /v1/api/shutdown "),
            "got {request}"
        );
        // Basic auth for admin:secret.
        assert!(request.contains("authorization: Basic YWRtaW46c2VjcmV0"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({"waittime": 0})
        );
        assert!(
            !flag.exists(),
            "SIGINT must not be sent when shutdown was delivered"
        );
    }

    #[tokio::test]
    async fn palworld_stop_falls_back_to_sigint_when_the_rest_api_is_unreachable() {
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let (mut child, pid, started_at) =
            spawn_shell("trap 'exit 0' INT; while :; do sleep 0.1; done");
        let spec = palworld_spec(palworld_instance_dir(port, true));

        let begun = std::time::Instant::now();
        stop_child(&mut child, pid, started_at, Duration::from_secs(10), &spec)
            .await
            .unwrap();

        assert!(begun.elapsed() < Duration::from_secs(8));
    }

    #[tokio::test]
    async fn palworld_stop_with_the_rest_api_disabled_signals_the_server() {
        let (mut child, pid, started_at) =
            spawn_shell("trap 'exit 0' INT; while :; do sleep 0.1; done");
        let spec = palworld_spec(palworld_instance_dir(1, false));

        stop_child(&mut child, pid, started_at, Duration::from_secs(10), &spec)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ping_during_bootstrap_reports_not_ready() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let (actions, _rx) = mpsc::channel(1);
        let handler = tokio::spawn(handle_control(
            server,
            7,
            11,
            Utc::now(),
            false,
            actions,
            Arc::new(Mutex::new(None)),
        ));

        write_frame(&mut client, &Request::Ping).await.unwrap();
        let mut reader = BufReader::new(&mut client);
        let response = read_frame::<Response, _>(&mut reader).await.unwrap();
        handler.await.unwrap();

        assert!(matches!(
            response,
            Some(Response::Pong {
                pid: 7,
                ready: false,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn stats_are_not_ready_before_the_first_sample() {
        let response = stats_response(Arc::new(Mutex::new(None)), 1).await;
        assert!(matches!(
            response,
            Response::Error {
                code: ErrorCode::StatsNotReady,
                ..
            }
        ));
    }
}
