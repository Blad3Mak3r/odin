use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;

use anyhow::{Context, Result, bail};
use thiserror::Error;

const STEAMCMD_URL: &str = "https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz";
pub const VALHEIM_DEDICATED_SERVER_APP_ID: &str = "896660";

#[derive(Debug, Error)]
pub enum SteamCmdError {
    #[error("steamcmd exited with a failure status: {0}")]
    CommandFailed(String),
    #[error(
        "steamcmd ran but the expected server binary was not found at {0}; check the log for details"
    )]
    BinaryMissingAfterUpdate(String),
}

/// SteamCMD's first run on a fresh install often ends in "Missing
/// configuration" and succeeds when simply run again.
const APP_UPDATE_ATTEMPTS: u32 = 3;
const APP_UPDATE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

pub struct SteamCmd {
    steamcmd_dir: PathBuf,
}

impl SteamCmd {
    pub fn new(steamcmd_dir: PathBuf) -> Self {
        Self { steamcmd_dir }
    }

    fn script_path(&self) -> PathBuf {
        self.steamcmd_dir.join("steamcmd.sh")
    }

    pub fn is_installed(&self) -> bool {
        self.script_path().is_file()
    }

    /// Makes SteamCMD's 64-bit client library available at the fallback path
    /// used by Steamworks dedicated servers on Linux.
    ///
    /// SteamCMD can replace its copy during an update, so this is deliberately
    /// safe to call before every server start rather than being a one-time
    /// installation step.
    pub(crate) fn ensure_sdk64_client_at(&self, home_dir: &Path) -> Result<PathBuf> {
        self.ensure_sdk64_client_at_with(home_dir, |source, destination| {
            std::fs::hard_link(source, destination)
        })
    }

