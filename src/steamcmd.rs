use std::path::{Path, PathBuf};
use std::process::Command;

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

    /// Runs `steamcmd.sh +login anonymous +force_install_dir <install_dir> +app_update <app_id> validate +quit`.
    /// Streams SteamCMD's own output to the console and also tees it to `log_file`.
    pub fn update_app(&self, app_id: &str, install_dir: &Path, log_file: &Path) -> Result<()> {
        self.ensure_installed()?;
        std::fs::create_dir_all(install_dir)
            .with_context(|| format!("failed to create install dir {}", install_dir.display()))?;
        if let Some(parent) = log_file.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create log dir {}", parent.display()))?;
        }

        let install_dir_str = install_dir.to_string_lossy();
        tracing::info!(app_id, install_dir = %install_dir_str, "running steamcmd app_update");

        let output = Command::new(self.script_path())
            .arg(format!("+force_install_dir {install_dir_str}"))
            .arg("+login anonymous")
            .arg(format!("+app_update {app_id} validate"))
            .arg("+quit")
            .output()
            .context("failed to invoke steamcmd.sh")?;

        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::write(log_file, &combined)
            .with_context(|| format!("failed to write steamcmd log to {}", log_file.display()))?;
        print!("{combined}");

        if !output.status.success() {
            bail!(SteamCmdError::CommandFailed(format!(
                "exit status {} (see log at {})",
                output.status,
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
