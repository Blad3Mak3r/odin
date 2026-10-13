//! ARK: Survival Ascended's Podman lifecycle facts.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::db::Db;
use crate::db::game_instances::{self, GameInstanceIdentity, GenericGameInstance};
use crate::game::{GameId, RuntimeImage};
use crate::paths::Paths;

use super::lifecycle::ContainerGame;
use super::{
    ContainerSpec, ContainerState, CreateContainerRequest, Launch, Mount, Namespace, PodmanClient,
    bind, bind_ro, write_runtime_spec, write_runtime_spec_with_install_dir,
};

const SHARED_INSTALL_CONTAINER: &str = "odin-ark-asa-install";

pub fn install_shared(paths: &Paths, mut logger: impl FnMut(&str)) -> Result<()> {
    let config = crate::config::GlobalConfig::load(paths)?
        .podman
        .context("Podman must be configured before installing ARK: Survival Ascended")?;
    let client = PodmanClient::from_config(&config)?;
    let shared = paths.shared_install_dir(GameId::ArkSurvivalAscended);
    let steamcmd = shared.join(".odin-steamcmd");
    // Podman refuses to bind-mount a missing host path (statfs ENOENT).
    std::fs::create_dir_all(&steamcmd)
        .with_context(|| format!("create SteamCMD home {}", steamcmd.display()))?;
    let mut labels = std::collections::BTreeMap::new();
    labels.insert(
        "io.odin.game".into(),
        GameId::ArkSurvivalAscended.to_string(),
    );
    labels.insert("io.odin.job".into(), "install".into());
    let odin_binary = config.odin_binary()?;
    let spec = ContainerSpec {
        request: CreateContainerRequest {
            name: SHARED_INSTALL_CONTAINER.into(),
            image: config.image(RuntimeImage::Proton),
            entrypoint: vec!["/usr/local/bin/odin".into()],
            command: vec!["install-ark-shared".into()],
            env: std::collections::BTreeMap::from([
                ("ODIN_SHARED_INSTALL_DIR".into(), "/srv/odin/shared".into()),
                (
                    "ODIN_SHARED_STEAMCMD_DIR".into(),
                    "/srv/odin/steamcmd".into(),
                ),
                (
                    "ODIN_SHARED_UPDATE_LOG".into(),
                    "/srv/odin/shared/logs/update.log".into(),
                ),
                ("HOME".into(), "/srv/odin/steamcmd".into()),
            ]),
            labels,
            user: format!("{}:{}", unsafe { libc::geteuid() }, unsafe {
                libc::getegid()
            }),
            mounts: vec![
                bind(&shared, Path::new("/srv/odin/shared")),
                bind(&steamcmd, Path::new("/srv/odin/steamcmd")),
                bind_ro(&odin_binary, Path::new("/usr/local/bin/odin")),
            ],
            netns: Namespace {
                nsmode: "host".into(),
            },
            resource_limits: None,
        },
    };
    client.ensure_supported_blocking()?;
    client.ensure_image_blocking(&spec.request.image)?;
    client.remove_blocking(SHARED_INSTALL_CONTAINER, true)?;
    logger("creating shared ARK installer container");
    client.create_blocking(&spec)?;
    client.start_blocking(SHARED_INSTALL_CONTAINER)?;
    let outcome = follow_install(&client, &mut logger);
    // Force-remove so a failed or abandoned run never leaves the installer
    // running; the original error, if any, is the one worth reporting.
    let removed = client.remove_blocking(SHARED_INSTALL_CONTAINER, true);
    outcome?;
    removed?;
    logger("shared ARK installation completed");
    Ok(())
}

/// How often the installer container's state is checked.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Consecutive failed state checks tolerated before giving up on Podman.
const MAX_POLL_FAILURES: u32 = 5;
/// How long the log stream may keep delivering lines after the container exits.
const LOG_DRAIN: Duration = Duration::from_secs(5);
/// Last installer lines repeated in the error when the installation fails.
const FAILURE_TAIL_LINES: usize = 20;

enum LogEvent {
    Line(String),
    Interrupted(String),
}