    fn ensure_sdk64_client_at_with(
        &self,
        home_dir: &Path,
        hard_link: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
    ) -> Result<PathBuf> {
        let source = self.steamcmd_dir.join("linux64/steamclient.so");
        if !source.is_file() {
            bail!(
                "SteamCMD's 64-bit Steam client library is missing at {}; reinstall or update the game server",
                source.display()
            );
        }
        let source = source
            .canonicalize()
            .with_context(|| format!("failed to resolve {}", source.display()))?;

        let sdk_dir = home_dir.join(".steam/sdk64");
        let destination = sdk_dir.join("steamclient.so");
        std::fs::create_dir_all(&sdk_dir).with_context(|| {
            format!("failed to create Steam SDK directory {}", sdk_dir.display())
        })?;

        let source_metadata = std::fs::metadata(&source)
            .with_context(|| format!("failed to inspect {}", source.display()))?;
        match std::fs::metadata(&destination) {
            Ok(destination_metadata)
                if source_metadata.dev() == destination_metadata.dev()
                    && source_metadata.ino() == destination_metadata.ino() =>
            {
                return Ok(destination);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect {}", destination.display()));
            }
        }
        if std::fs::symlink_metadata(&destination).is_ok_and(|metadata| metadata.is_dir()) {
            bail!(
                "cannot install Steam client library because {} is a directory",
                destination.display()
            );
        }

        let temporary = sdk_dir.join(format!(
            ".steamclient.so.{}.{}.tmp",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        match hard_link(&source, &temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
                std::os::unix::fs::symlink(&source, &temporary).with_context(|| {
                    format!(
                        "failed to link Steam client library {} -> {}",
                        temporary.display(),
                        source.display()
                    )
                })?;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to hard-link Steam client library {} -> {}",
                        temporary.display(),
                        source.display()
                    )
                });
            }
        }

        if let Err(error) = std::fs::rename(&temporary, &destination) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error).with_context(|| {
                format!(
                    "failed to activate Steam client library at {}",
                    destination.display()
                )
            });
        }
        Ok(destination)
    }

    /// Downloads and unpacks SteamCMD into `steamcmd_dir` if it isn't already there.
    pub fn ensure_installed(&self) -> Result<()> {
        if self.is_installed() {
            return Ok(());
        }

        std::fs::create_dir_all(&self.steamcmd_dir).with_context(|| {
            format!(
                "failed to create steamcmd directory {}",
                self.steamcmd_dir.display()
            )
        })?;

        tracing::info!(url = STEAMCMD_URL, "downloading SteamCMD");
        let response = reqwest::blocking::get(STEAMCMD_URL)
            .context("failed to download SteamCMD")?
            .error_for_status()
            .context("SteamCMD download returned an error status")?;
        let bytes = response
            .bytes()
            .context("failed to read SteamCMD download body")?;

        let tar = flate2::read::GzDecoder::new(bytes.as_ref());
        let mut archive = tar::Archive::new(tar);
        archive
            .unpack(&self.steamcmd_dir)
            .context("failed to unpack SteamCMD archive")?;

        if !self.is_installed() {
            bail!(
                "SteamCMD archive was unpacked but {} was not found",
                self.script_path().display()
            );
        }

        Ok(())
    }

    /// Runs `steamcmd.sh +login anonymous +force_install_dir <install_dir> +app_update <app_id> validate +quit`,
    /// calling `on_line` with each line of combined stdout/stderr as it's
    /// produced and also writing the same combined output to `log_file`.
    /// SteamCMD succeeding alone is not enough: a missing `expected_file`
    /// normally means an incompatible depot or incomplete install.
    pub fn update_app_expect_file(
        &self,
        app_id: &str,
        install_dir: &Path,
        log_file: &Path,
        expected_file: PathBuf,
        mut on_line: impl FnMut(&str),
    ) -> Result<()> {
        self.ensure_installed()?;
        std::fs::create_dir_all(install_dir)
            .with_context(|| format!("failed to create install dir {}", install_dir.display()))?;
        if let Some(parent) = log_file.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create log dir {}", parent.display()))?;
        }

        for attempt in 1..=APP_UPDATE_ATTEMPTS {
            if attempt > 1 {
                on_line("steamcmd failed; retrying (a first run often needs a second)");
                std::thread::sleep(APP_UPDATE_RETRY_DELAY);
            }
            match self.run_app_update(app_id, install_dir, log_file, attempt > 1, &mut on_line) {
                Ok(()) => break,
                Err(error) if attempt == APP_UPDATE_ATTEMPTS => return Err(error),
                Err(error) => tracing::warn!(app_id, attempt, %error, "steamcmd app_update failed"),
            }
        }

        if !expected_file.is_file() {
            bail!(SteamCmdError::BinaryMissingAfterUpdate(
                expected_file.display().to_string()
            ));
        }

        Ok(())
    }

    /// One `app_update` run, streaming its output to `on_line` and the log
    /// file (appended to when this is a retry, so the first failure stays
    /// visible).
    fn run_app_update(
        &self,
        app_id: &str,
        install_dir: &Path,
        log_file: &Path,
        append: bool,
        on_line: &mut impl FnMut(&str),
    ) -> Result<()> {
        let install_dir_str = install_dir.to_string_lossy();
        tracing::info!(app_id, install_dir = %install_dir_str, "running steamcmd app_update");

        let mut child = Command::new(self.script_path())
            .arg(format!("+force_install_dir {install_dir_str}"))
            .arg("+login anonymous")
            .arg(format!("+app_update {app_id} validate"))
            .arg("+quit")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("failed to invoke steamcmd.sh")?;

        let mut log_file_handle = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(log_file)
            .with_context(|| format!("failed to create log file {}", log_file.display()))?;

        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let stdout_tx = tx.clone();
        let stdout_thread = std::thread::spawn(move || stream_lines(stdout, &stdout_tx));
        let stderr_thread = std::thread::spawn(move || stream_lines(stderr, &tx));

        for line in rx {
            writeln!(
                log_file_handle,
                "{}",
                crate::console_log::stamp_line(chrono::Utc::now(), &line)
            )
            .ok();
            on_line(&line);
        }

        stdout_thread.join().ok();
        stderr_thread.join().ok();
        let status = child.wait().context("failed to wait on steamcmd.sh")?;

        if !status.success() {
            bail!(SteamCmdError::CommandFailed(format!(
                "exit status {status} (see log at {})",
                log_file.display()
            )));
        }
        Ok(())
    }
}

pub(crate) fn steam_home_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context(
        "HOME is not set; cannot determine Steamworks SDK directory for dedicated servers",
    )?;
    let home = PathBuf::from(home);
    anyhow::ensure!(
        home.is_absolute(),
        "HOME must be an absolute path to prepare the Steamworks SDK directory"
    );
    Ok(home)
}

