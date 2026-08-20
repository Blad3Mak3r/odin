use clap::{Parser, Subcommand};

/// Manage the lifecycle of one or more Valheim dedicated game server instances.
#[derive(Parser, Debug)]
#[command(name = "valheim", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Install SteamCMD (if missing) and install/update the Valheim dedicated server.
    /// Refuses to run while any instance is currently running.
    Install,

    /// Create (if new) and start an instance, always detached in a tmux session.
    Start {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
    },

    /// Gracefully stop a running instance.
    Stop {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
    },

    /// Attach interactively to a running instance's console (tmux attach).
    Console {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
    },

    /// List all known instances and their derived state.
    Status,

    /// Stop (if running) and start an instance again.
    Restart {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
    },

    /// Get or set an instance's world/port/password/visibility.
    Config {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Snapshot an instance's world save to `<instance>/backups/<id>.zip`.
    Backup {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
    },

    /// List available backups, or restore one (instance must be stopped).
    Restore {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
        backup_id: Option<String>,
    },

    /// Print (and optionally follow) an instance's captured console log.
    Logs {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
        #[arg(short, long)]
        follow: bool,
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
    },

    /// Send a command to a running instance's console without attaching.
    Exec {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
        command: String,
    },

    /// Check the environment: tmux, SteamCMD, install, data dir, network.
    Doctor,

    /// Manage mods for an instance.
    Mods {
        #[command(subcommand)]
        command: ModsCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print the instance's current world/port/password/visibility.
    Get,

    /// Update one or more fields; unset flags are left unchanged.
    Set {
        #[arg(long)]
        world: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        public: Option<bool>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ModsCommand {
    /// Install a new mod into an instance (bootstraps BepInEx if needed).
    Add {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
        mod_id: String,
    },

    /// Update an instance's already-installed mods to their latest versions.
    Update {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
    },

    /// List an instance's installed mods (no network call).
    List {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
    },

    /// Uninstall a mod from an instance.
    Remove {
        #[arg(value_parser = parse_instance_name)]
        server_name: String,
        mod_id: String,
    },

    /// Search the Thunderstore package index by name/owner.
    Search { query: String },
}

/// Validates that a server name is DNS-friendly (RFC 1123 label): lowercase
/// letters, digits, and hyphens only, must start/end alphanumeric, max 63 chars.
pub fn parse_instance_name(raw: &str) -> Result<String, String> {
    validate_instance_name(raw).map(|()| raw.to_string())
}

pub fn validate_instance_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("server name must not be empty".to_string());
    }
    if name.len() > 63 {
        return Err(format!(
            "server name '{name}' is too long ({} chars, max 63)",
            name.len()
        ));
    }
    let is_dns_label = name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if !is_dns_label {
        return Err(format!(
            "server name '{name}' is not DNS-friendly: use only lowercase letters, \
             digits, and hyphens, and don't start or end with a hyphen (e.g. 'my-server')"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_names() {
        assert!(validate_instance_name("my-server").is_ok());
        assert!(validate_instance_name("server1").is_ok());
        assert!(validate_instance_name("a").is_ok());
    }

    #[test]
    fn rejects_invalid_names() {
        assert!(validate_instance_name("").is_err());
        assert!(validate_instance_name("My Server").is_err());
        assert!(validate_instance_name("-server").is_err());
        assert!(validate_instance_name("server-").is_err());
        assert!(validate_instance_name("server_1").is_err());
        assert!(validate_instance_name(&"a".repeat(64)).is_err());
    }
}
