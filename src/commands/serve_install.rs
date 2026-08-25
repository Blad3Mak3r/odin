use std::env;

use anyhow::{Context, Result, bail};

use crate::systemd;

pub fn run(bind: &str, port: u16, force: bool) -> Result<()> {
    let binary = env::current_exe()
        .and_then(|path| path.canonicalize())
        .context("failed to resolve the path to the running `odin` binary")?;

    let unit_path = systemd::user_unit_path()?;
    if unit_path.exists() && !force {
        bail!(
            "a service is already installed at {}; use --force to overwrite, or run \
             `odin serve uninstall` first",
            unit_path.display()
        );
    }
    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let unit = systemd::render_unit(&binary, bind, port);
    let tmp_path = unit_path.with_extension("service.tmp");
    std::fs::write(&tmp_path, unit)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, &unit_path)
        .with_context(|| format!("failed to install {}", unit_path.display()))?;

    println!("Installed user systemd service at {}", unit_path.display());
    println!("Binding to {bind}:{port}.");
    println!();

    if let Err(e) = systemd::daemon_reload() {
        tracing::warn!(error = %e, "could not reload the user systemd instance automatically");
        println!(
            "Warning: couldn't reach your user systemd instance ({e:#}); run `systemctl --user \
             daemon-reload` yourself once you have an active session."
        );
        println!();
    }

    println!("Next steps:");
    println!(
        "  systemctl --user enable --now {}   # start now and on boot",
        systemd::UNIT_NAME
    );
    println!(
        "  systemctl --user status {}         # check it's running",
        systemd::UNIT_NAME
    );
    println!(
        "  journalctl --user -u {} -f         # follow logs",
        systemd::UNIT_NAME
    );
    println!(
        "  systemctl --user disable --now {}  # stop and disable",
        systemd::UNIT_NAME
    );
    println!();

    match systemd::enable_linger() {
        Ok(()) => println!(
            "Lingering enabled for your user: the service will keep running after you log out \
             and across reboots."
        ),
        Err(e) => {
            tracing::warn!(error = %e, "could not enable lingering automatically");
            println!(
                "Note: couldn't enable lingering automatically ({e:#}). Without it, the service \
                 stops as soon as you log out. Enable it yourself with:"
            );
            println!("  sudo loginctl enable-linger $(whoami)");
        }
    }
    println!();
    println!(
        "Note: the dashboard has no built-in authentication; keep --bind private or put it \
         behind your own reverse proxy/SSH tunnel if exposing it remotely."
    );

    Ok(())
}