/// Forwards the started installer container's output to `logger` and returns
/// once it has exited successfully.
///
/// No single request spans the install: the output comes from libpod's log
/// stream and completion from short state checks, so a Podman restart or a
/// dropped stream is reported or retried instead of hanging the job.
fn follow_install(client: &PodmanClient, mut logger: impl FnMut(&str)) -> Result<()> {
    let (events, lines) = mpsc::channel();
    let follower = client.clone();
    std::thread::spawn(move || {
        let sender = events.clone();
        let result = follower.follow_logs_blocking(SHARED_INSTALL_CONTAINER, |line| {
            sender.send(LogEvent::Line(line.to_owned())).ok();
        });
        if let Err(error) = result {
            events
                .send(LogEvent::Interrupted(format!("{error:#}")))
                .ok();
        }
    });

    let mut tail = VecDeque::with_capacity(FAILURE_TAIL_LINES);
    let mut forward = |event: LogEvent| match event {
        LogEvent::Line(line) => {
            logger(&line);
            if tail.len() == FAILURE_TAIL_LINES {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        LogEvent::Interrupted(error) => {
            logger(&format!("installer log stream interrupted: {error}"))
        }
    };

    let mut streaming = true;
    let mut failures = 0;
    let mut next_poll = Instant::now();
    loop {
        if streaming {
            match lines.recv_timeout(Duration::from_millis(500)) {
                Ok(event) => forward(event),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => streaming = false,
            }
        } else {
            std::thread::sleep(Duration::from_millis(500));
        }
        if Instant::now() < next_poll {
            continue;
        }
        next_poll = Instant::now() + POLL_INTERVAL;
        match client.inspect_blocking(SHARED_INSTALL_CONTAINER) {
            Ok(Some(ContainerState::Exited)) => break,
            Ok(Some(_)) => failures = 0,
            Ok(None) => bail!("the ARK installer container disappeared before it finished"),
            Err(error) => {
                failures += 1;
                if failures >= MAX_POLL_FAILURES {
                    return Err(error.context("lost contact with Podman during the ARK install"));
                }
            }
        }
    }

    // Collect what the stream still has buffered; it ends by itself once the
    // container is gone, but a stuck connection must not block the job.
    let deadline = Instant::now() + LOG_DRAIN;
    if streaming {
        while let Ok(event) = lines.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            forward(event);
        }
    }

    // The container has exited, so this returns immediately.
    let status = client.wait_blocking(SHARED_INSTALL_CONTAINER)?;
    if status != 0 {
        let output = tail
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        bail!("shared ARK installation container exited with status {status}:\n{output}");
    }
    Ok(())
}

pub fn shared_install_build_id(paths: &Paths) -> Option<String> {
    let manifest = paths
        .shared_install_dir(GameId::ArkSurvivalAscended)
        .join("steamapps/appmanifest_2430930.acf");
    let raw = std::fs::read_to_string(manifest).ok()?;
    raw.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("\"buildid\"")?
            .split('"')
            .nth(1)
            .map(str::to_owned)
    })
}

pub(super) struct ArkSurvivalAscended;

impl ContainerGame for ArkSurvivalAscended {
    const GAME: GameId = GameId::ArkSurvivalAscended;
    type Instance = GenericGameInstance;

    fn load(_paths: &Paths, db: &Db, name: &str) -> Result<GenericGameInstance> {
        game_instances::load_generic(db, Self::GAME, name)?
            .context("ARK: Survival Ascended instance does not exist")
    }

    fn identity(_db: &Db, instance: &GenericGameInstance) -> Result<GameInstanceIdentity> {
        Ok(instance.identity.clone())
    }

    fn instance_dir(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
        paths.game_instance_dir(Self::GAME, instance.name())
    }

    fn ports(instance: &GenericGameInstance) -> Result<Vec<u16>> {
        game_instances::claimed_ports(Self::GAME, &instance.config)
    }

    fn ensure_ready_to_start(paths: &Paths, db: &Db, instance: &GenericGameInstance) -> Result<()> {
        if game_instances::install_mode(db, &instance.identity.id)?
            == game_instances::InstallMode::Shared
        {
            let binary = paths
                .shared_install_dir(Self::GAME)
                .join(crate::game::driver(Self::GAME).server_binary());
            anyhow::ensure!(
                binary.is_file(),
                "Install the game files from Games › ARK: Survival Ascended before starting a shared instance"
            );
        }
        Ok(())
    }

    fn prepare_layout(
        paths: &Paths,
        db: &Db,
        identity: &GameInstanceIdentity,
        instance: &GenericGameInstance,
    ) -> Result<()> {
        let shared =
            game_instances::install_mode(db, &identity.id)? == game_instances::InstallMode::Shared;
        // Saves are per instance. A shared install instead owns its native
        // configuration so every server using it gets the same game settings.
        let root = Self::instance_dir(paths, instance);
        for directory in ["Saved", "Mods", "logs"] {
            std::fs::create_dir_all(root.join(directory))?;
        }
        if shared {
            crate::game::config_documents::initialize_shared(paths, Self::GAME)?;
        } else {
            for file in [
                "Saved/Config/WindowsServer/GameUserSettings.ini",
                "Saved/Config/WindowsServer/Game.ini",
            ] {
                let path = root.join(file);
                std::fs::create_dir_all(path.parent().context("ARK config path has no parent")?)?;
                if !path.exists() {
                    std::fs::File::create(path)?;
                }
            }
        }
        std::fs::create_dir_all(paths.container_runtime_dir(Self::GAME, &identity.id))?;
        std::fs::create_dir_all(paths.container_socket_dir(&identity.id))?;
        let launch = Launch::ArkSurvivalAscended(game_instances::ark_launch(db, instance)?);
        if shared {
            write_runtime_spec_with_install_dir(
                paths,
                identity,
                launch,
                Some(PathBuf::from("/srv/odin/shared")),
            )?;
        } else {
            write_runtime_spec(paths, identity, launch)?;
        }
        Ok(())
    }

