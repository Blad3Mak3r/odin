//! systemd unit rendering and `systemctl` wrapper for `odin serve
//! install`/`uninstall`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

pub const UNIT_NAME: &str = "odin-serve.service";

pub fn unit_path() -> PathBuf {
    PathBuf::from("/etc/systemd/system").join(UNIT_NAME)
}

unsafe extern "C" {
    fn geteuid() -> u32;
}

pub fn is_root() -> bool {
    // SAFETY: geteuid() takes no arguments, touches no memory, and can't fail.
    unsafe { geteuid() == 0 }
}

pub fn render_unit(binary: &Path, bind: &str, port: u16, user: &str) -> String {
    format!(
        "[Unit]\n\
         Description=Odin Valheim server manager dashboard\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         User={user}\n\
         ExecStart={} serve --bind {bind} --port {port}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        binary.display()
    )
}

pub fn daemon_reload() -> Result<()> {
    let status = Command::new("systemctl")
        .arg("daemon-reload")
        .status()
        .context("failed to run `systemctl daemon-reload`")?;
    if !status.success() {
        bail!("`systemctl daemon-reload` exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_expected_unit_fields() {
        let unit = render_unit(Path::new("/usr/local/bin/odin"), "0.0.0.0", 8080, "alice");
        assert!(unit.contains("User=alice"));
        assert!(unit.contains("ExecStart=/usr/local/bin/odin serve --bind 0.0.0.0 --port 8080"));
        assert!(unit.contains("WantedBy=multi-user.target"));
    }
}
