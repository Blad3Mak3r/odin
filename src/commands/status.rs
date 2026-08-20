use std::time::Duration;

use anyhow::Result;
use chrono::Utc;

use crate::instance::{self, lifecycle};
use crate::paths::Paths;

pub fn run(paths: &Paths) -> Result<()> {
    let instances = instance::list_all(paths)?;

    if instances.is_empty() {
        println!("no instances found; create one with `odin start <server-name>`");
        return Ok(());
    }

    // Best-effort: friends need the WAN address (not the LAN one) to connect,
    // so we ask a public echo service. Never fatal — falls back to "-".
    let ip = public_ip();

    println!(
        "{:<20} {:<10} {:<22} {:<20} {:<10} {:<6} PASSWORD",
        "NAME", "STATUS", "ADDRESS", "WORLD", "UPTIME", "MODS"
    );
    for inst in &instances {
        let running = lifecycle::is_running(inst)?;
        let status = if running { "running" } else { "stopped" };
        let uptime = if running {
            inst.state
                .last_started_at
                .map(|t| format_duration(Utc::now() - t))
                .unwrap_or_else(|| "-".to_string())
        } else {
            "-".to_string()
        };
        let address = format!("{}:{}", ip.as_deref().unwrap_or("-"), inst.state.port);
        println!(
            "{:<20} {:<10} {:<22} {:<20} {:<10} {:<6} {}",
            inst.state.name,
            status,
            address,
            inst.state.world_name,
            uptime,
            inst.state.installed_mods.len(),
            inst.state.password.as_deref().unwrap_or("-")
        );
    }

    Ok(())
}

fn format_duration(duration: chrono::Duration) -> String {
    let total_secs = duration.num_seconds().max(0);
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

/// Looks up this machine's public IP via a short-timeout outbound request.
/// Returns `None` (never an error) if offline or the lookup is slow/unavailable.
fn public_ip() -> Option<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .ok()?;
    let ip = client
        .get("https://api.ipify.org")
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .ok()?;
    let ip = ip.trim();
    if ip.is_empty() { None } else { Some(ip.to_string()) }
}
