use std::time::Duration;

use anyhow::Result;

use crate::db::Db;
use crate::instance::{self};
use crate::paths::Paths;
use crate::podman::valheim;

pub fn run(paths: &Paths, db: &Db) -> Result<()> {
    let instances = instance::list_all(paths, db)?;

    if instances.is_empty() {
        println!("no instances found; create one with `odin start <server-name>`");
        return Ok(());
    }

    // Best-effort: friends need the WAN address (not the LAN one) to connect,
    // so we ask a public echo service. Never fatal — falls back to "-".
    let ip = public_ip();

    println!(
        "{:<20} {:<10} {:<22} {:<20} MODS",
        "NAME", "STATUS", "ADDRESS", "WORLD"
    );
    for inst in &instances {
        let running = valheim::is_running(paths, db, inst)?;
        let status = if running { "running" } else { "stopped" };
        let address = format!("{}:{}", ip.as_deref().unwrap_or("-"), inst.state.port);
        println!(
            "{:<20} {:<10} {:<22} {:<20} {}",
            inst.state.name,
            status,
            address,
            inst.state.world_name,
            inst.state.installed_mods.len()
        );
    }

    Ok(())
}

/// Looks up this machine's public IP via a short-timeout outbound request.
/// Returns `None` (never an error) if offline or the lookup is slow/unavailable.
fn public_ip() -> Option<String> {
    let ip = crate::http::CLIENT
        .get("https://api.ipify.org")
        .timeout(Duration::from_secs(3))
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .ok()?;
    let ip = ip.trim();
    if ip.is_empty() {
        None
    } else {
        Some(ip.to_string())
    }
}
