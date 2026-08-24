use std::io::{BufRead as _, BufReader, Read, Write as _};
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
    /// produced (so a caller can stream progress instead of waiting for the
    /// whole, possibly multi-minute, run to finish) and also writing the
    /// same combined output to `log_file`.
    pub fn update_app(
        &self,
        app_id: &str,
        install_dir: &Path,
        log_file: &Path,
        mut on_line: impl FnMut(&str),
    ) -> Result<()> {
        self.ensure_installed()?;
        std::fs::create_dir_all(install_dir)
            .with_context(|| format!("failed to create install dir {}", install_dir.display()))?;
        if let Some(parent) = log_file.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create log dir {}", parent.display()))?;
        }

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

        let mut log_file_handle = std::fs::File::create(log_file)
            .with_context(|| format!("failed to create log file {}", log_file.display()))?;

        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let stdout_tx = tx.clone();
        let stdout_thread = std::thread::spawn(move || stream_lines(stdout, &stdout_tx));
        let stderr_thread = std::thread::spawn(move || stream_lines(stderr, &tx));

        for line in rx {
            writeln!(log_file_handle, "{line}").ok();
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

        let server_binary = install_dir.join("valheim_server.x86_64");
        if !server_binary.is_file() {
            bail!(SteamCmdError::BinaryMissingAfterUpdate(
                server_binary.display().to_string()
            ));
        }

        Ok(())
    }
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
