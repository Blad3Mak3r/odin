//! `odin run` implementation used inside an `odin-srv` container.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use tokio::io::BufReader;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

use crate::instance::{Instance, process};
use crate::paths::{self, Paths};
use crate::steamcmd::{SteamCmd, VALHEIM_DEDICATED_SERVER_APP_ID};
use crate::supervisor::protocol::{
    ErrorCode, PROTOCOL_VERSION, Request, Response, read_frame, write_frame,
};

use super::RuntimeSpec;

const STOP_TIMEOUT: Duration = Duration::from_secs(30);

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
    let control_path = crate::supervisor::control_sock_path(
        &paths,
        crate::game::GameId::Valheim,
        &spec.instance.name,
    );
    let events_path = crate::supervisor::events_sock_path(
        &paths,
        crate::game::GameId::Valheim,
        &spec.instance.name,
    );
    let control = bind_socket(&control_path)?;
    let events = bind_socket(&events_path)?;

    let result = run_loop(spec, control, events).await;
    let _ = std::fs::remove_file(control_path);
    let _ = std::fs::remove_file(events_path);
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

async fn run_loop(spec: RuntimeSpec, control: UnixListener, events: UnixListener) -> Result<()> {
    if !bootstrap_while_serving(&spec, &control, &events).await? {
        return Ok(());
    }
    let mut child = spawn_child(&spec).await?;
    let mut pid = child.id().context("Valheim child has no PID")?;
    let mut started_at = Utc::now();
    let mut pid_started_at = process::start_time_of(pid)?;
    let (actions_tx, mut actions_rx) = mpsc::channel(4);
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("failed to register SIGTERM handler")?;

    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.context("failed to wait for Valheim child")?;
                if !status.success() {
                    bail!("Valheim exited with {status}");
                }
                return Ok(());
            }
            _ = tokio::signal::ctrl_c() => {
                stop_child(&mut child, pid, pid_started_at, STOP_TIMEOUT).await?;
                return Ok(());
            }
            _ = sigterm.recv() => {
                stop_child(&mut child, pid, pid_started_at, STOP_TIMEOUT).await?;
                return Ok(());
            }
            Ok((stream, _)) = control.accept() => {
                tokio::spawn(handle_control(stream, pid, pid_started_at, started_at, true, actions_tx.clone()));
            }
            Ok((stream, _)) = events.accept() => {
                // A connection is valid but this lightweight container runtime
                // has no separate event producer; close it immediately so the
                // host falls back to its existing log tail reader.
                drop(stream);
            }
            Some(action) = actions_rx.recv() => match action {
                Action::Stop(timeout) => {
                    stop_child(&mut child, pid, pid_started_at, Duration::from_secs(timeout)).await?;
                    return Ok(());
                }
                Action::Update(reply) => {
                    let update = async {
                        stop_child(&mut child, pid, pid_started_at, STOP_TIMEOUT).await?;
                        update_payload(&spec).await?;
                        child = spawn_child(&spec).await?;
                        pid = child.id().context("restarted Valheim child has no PID")?;
                        pid_started_at = process::start_time_of(pid)?;
                        started_at = Utc::now();
                        Ok(())
                    }.await;
                    let failed = update.is_err();
                    let _ = reply.send(update);
                    if failed {
                        // A failed SteamCMD run must never leave an idle
                        // runtime looking healthy to the host.
                        bail!("Valheim update failed; runtime is stopping");
                    }
                }
            }
        }
    }
}

