use std::env;

use anyhow::{Context, Result, bail};

use crate::systemd;

pub fn run(bind: &str, port: u16, user: Option<String>, force: bool) -> Result<()> {
    if !systemd::is_root() {
        bail!("must run as root to install a systemd service, e.g. `sudo odin serve install`");
    }

    let user = match user {
        Some(user) => user,
        None => match env::var("SUDO_USER") {
            Ok(user) if !user.is_empty() && user != "root" => user,
            _ => bail!(
                "refusing to run the dashboard as root; re-run via `sudo odin serve install` \
                 as a regular user, or pass --user <name> to target another account"
            ),
        },
    };

    let binary = env::current_exe()
        .and_then(|path| path.canonicalize())
        .context("failed to resolve the path to the running `odin` binary")?;

    let unit_path = systemd::unit_path();
    if unit_path.exists() && !force {
        bail!(
            "a service is already installed at {}; use --force to overwrite, or run \
             `odin serve uninstall` first",
            unit_path.display()
        );
    }

    let unit = systemd::render_unit(&binary, bind, port, &user);
    let tmp_path = unit_path.with_extension("service.tmp");
    std::fs::write(&tmp_path, unit)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, &unit_path)
        .with_context(|| format!("failed to install {}", unit_path.display()))?;

    systemd::daemon_reload()?;

    println!("Installed systemd service at {}", unit_path.display());
    println!("Running as user '{user}', binding to {bind}:{port}.");
    println!();
    println!("Next steps:");
    println!(
        "  sudo systemctl enable --now {}   # start now and on boot",
        systemd::UNIT_NAME
    );
    println!(
        "  sudo systemctl status {}         # check it's running",
        systemd::UNIT_NAME
    );
    println!(
        "  journalctl -u {} -f              # follow logs",
        systemd::UNIT_NAME
    );
    println!(
        "  sudo systemctl disable --now {}  # stop and disable",
        systemd::UNIT_NAME
    );
    println!();
    println!(
        "Note: the dashboard has no built-in authentication; keep --bind private or put it \
         behind your own reverse proxy/SSH tunnel if exposing it remotely."
    );

    Ok(())
}
