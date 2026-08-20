use anyhow::Result;
use chrono::Utc;

use crate::instance::{self, lifecycle};
use crate::paths::Paths;

pub fn run(paths: &Paths) -> Result<()> {
    let instances = instance::list_all(paths)?;

    if instances.is_empty() {
        println!("no instances found; create one with `valheim start <server-name>`");
        return Ok(());
    }

    println!(
        "{:<20} {:<10} {:<7} {:<20} {:<10} {:<4}",
        "NAME", "STATUS", "PORT", "WORLD", "UPTIME", "MODS"
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
        println!(
            "{:<20} {:<10} {:<7} {:<20} {:<10} {:<4}",
            inst.state.name,
            status,
            inst.state.port,
            inst.state.world_name,
            uptime,
            inst.state.installed_mods.len()
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
