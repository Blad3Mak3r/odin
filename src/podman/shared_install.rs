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

/// A request to move an instance onto the shared files that cannot be carried
/// out as asked, as opposed to a failure while doing it.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct MigrationRefused(String);

/// What moving an instance onto the shared files did to its game files.
#[derive(Debug, serde::Serialize)]
pub struct Migration {
    /// The instance's own copy became the shared installation, because none
    /// existed yet. Nothing was downloaded.
    pub adopted_private_files: bool,
}

/// Moves a stopped, isolated instance onto the shared installation. Its saves,
/// configuration and mods already live in the instance directory and are not
/// touched. The instance's own copy of the game is kept unless
/// `delete_private_files` asks for it, or it is adopted as the shared
/// installation when there is none yet.
pub fn migrate_to_shared(
    paths: &Paths,
    db: &Db,
    identity: &GameInstanceIdentity,
    delete_private_files: bool,
) -> Result<Migration> {
    let game = identity.game;
    let driver = game::driver(game);
    if !driver.capabilities().shared_install_migration {
        bail!(MigrationRefused(format!(
            "{} instances cannot be moved to the shared install",
            driver.display_name()
        )));
    }
    let _lock = crate::instance::lifecycle::LifecycleLock::acquire(paths, game, &identity.name)?;
    if game_instances::is_shared_install(db, &identity.id)? {
        bail!(MigrationRefused(format!(
            "'{}' already uses the shared install",
            identity.name
        )));
    }
    if super::dispatch::is_running(paths, db, game, &identity.name)? {
        bail!(crate::instance::InstanceError::AlreadyRunning(
            identity.name.clone()
        ));
    }

    let runtime = paths.container_runtime_dir(game, &identity.id);
    let private = runtime.join(game.as_str());
    // The existing container keeps its old mounts when it is started again, so
    // it has to go before the instance changes mode.
    super::lifecycle::remove_container(paths, identity)?;
    let adopted = !is_installed(paths, game);
    if adopted {
        adopt_private_files(paths, game, &private)?;
    }
    game_instances::set_install_mode(db, &identity.id, game_instances::InstallMode::Shared)?;
    if delete_private_files {
        for directory in [&private, &runtime.join("steamcmd")] {
            match std::fs::remove_dir_all(directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to remove {}", directory.display()));
                }
            }
        }
    }
    super::dispatch::sync_spec(paths, db, game, &identity.name)?;
    Ok(Migration {
        adopted_private_files: adopted,
    })
}

