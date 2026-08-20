mod backup;
mod cli;
mod commands;
mod config;
mod doctor;
mod instance;
mod mods;
mod paths;
mod steamcmd;
mod tmux;

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command, ModsCommand};
use paths::Paths;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    let paths = Paths::resolve(None)?;
    let cfg = config::GlobalConfig::load(&paths)?;
    let paths = if cfg.data_dir.is_some() {
        Paths::resolve(cfg.data_dir.clone())?
    } else {
        paths
    };

    match cli.command {
        Command::Install => commands::install::run(&paths),
        Command::Start { server_name } => commands::start::run(&paths, &server_name),
        Command::Stop { server_name } => commands::stop::run(&paths, &server_name),
        Command::Console { server_name } => commands::console::run(&paths, &server_name),
        Command::Status => commands::status::run(&paths),
        Command::Restart { server_name } => commands::restart::run(&paths, &server_name),
        Command::Config {
            server_name,
            action,
        } => commands::config_cmd::run(&paths, &server_name, action),
        Command::Backup { server_name } => commands::backup::run(&paths, &server_name),
        Command::Restore {
            server_name,
            backup_id,
        } => commands::restore::run(&paths, &server_name, backup_id.as_deref()),
        Command::Logs {
            server_name,
            follow,
            lines,
        } => commands::logs::run(&paths, &server_name, follow, lines),
        Command::Exec {
            server_name,
            command,
        } => commands::exec::run(&paths, &server_name, &command),
        Command::Doctor => commands::doctor::run(&paths),
        Command::Mods { command } => match command {
            ModsCommand::Add {
                server_name,
                mod_id,
            } => commands::mods_add::run(&paths, &server_name, &mod_id),
            ModsCommand::Update { server_name } => commands::mods_update::run(&paths, &server_name),
            ModsCommand::List { server_name } => commands::mods_list::run(&paths, &server_name),
            ModsCommand::Remove {
                server_name,
                mod_id,
            } => commands::mods_remove::run(&paths, &server_name, &mod_id),
            ModsCommand::Search { query } => commands::mods_search::run(&paths, &query),
        },
    }
}
