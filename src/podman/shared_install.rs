//! Game-neutral shared installations: one copy of a game's files, installed
//! once by a short-lived container and mounted by every instance whose
//! install mode is `shared`. What each game keeps per instance on top of that
//! tree (saves, mods, ...) is mounted by the game's own module.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::db::Db;
use crate::db::game_instances::{self, GameInstanceIdentity};
use crate::game::{self, GameId};
use crate::paths::Paths;

use super::{
    ContainerSpec, ContainerState, CreateContainerRequest, Mount, Namespace, PodmanClient, bind,
    bind_ro,
};

/// Where a shared installation is mounted in every container that uses it.
pub const MOUNT_DIR: &str = "/srv/odin/shared";

/// The installer runs one job per game, so its container is named after it.
fn installer_container(game: GameId) -> String {
    format!("odin-{game}-install")
}

/// The shared installation as the containers see it.
pub fn mount(paths: &Paths, game: GameId) -> Mount {
    bind(&paths.shared_install_dir(game), Path::new(MOUNT_DIR))
}

pub fn is_installed(paths: &Paths, game: GameId) -> bool {
    paths
        .shared_install_dir(game)
        .join(game::driver(game).server_binary())
        .is_file()
}

/// Shown wherever a shared instance cannot start for lack of the files.
pub fn not_installed_message(game: GameId) -> String {
    format!(
        "Install the game files from Games › {} before starting a shared instance",
        game::driver(game).display_name()
    )
}

/// Shown wherever an instance's own update is refused because the files are
/// shared.
pub fn update_elsewhere_message(game: GameId) -> String {
    format!(
        "shared installs are updated from Games › {}",
        game::driver(game).display_name()
    )
}

pub fn ensure_installed(paths: &Paths, game: GameId) -> Result<()> {
    anyhow::ensure!(is_installed(paths, game), not_installed_message(game));
    Ok(())
}

/// The start-time check for a game's instances: one that runs from the shared
/// files needs them installed. Isolated instances install themselves.
pub fn ensure_ready(paths: &Paths, db: &Db, identity: &GameInstanceIdentity) -> Result<()> {
    if game_instances::is_shared_install(db, &identity.id)? {
        ensure_installed(paths, identity.game)?;
    }
    Ok(())
}