/// Makes an instance's private game files the shared installation. Both live
/// under the data directory, so this is a rename and needs no extra space.
fn adopt_private_files(paths: &Paths, game: GameId, private: &Path) -> Result<()> {
    let driver = game::driver(game);
    if !private.join(driver.server_binary()).is_file() {
        bail!(MigrationRefused(format!(
            "Install the shared files from Games › {} first: this instance has no game files to use",
            driver.display_name()
        )));
    }
    let shared = paths.shared_install_dir(game);
    match std::fs::read_dir(&shared) {
        Ok(mut entries) => {
            if entries.next().is_some() {
                bail!(MigrationRefused(format!(
                    "The shared installation of {name} is incomplete; finish installing it from Games › {name} first",
                    name = driver.display_name()
                )));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = shared.parent().context("shared install has no parent")?;
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", shared.display()));
        }
    }
    std::fs::rename(private, &shared).with_context(|| {
        format!(
            "failed to move {} to {}",
            private.display(),
            shared.display()
        )
    })
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
    use crate::db::Db;

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

    /// Points `paths` at a Podman that has no containers and records every
    /// request line. `None` when the sandbox forbids Unix sockets.
    fn recording_podman(paths: &Paths) -> Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>> {
        use std::io::{BufRead, BufReader, Write};
        let socket = paths.data_dir.join("podman.sock");
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        let listener = match std::os::unix::net::UnixListener::bind(&socket) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(error) => panic!("failed to bind fake Podman socket: {error}"),
        };
        crate::podman::test_support::write_podman_config(paths, &socket);
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = requests.clone();
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
                let status = if line.starts_with("DELETE") {
                    "204 No Content"
                } else {
                    "404 Not Found"
                };
                recorded.lock().unwrap().push(line.trim().to_owned());
                let _ = stream.write_all(
                    format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
                        .as_bytes(),
                );
            }
        });
        Some(requests)
    }

    const GAME: GameId = GameId::SevenDaysToDie;

    fn instance_with_private_files(
        paths: &Paths,
        db: &Db,
        name: &str,
    ) -> crate::db::game_instances::GenericGameInstance {
        let instance = game_instances::create_generic(paths, db, GAME, name).unwrap();
        let runtime = paths.container_runtime_dir(GAME, &instance.identity.id);
        let private = runtime.join(GAME.as_str());
        std::fs::create_dir_all(private.join("Data")).unwrap();
        std::fs::write(private.join(game::driver(GAME).server_binary()), b"binary").unwrap();
        std::fs::write(private.join("Data/world"), b"world").unwrap();
        std::fs::create_dir_all(runtime.join("steamcmd")).unwrap();
        std::fs::write(runtime.join("steamcmd/steamcmd.sh"), b"").unwrap();
        // What the instance owns outside its game files.
        let config = paths.game_instance_dir(GAME, name).join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("serverconfig.xml"), b"operator").unwrap();
        instance
    }

    fn install_shared_binary(paths: &Paths) {
        let binary = paths
            .shared_install_dir(GAME)
            .join(game::driver(GAME).server_binary());
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(binary, b"shared").unwrap();
    }

    fn spec_of(paths: &Paths, id: &str) -> serde_json::Value {
        serde_json::from_slice(
            &std::fs::read(crate::podman::runtime_spec_path(paths, GAME, id)).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn migration_adopts_the_private_files_when_nothing_is_shared_yet() {
        let paths = crate::podman::test_support::temp_paths("migrate-adopt");
        let Some(requests) = recording_podman(&paths) else {
            return;
        };
        let db = Db::open(&paths).unwrap();
        let instance = instance_with_private_files(&paths, &db, "undead");

        let migration = migrate_to_shared(&paths, &db, &instance.identity, false).unwrap();

        assert!(migration.adopted_private_files);
        let shared = paths.shared_install_dir(GAME);
        assert_eq!(std::fs::read(shared.join("Data/world")).unwrap(), b"world");
        assert!(is_installed(&paths, GAME));
        let runtime = paths.container_runtime_dir(GAME, &instance.identity.id);
        assert!(!runtime.join("7d2d").exists());
        assert!(runtime.join("steamcmd/steamcmd.sh").exists());
        assert!(game_instances::is_shared_install(&db, &instance.identity.id).unwrap());
        let spec = spec_of(&paths, &instance.identity.id);
        assert_eq!(spec["install_dir"], "/srv/odin/shared");
        assert_eq!(spec["shared_install"], true);
        // The old container would come back with its old mounts.
        let id = &instance.identity.id;
        assert!(
            requests
                .lock()
                .unwrap()
                .iter()
                .any(|line| line.starts_with("DELETE") && line.contains(&format!("odin-7d2d-{id}")))
        );
        let config = paths
            .game_instance_dir(GAME, "undead")
            .join("config/serverconfig.xml");
        assert_eq!(std::fs::read(config).unwrap(), b"operator");
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn migration_keeps_the_private_files_unless_asked_to_delete_them() {
        let paths = crate::podman::test_support::temp_paths("migrate-delete");
        if recording_podman(&paths).is_none() {
            return;
        }
        let db = Db::open(&paths).unwrap();
        install_shared_binary(&paths);
        let kept = instance_with_private_files(&paths, &db, "kept");
        let deleted = instance_with_private_files(&paths, &db, "deleted");

        let migration = migrate_to_shared(&paths, &db, &kept.identity, false).unwrap();
        assert!(!migration.adopted_private_files);
        let kept_runtime = paths.container_runtime_dir(GAME, &kept.identity.id);
        assert!(kept_runtime.join("7d2d/Data/world").exists());
        assert!(kept_runtime.join("steamcmd/steamcmd.sh").exists());

        migrate_to_shared(&paths, &db, &deleted.identity, true).unwrap();
        let deleted_runtime = paths.container_runtime_dir(GAME, &deleted.identity.id);
        assert!(!deleted_runtime.join("7d2d").exists());
        assert!(!deleted_runtime.join("steamcmd").exists());
        // The shared files are never the ones removed.
        assert_eq!(
            std::fs::read(
                paths
                    .shared_install_dir(GAME)
                    .join(game::driver(GAME).server_binary())
            )
            .unwrap(),
            b"shared"
        );
        let config = paths
            .game_instance_dir(GAME, "deleted")
            .join("config/serverconfig.xml");
        assert_eq!(std::fs::read(config).unwrap(), b"operator");
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn migration_refusals_leave_the_instance_untouched() {
        let paths = crate::podman::test_support::temp_paths("migrate-refuse");
        let db = Db::open(&paths).unwrap();
        let ark = game_instances::create_generic(&paths, &db, GameId::ArkSurvivalAscended, "ark")
            .unwrap();
        let error = migrate_to_shared(&paths, &db, &ark.identity, false)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "ARK: Survival Ascended instances cannot be moved to the shared install"
        );

        // Nothing installed anywhere and nothing of its own to adopt.
        let empty = game_instances::create_generic(&paths, &db, GAME, "empty").unwrap();
        let error = migrate_to_shared(&paths, &db, &empty.identity, false).unwrap_err();
        assert!(
            error.downcast_ref::<MigrationRefused>().is_some(),
            "{error}"
        );
        assert!(
            error.to_string().contains("Install the shared files"),
            "{error}"
        );
        assert!(!game_instances::is_shared_install(&db, &empty.identity.id).unwrap());

        // A half-finished shared install is not overwritten by adoption.
        let instance = instance_with_private_files(&paths, &db, "undead");
        std::fs::create_dir_all(paths.shared_install_dir(GAME).join("steamapps")).unwrap();
        let error = migrate_to_shared(&paths, &db, &instance.identity, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("incomplete"), "{error}");
        let runtime = paths.container_runtime_dir(GAME, &instance.identity.id);
        assert!(runtime.join("7d2d/Data/world").exists());
        assert!(!game_instances::is_shared_install(&db, &instance.identity.id).unwrap());

        // Moving twice is a mistake, not a no-op.
        install_shared_binary(&paths);
        migrate_to_shared(&paths, &db, &instance.identity, false).unwrap();
        let error = migrate_to_shared(&paths, &db, &instance.identity, false)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "'undead' already uses the shared install");
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn a_running_instance_cannot_be_moved() {
        let paths = crate::podman::test_support::temp_paths("migrate-running");
        let db = Db::open(&paths).unwrap();
        let instance = instance_with_private_files(&paths, &db, "undead");
        if !crate::podman::test_support::podman_reporting_running(
            &paths,
            &[instance.identity.id.as_str()],
        ) {
            return;
        }
        install_shared_binary(&paths);

        let error = migrate_to_shared(&paths, &db, &instance.identity, true).unwrap_err();

        assert!(
            matches!(
                error.downcast_ref::<crate::instance::InstanceError>(),
                Some(crate::instance::InstanceError::AlreadyRunning(_))
            ),
            "{error}"
        );
        assert!(!game_instances::is_shared_install(&db, &instance.identity.id).unwrap());
        let runtime = paths.container_runtime_dir(GAME, &instance.identity.id);
        assert!(runtime.join("7d2d/Data/world").exists());
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
