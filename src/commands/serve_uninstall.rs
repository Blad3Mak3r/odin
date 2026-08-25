use std::process::Command;

use anyhow::{Context, Result};

use crate::commands::confirm;
use crate::systemd;

pub fn run(yes: bool) -> Result<()> {
    let unit_path = systemd::user_unit_path()?;
    if !unit_path.exists() {
        println!("no systemd service installed at {}", unit_path.display());
        return Ok(());
    }

    if !yes
        && !confirm(&format!(
            "remove the systemd service at {}?",
            unit_path.display()
        ))?
    {
        println!("aborted");
        return Ok(());
    }

    let status = Command::new("systemctl")
        .args(["--user", "disable", "--now", systemd::UNIT_NAME])
        .status()
        .context("failed to run `systemctl --user disable --now`")?;
    if !status.success() {
        tracing::warn!(
            %status,
            "`systemctl --user disable --now` did not exit cleanly; continuing anyway"
        );
    }

    std::fs::remove_file(&unit_path)
        .with_context(|| format!("failed to remove {}", unit_path.display()))?;

    if let Err(e) = systemd::daemon_reload() {
        tracing::warn!(error = %e, "could not reload the user systemd instance automatically");
    }

    println!("removed {}", unit_path.display());
    Ok(())
}