/// Installs or updates the shared files of `game`, forwarding SteamCMD's
/// output to `logger`. No instance of the game may be running from them.
pub fn install(paths: &Paths, game: GameId, mut logger: impl FnMut(&str)) -> Result<()> {
    let driver = game::driver(game);
    anyhow::ensure!(
        driver.capabilities().shared_install,
        "{} does not support a shared install",
        driver.display_name()
    );
    let config = crate::config::GlobalConfig::load(paths)?
        .podman
        .with_context(|| {
            format!(
                "Podman must be configured before installing {}",
                driver.display_name()
            )
        })?;
    let client = PodmanClient::from_config(&config)?;
    let shared = paths.shared_install_dir(game);
    let steamcmd = shared.join(".odin-steamcmd");
    // Podman refuses to bind-mount a missing host path (statfs ENOENT).
    std::fs::create_dir_all(&steamcmd)
        .with_context(|| format!("create SteamCMD home {}", steamcmd.display()))?;
    let mut labels = std::collections::BTreeMap::new();
    labels.insert("io.odin.game".into(), game.to_string());
    labels.insert("io.odin.job".into(), "install".into());
    let odin_binary = config.odin_binary()?;
    let container = installer_container(game);
    let spec = ContainerSpec {
        request: CreateContainerRequest {
            name: container.clone(),
            image: config.image(driver.runtime_image()),
            entrypoint: vec!["/usr/local/bin/odin".into()],
            command: vec!["install-shared".into(), "--game".into(), game.to_string()],
            env: std::collections::BTreeMap::from([
                ("ODIN_SHARED_INSTALL_DIR".into(), MOUNT_DIR.into()),
                (
                    "ODIN_SHARED_STEAMCMD_DIR".into(),
                    "/srv/odin/steamcmd".into(),
                ),
                (
                    "ODIN_SHARED_UPDATE_LOG".into(),
                    format!("{MOUNT_DIR}/logs/update.log"),
                ),
                ("HOME".into(), "/srv/odin/steamcmd".into()),
            ]),
            labels,
            user: format!("{}:{}", unsafe { libc::geteuid() }, unsafe {
                libc::getegid()
            }),
            mounts: vec![
                mount(paths, game),
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
    client.remove_blocking(&container, true)?;
    logger(&format!(
        "creating shared {} installer container",
        driver.display_name()
    ));
    client.create_blocking(&spec)?;
    client.start_blocking(&container)?;
    let outcome = follow_install(&client, &container, driver.display_name(), &mut logger);
    // Force-remove so a failed or abandoned run never leaves the installer
    // running; the original error, if any, is the one worth reporting.
    let removed = client.remove_blocking(&container, true);
    outcome?;
    removed?;
    logger(&format!(
        "shared {} installation completed",
        driver.display_name()
    ));
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
fn follow_install(
    client: &PodmanClient,
    container: &str,
    display_name: &str,
    mut logger: impl FnMut(&str),
) -> Result<()> {
    let (events, lines) = mpsc::channel();
    let follower = client.clone();
    let followed = container.to_owned();
    std::thread::spawn(move || {
        let sender = events.clone();
        let result = follower.follow_logs_blocking(&followed, |line| {
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
        match client.inspect_blocking(container) {
            Ok(Some(ContainerState::Exited)) => break,
            Ok(Some(_)) => failures = 0,
            Ok(None) => {
                bail!("the {display_name} installer container disappeared before it finished")
            }
            Err(error) => {
                failures += 1;
                if failures >= MAX_POLL_FAILURES {
                    return Err(error.context(format!(
                        "lost contact with Podman during the {display_name} install"
                    )));
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
    let status = client.wait_blocking(container)?;
    if status != 0 {
        let output = tail
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "shared {display_name} installation container exited with status {status}:\n{output}"
        );
    }
    Ok(())
}

/// The Steam build the shared files were last installed at.
pub fn build_id(paths: &Paths, game: GameId) -> Option<String> {
    let manifest = paths.shared_install_dir(game).join(format!(
        "steamapps/appmanifest_{}.acf",
        game::driver(game).steam_app_id()
    ));
    let raw = std::fs::read_to_string(manifest).ok()?;
    raw.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("\"buildid\"")?
            .split('"')
            .nth(1)
            .map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTAINER: &str = "odin-ark-asa-install";

    /// A Podman that always reports the installer as exited with `status`
    /// and serves `logs` as a multiplexed stdout stream.
    fn fake_installer(status: i64, logs: &[&str]) -> PodmanClient {
        use std::io::{BufRead, BufReader, Write};
        let socket =
            std::env::temp_dir().join(format!("odin-shared-{}.sock", uuid::Uuid::new_v4()));
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
    fn installer_container_is_named_after_the_game() {
        assert_eq!(installer_container(GameId::ArkSurvivalAscended), CONTAINER);
        assert_eq!(
            installer_container(GameId::SevenDaysToDie),
            "odin-7d2d-install"
        );
    }

    #[test]
    fn install_forwards_container_output_and_succeeds_on_exit_zero() {
        let client = fake_installer(0, &["Logging in", "Success! App installed"]);
        let mut lines = Vec::new();
        follow_install(&client, CONTAINER, "ARK: Survival Ascended", |line| {
            lines.push(line.to_owned())
        })
        .unwrap();
        assert_eq!(lines, ["Logging in", "Success! App installed"]);
    }

    #[test]
    fn install_failure_reports_the_exit_status_and_last_output() {
        let client = fake_installer(8, &["Logging in", "ERROR! Failed to install app"]);
        let error = follow_install(&client, CONTAINER, "ARK: Survival Ascended", |_| {})
            .unwrap_err()
            .to_string();
        assert!(error.contains("ARK: Survival Ascended"), "{error}");
        assert!(error.contains("exited with status 8"), "{error}");
        assert!(error.contains("ERROR! Failed to install app"), "{error}");
    }

    fn temp_paths(name: &str) -> Paths {
        let root =
            std::env::temp_dir().join(format!("odin-shared-{name}-{}", uuid::Uuid::new_v4()));
        Paths {
            data_dir: root.clone(),
            config_dir: root,
        }
    }

    #[test]
    fn reads_the_shared_steam_build_id_of_the_game() {
        let paths = temp_paths("build");
        for (game, app_id, build) in [
            (GameId::ArkSurvivalAscended, "2430930", "123456"),
            (GameId::SevenDaysToDie, "294420", "789"),
        ] {
            let manifest = paths
                .shared_install_dir(game)
                .join(format!("steamapps/appmanifest_{app_id}.acf"));
            std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
            std::fs::write(
                &manifest,
                format!("\"AppState\"\n{{\n\t\"buildid\"\t\"{build}\"\n}}\n"),
            )
            .unwrap();
            assert_eq!(build_id(&paths, game).as_deref(), Some(build));
        }
        assert_eq!(build_id(&paths, GameId::Palworld), None);
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn installed_means_the_games_server_binary_is_present() {
        let paths = temp_paths("installed");
        let game = GameId::ArkSurvivalAscended;
        assert!(!is_installed(&paths, game));
        let error = ensure_installed(&paths, game).unwrap_err().to_string();
        assert_eq!(
            error,
            "Install the game files from Games › ARK: Survival Ascended before starting a shared instance"
        );

        let binary = paths
            .shared_install_dir(game)
            .join(game::driver(game).server_binary());
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, b"").unwrap();

        assert!(is_installed(&paths, game));
        ensure_installed(&paths, game).unwrap();
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn refuses_to_install_a_game_without_shared_install_support() {
        let paths = temp_paths("unsupported");
        let error = install(&paths, GameId::Valheim, |_| {})
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Valheim does not support a shared install");
    }
}