/// Reads `source` line by line, forwarding each to `tx` as it arrives. Runs
/// on its own thread so stdout and stderr can be drained concurrently
/// without either pipe's OS buffer filling up and deadlocking the child.
fn stream_lines(source: impl Read, tx: &Sender<String>) {
    for line in BufReader::new(source).lines() {
        match line {
            Ok(line) => {
                if tx.send(line).is_err() {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    fn fake_steamcmd(dir: &Path, body: &str) -> SteamCmd {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).unwrap();
        let script = dir.join("steamcmd.sh");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        SteamCmd::new(dir.to_path_buf())
    }

    #[test]
    fn app_update_runs_again_after_a_failed_first_run() {
        let root = temp_dir("retry");
        let install = root.join("install");
        let marker = root.join("attempted");
        // Fails like a fresh SteamCMD ("Missing configuration"), then works.
        let steamcmd = fake_steamcmd(
            &root.join("steamcmd"),
            &format!(
                r#"if [ ! -e {marker} ]; then touch {marker}; echo "ERROR! Failed to install app (Missing configuration)"; exit 8; fi
touch {install}/server"#,
                marker = marker.display(),
                install = install.display()
            ),
        );
        let log = root.join("update.log");
        let mut lines = Vec::new();

        steamcmd
            .update_app_expect_file("294420", &install, &log, install.join("server"), |line| {
                lines.push(line.to_string())
            })
            .unwrap();

        assert!(
            lines
                .iter()
                .any(|line| line.contains("Missing configuration"))
        );
        assert!(lines.iter().any(|line| line.contains("retrying")));
        // The first failure stays in the log next to the successful run.
        assert!(
            std::fs::read_to_string(&log)
                .unwrap()
                .contains("Missing configuration")
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn app_update_gives_up_after_repeated_failures() {
        let root = temp_dir("giveup");
        let install = root.join("install");
        let steamcmd = fake_steamcmd(&root.join("steamcmd"), "exit 8");

        let error = steamcmd
            .update_app_expect_file(
                "294420",
                &install,
                &root.join("update.log"),
                install.join("server"),
                |_| {},
            )
            .unwrap_err();

        assert!(error.to_string().contains("exit status"));
        std::fs::remove_dir_all(root).ok();
    }

    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "odin-steamcmd-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }

    fn steamcmd_with_client(label: &str) -> (PathBuf, SteamCmd) {
        let dir = temp_dir(label);
        let steamcmd_dir = dir.join("steamcmd");
        std::fs::create_dir_all(steamcmd_dir.join("linux64")).unwrap();
        std::fs::write(steamcmd_dir.join("linux64/steamclient.so"), "version one").unwrap();
        (dir, SteamCmd::new(steamcmd_dir))
    }

    #[test]
    fn sdk64_client_is_hard_linked_idempotently_and_repaired_after_update() {
        let (dir, steamcmd) = steamcmd_with_client("sdk64-hardlink");
        let home = dir.join("home");
        let source = steamcmd.steamcmd_dir.join("linux64/steamclient.so");

        let destination = steamcmd.ensure_sdk64_client_at(&home).unwrap();
        let source_metadata = std::fs::metadata(&source).unwrap();
        let destination_metadata = std::fs::metadata(&destination).unwrap();
        assert_eq!(source_metadata.dev(), destination_metadata.dev());
        assert_eq!(source_metadata.ino(), destination_metadata.ino());

        let unchanged_metadata =
            std::fs::metadata(steamcmd.ensure_sdk64_client_at(&home).unwrap()).unwrap();
        assert_eq!(destination_metadata.ino(), unchanged_metadata.ino());

        let replacement = steamcmd.steamcmd_dir.join("linux64/steamclient.so.new");
        std::fs::write(&replacement, "version two").unwrap();
        std::fs::rename(replacement, &source).unwrap();
        assert_ne!(
            std::fs::metadata(&source).unwrap().ino(),
            std::fs::metadata(&destination).unwrap().ino()
        );

        steamcmd.ensure_sdk64_client_at(&home).unwrap();
        assert_eq!(
            std::fs::metadata(&source).unwrap().ino(),
            std::fs::metadata(&destination).unwrap().ino()
        );
        assert_eq!(std::fs::read_to_string(destination).unwrap(), "version two");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sdk64_client_uses_a_symlink_across_filesystems() {
        let (dir, steamcmd) = steamcmd_with_client("sdk64-symlink");
        let home = dir.join("home");
        let destination = steamcmd
            .ensure_sdk64_client_at_with(&home, |_, _| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::CrossesDevices,
                    "simulated cross-device hard link",
                ))
            })
            .unwrap();

        assert!(
            std::fs::symlink_metadata(&destination)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_link(destination).unwrap(),
            steamcmd
                .steamcmd_dir
                .join("linux64/steamclient.so")
                .canonicalize()
                .unwrap()
        );

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn sdk64_client_reports_a_missing_steamcmd_library() {
        let dir = temp_dir("sdk64-missing");
        let steamcmd = SteamCmd::new(dir.join("steamcmd"));
        let error = steamcmd
            .ensure_sdk64_client_at(&dir.join("home"))
            .unwrap_err();
        assert!(error.to_string().contains("linux64/steamclient.so"));
    }
}