    fn extra_mounts(paths: &Paths, db: &Db, instance: &GenericGameInstance) -> Result<Vec<Mount>> {
        let root = Self::instance_dir(paths, instance);
        let shared = game_instances::install_mode(db, &instance.identity.id)?
            == game_instances::InstallMode::Shared;
        let install = if shared {
            "/srv/odin/shared"
        } else {
            "/srv/odin/runtime/ark-asa"
        };
        // Parent mounts must precede submounts so the per-instance state is
        // visible when the install tree itself is shared.
        let mut mounts = Vec::new();
        if shared {
            mounts.push(bind(
                &paths.shared_install_dir(Self::GAME),
                Path::new("/srv/odin/shared"),
            ));
        }
        if let Some(cluster) = game_instances::ark_cluster(db, &instance.identity.id)? {
            let host = paths.ark_cluster_dir(&cluster.cluster_id);
            std::fs::create_dir_all(&host)?;
            mounts.push(bind(
                &host,
                &PathBuf::from(format!("/srv/odin/clusters/{}", cluster.cluster_id)),
            ));
        }
        mounts.extend([
            bind(
                &root.join("Saved"),
                &PathBuf::from(format!("{install}/ShooterGame/Saved")),
            ),
            bind(
                &root.join("Mods"),
                &PathBuf::from(format!(
                    "{install}/ShooterGame/Binaries/Win64/ShooterGame/Mods"
                )),
            ),
        ]);
        if shared {
            mounts.push(bind(
                &paths
                    .shared_install_dir(Self::GAME)
                    .join("ShooterGame/Saved/Config"),
                Path::new("/srv/odin/shared/ShooterGame/Saved/Config"),
            ));
        }
        Ok(mounts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Podman that always reports the installer as exited with `status`
    /// and serves `logs` as a multiplexed stdout stream.
    fn fake_installer(status: i64, logs: &[&str]) -> PodmanClient {
        use std::io::{BufRead, BufReader, Write};
        let socket = std::env::temp_dir().join(format!("odin-ark-{}.sock", uuid::Uuid::new_v4()));
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let mut stream = Vec::new();
        for line in logs {
            let payload = format!("{line}\n");
            stream.extend([1, 0, 0, 0]);
            stream.extend((payload.len() as u32).to_be_bytes());
            stream.extend(payload.as_bytes());
        }
        std::thread::spawn(move || {
            for connection in listener.incoming() {
                let mut connection = connection.unwrap();
                let mut request = String::new();
                let mut reader = BufReader::new(connection.try_clone().unwrap());
                reader.read_line(&mut request).unwrap();
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header == "\r\n" {
                        break;
                    }
                }
                let body = if request.contains("/logs") {
                    stream.clone()
                } else if request.contains("/wait") {
                    format!(r#"{{"StatusCode":{status}}}"#).into_bytes()
                } else {
                    br#"{"State":{"Running":false,"Status":"exited"}}"#.to_vec()
                };
                connection
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                connection.write_all(&body).unwrap();
            }
        });
        PodmanClient::new(&socket).unwrap()
    }

    #[test]
    fn install_forwards_container_output_and_succeeds_on_exit_zero() {
        let client = fake_installer(0, &["Logging in", "Success! App installed"]);
        let mut lines = Vec::new();
        follow_install(&client, |line| lines.push(line.to_owned())).unwrap();
        assert_eq!(lines, ["Logging in", "Success! App installed"]);
    }

    #[test]
    fn install_failure_reports_the_exit_status_and_last_output() {
        let client = fake_installer(8, &["Logging in", "ERROR! Failed to install app"]);
        let error = follow_install(&client, |_| {}).unwrap_err().to_string();
        assert!(error.contains("exited with status 8"), "{error}");
        assert!(error.contains("ERROR! Failed to install app"), "{error}");
    }

    #[test]
    fn reads_the_shared_steam_build_id() {
        let root = std::env::temp_dir().join(format!("odin-ark-build-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: root.clone(),
            config_dir: root,
        };
        let manifest = paths
            .shared_install_dir(GameId::ArkSurvivalAscended)
            .join("steamapps/appmanifest_2430930.acf");
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(&manifest, "\"AppState\"\n{\n\t\"buildid\"\t\"123456\"\n}\n").unwrap();

        assert_eq!(shared_install_build_id(&paths).as_deref(), Some("123456"));
    }
}
