//! systemd unit rendering and `systemctl --user` wrapper for `odin serve
//! install`/`uninstall`. Installs a per-user service (`systemctl --user`),
//! not a system-wide one, so no root privileges are needed.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use directories::BaseDirs;

pub const UNIT_NAME: &str = "odin-serve.service";

pub fn user_unit_path() -> Result<PathBuf> {
    let base_dirs = BaseDirs::new().context("could not determine home directory for XDG paths")?;
    Ok(base_dirs
        .config_dir()
        .join("systemd")
        .join("user")
        .join(UNIT_NAME))
}

pub fn render_unit(binary: &Path, bind: &str, port: u16) -> String {
    format!(
        "[Unit]\n\
         Description=Odin Valheim server manager dashboard\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={} serve --bind {bind} --port {port}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        binary.display()
    )
}

pub fn daemon_reload() -> Result<()> {
    let status = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .context("failed to run `systemctl --user daemon-reload`")?;
    if !status.success() {
        bail!("`systemctl --user daemon-reload` exited with {status}");
    }
    Ok(())
}

/// Lets the user's systemd instance (and this service) keep running after
/// they log out and across reboots. Best-effort: on some systems enabling
/// it for another account requires privileges, but enabling it for your
/// own account normally doesn't.
pub fn enable_linger() -> Result<()> {
    let status = Command::new("loginctl")
        .arg("enable-linger")
        .status()
        .context("failed to run `loginctl enable-linger`")?;
    if !status.success() {
        bail!("`loginctl enable-linger` exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_expected_unit_fields() {
        let unit = render_unit(Path::new("/usr/local/bin/odin"), "0.0.0.0", 8080);
        assert!(unit.contains("ExecStart=/usr/local/bin/odin serve --bind 0.0.0.0 --port 8080"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(!unit.contains("User="));
    }
}
