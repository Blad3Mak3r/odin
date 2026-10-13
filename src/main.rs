mod activity;
mod backup;
mod backup_storage;
mod cli;
mod commands;
mod config;
mod db;
mod doctor;
mod game;
mod http;
mod instance;
mod log_poll;
mod mods;
mod odin_update;
mod paths;
mod player_events;
mod podman;
mod save_events;
mod steamcmd;
mod supervisor;
mod web;

use std::process::ExitCode;

use anyhow::{Context, Result};
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

    // Container runtime mode deliberately does not open the host database:
    // the bind-mounted runtime spec is its complete typed launch contract.
    if let Command::Run { instance_id } = &cli.command {
        let spec_path = std::env::var_os("ODIN_RUNTIME_SPEC")
            .context("`odin run` is the container runtime and requires ODIN_RUNTIME_SPEC")?;
        return commands::run::run_container(
            &std::path::PathBuf::from(spec_path),
            instance_id.clone(),
        );
    }
    if matches!(cli.command, Command::InstallArkShared) {
        let install_dir = std::env::var_os("ODIN_SHARED_INSTALL_DIR")
            .context("shared installer requires ODIN_SHARED_INSTALL_DIR")?;
        let steamcmd_dir = std::env::var_os("ODIN_SHARED_STEAMCMD_DIR")
            .context("shared installer requires ODIN_SHARED_STEAMCMD_DIR")?;
        let log_file = std::env::var_os("ODIN_SHARED_UPDATE_LOG")
            .context("shared installer requires ODIN_SHARED_UPDATE_LOG")?;
        return commands::run::install_ark_shared(
            &std::path::PathBuf::from(install_dir),
            &std::path::PathBuf::from(steamcmd_dir),
            &std::path::PathBuf::from(log_file),
        );
    }

    let paths = Paths::resolve(None)?;
    let cfg = config::GlobalConfig::load(&paths)?;
    let paths = match cfg.data_dir {
        Some(data_dir) => Paths::resolve(Some(data_dir))?,
        None => paths,
    };

    // `serve` and `run` each open their own connection (they need an
    // `Arc<Db>`/a fresh handle from within their own async runtime, not this
    // single-threaded one), and `install`/`doctor`/`completions` don't touch
    // instance state at all — opening the database eagerly for every
    // subcommand keeps this dispatch simple rather than special-casing each.
    let db = db::Db::open(&paths)?;

    match cli.command {
        Command::Create { server_name } => commands::create::run(&paths, &db, &server_name),
        Command::Start { server_name } => commands::start::run(&paths, &db, &server_name),
        Command::Stop { server_name } => commands::stop::run(&paths, &db, &server_name),
        Command::Status => commands::status::run(&paths, &db),
        Command::Restart { server_name } => commands::restart::run(&paths, &db, &server_name),
        Command::Rename { old_name, new_name } => {
            commands::rename::run(&paths, &db, &old_name, &new_name)
        }
        Command::Delete {
            server_name,
            yes,
            keep_backups,
        } => commands::delete::run(&paths, &db, &server_name, yes, keep_backups),
        Command::Config {
            server_name,
            action,
        } => commands::config_cmd::run(&paths, &db, &server_name, action),
        Command::Backup { server_name } => commands::backup::run(&paths, &db, &server_name),
        Command::Restore {
            server_name,
            backup_id,
        } => commands::restore::run(&paths, &db, &server_name, backup_id.as_deref()),
        Command::Logs {
            server_name,
            follow,
            lines,
        } => commands::logs::run(&paths, &db, &server_name, follow, lines),
        Command::Doctor => commands::doctor::run(&paths),
        Command::Mods { command } => match command {
            ModsCommand::Add {
                server_name,
                mod_id,
            } => commands::mods_add::run(&paths, &db, &server_name, &mod_id),
            ModsCommand::Update { server_name } => {
                commands::mods_update::run(&paths, &db, &server_name)
            }
            ModsCommand::List { server_name } => {
                commands::mods_list::run(&paths, &db, &server_name)
            }
            ModsCommand::Remove {
                server_name,
                mod_id,
            } => commands::mods_remove::run(&paths, &db, &server_name, &mod_id),
            ModsCommand::Search {
                server_name,
                query,
                list,
            } => commands::mods_search::run(&paths, &db, &server_name, &query, list),
            ModsCommand::Enable {
                server_name,
                mod_id,
            } => commands::mods_enable::run(&paths, &db, &server_name, &mod_id),
            ModsCommand::Disable {
                server_name,
                mod_id,
            } => commands::mods_disable::run(&paths, &db, &server_name, &mod_id),
            ModsCommand::Manage { server_name } => {
                commands::mods_manage::run(&paths, &db, &server_name)
            }
        },
        Command::Completions { shell } => {
            commands::completions::run(shell);
            Ok(())
        }
        Command::Serve { bind, port } => commands::serve::run(&paths, &bind, port),
        Command::Run { .. } => unreachable!("handled before the host database is opened"),
        Command::InstallArkShared => unreachable!("handled before the host database is opened"),
    }
}
