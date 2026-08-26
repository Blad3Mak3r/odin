mod activity;
mod backup;
mod cli;
mod commands;
mod config;
mod db;
mod doctor;
mod instance;
mod mods;
mod paths;
mod steamcmd;
mod systemd;
mod tmux;
mod web;

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command, ModsCommand, ServeCommand};
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
    let paths = match cfg.data_dir {
        Some(data_dir) => Paths::resolve(Some(data_dir))?,
        None => paths,
    };

    match cli.command {
        Command::Install => commands::install::run(&paths),
        Command::Create { server_name } => commands::create::run(&paths, &server_name),
        Command::Start { server_name } => commands::start::run(&paths, &server_name),
        Command::Stop { server_name } => commands::stop::run(&paths, &server_name),
        Command::Console { server_name } => commands::console::run(&paths, &server_name),
        Command::Status => commands::status::run(&paths),
        Command::Restart { server_name } => commands::restart::run(&paths, &server_name),
        Command::Rename { old_name, new_name } => {
            commands::rename::run(&paths, &old_name, &new_name)
        }
        Command::Delete {
            server_name,
            yes,
            keep_backups,
        } => commands::delete::run(&paths, &server_name, yes, keep_backups),
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
            ModsCommand::Search {
                server_name,
                query,
                list,
            } => commands::mods_search::run(&paths, &server_name, &query, list),
            ModsCommand::Enable {
                server_name,
                mod_id,
            } => commands::mods_enable::run(&paths, &server_name, &mod_id),
            ModsCommand::Disable {
                server_name,
                mod_id,
            } => commands::mods_disable::run(&paths, &server_name, &mod_id),
            ModsCommand::Manage { server_name } => commands::mods_manage::run(&paths, &server_name),
        },
        Command::Completions { shell } => {
            commands::completions::run(shell);
            Ok(())
        }
        Command::Serve { bind, port, action } => match action {
            None => commands::serve::run(&paths, &bind, port),
            Some(ServeCommand::Install { bind, port, force }) => {
                commands::serve_install::run(&bind, port, force)
            }
            Some(ServeCommand::Uninstall { yes }) => commands::serve_uninstall::run(yes),
        },
    }
}