/// Runs `bootstrap` (a first-start SteamCMD download that can take minutes)
/// while still answering the host's control requests, so a `Ping` gets a
/// `ready: false` `Pong` instead of queueing unanswered until the 2s client
/// timeout. Returns `false` when a stop was requested before the server was
/// installed, in which case there is nothing to run.
async fn bootstrap_while_serving(
    spec: &RuntimeSpec,
    control: &UnixListener,
    events: &UnixListener,
) -> Result<bool> {
    let pid = std::process::id();
    let pid_started_at = process::start_time_of(pid)?;
    let started_at = Utc::now();
    let (actions_tx, mut actions_rx) = mpsc::channel(4);
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("failed to register SIGTERM handler")?;
    let bootstrap = bootstrap(spec);
    tokio::pin!(bootstrap);

    loop {
        tokio::select! {
            result = &mut bootstrap => return result.map(|()| true),
            _ = tokio::signal::ctrl_c() => return Ok(false),
            _ = sigterm.recv() => return Ok(false),
            Ok((stream, _)) = control.accept() => {
                tokio::spawn(handle_control(stream, pid, pid_started_at, started_at, false, actions_tx.clone()));
            }
            Ok((stream, _)) = events.accept() => drop(stream),
            Some(action) = actions_rx.recv() => match action {
                Action::Stop(_) => return Ok(false),
                Action::Update(reply) => {
                    let _ = reply.send(Err(anyhow::anyhow!(
                        "the Valheim server is still being installed"
                    )));
                }
            }
        }
    }
}

async fn bootstrap(spec: &RuntimeSpec) -> Result<()> {
    let binary = spec.install_dir.join("valheim_server.x86_64");
    if binary.is_file() {
        return Ok(());
    }
    seed_steamcmd(&spec.steamcmd_dir)?;
    update_payload(spec).await
}

fn seed_steamcmd(destination: &Path) -> Result<()> {
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
    let steamcmd = spec.steamcmd_dir.clone();
    let install = spec.install_dir.clone();
    let log = spec.update_log.clone();
    tokio::task::spawn_blocking(move || {
        SteamCmd::new(steamcmd).update_app_expect_file(
            VALHEIM_DEDICATED_SERVER_APP_ID,
            &install,
            &log,
            install.join("valheim_server.x86_64"),
            |_| {},
        )
    })
    .await
    .context("runtime update task failed")?
}

async fn spawn_child(spec: &RuntimeSpec) -> Result<tokio::process::Child> {
    std::fs::create_dir_all(paths::instance_logs_dir(&spec.instance_dir))?;
    let instance = Instance {
        dir: spec.instance_dir.clone(),
        state: spec.instance.clone(),
    };
    let command = crate::game::valheim::build_command_at(
        &instance,
        &spec.install_dir,
        &paths::instance_saves_dir(&spec.instance_dir),
        &paths::instance_logs_dir(&spec.instance_dir).join("console.log"),
    )?;
    process::spawn_with_cgroup(command, None).await
}

async fn stop_child(
    child: &mut tokio::process::Child,
    pid: u32,
    started_at: i64,
    timeout: Duration,
) -> Result<()> {
    let _ = process::send_signal(pid, started_at, sysinfo::Signal::Interrupt);
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => {
            status.context("failed to wait for Valheim shutdown")?;
        }
        Err(_) => {
            let _ = process::send_signal(pid, started_at, sysinfo::Signal::Kill);
            child
                .wait()
                .await
                .context("failed to reap killed Valheim child")?;
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
        Ok(Some(Request::Stats)) => {
            let mut system = sysinfo::System::new_all();
            system.refresh_all();
            let mut cpu_percent = 0.0;
            let mut memory_bytes = 0;
            for child_pid in crate::instance::process::descendant_pids(&system, &[pid]) {
                if let Some(process) = system.process(sysinfo::Pid::from_u32(child_pid)) {
                    cpu_percent += process.cpu_usage();
                    memory_bytes += process.memory();
                }
            }
            Response::Stats {
                cpu_percent,
                memory_bytes,
            }
        }
        Ok(Some(_)) => Response::Error {
            code: ErrorCode::Unknown,
            message: "request is not supported by the container runtime".into(),
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

    #[tokio::test]
    async fn ping_during_bootstrap_reports_not_ready() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let (actions, _rx) = mpsc::channel(1);
        let handler = tokio::spawn(handle_control(server, 7, 11, Utc::now(), false, actions));

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
}
