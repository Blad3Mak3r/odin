//! One module per CLI subcommand; each exposes a `run` function called
//! directly from `main`'s dispatch on `cli::Command`.

pub mod backup;
pub mod completions;
pub mod config_cmd;
pub mod console;
pub mod create;
pub mod delete;
pub mod doctor;
pub mod exec;
pub mod install;
pub mod logs;
pub mod mods_add;
pub mod mods_disable;
pub mod mods_enable;
pub mod mods_list;
pub mod mods_manage;
pub mod mods_remove;
pub mod mods_search;
pub mod mods_update;
pub mod rename;
pub mod restart;
pub mod restore;
pub mod serve;
pub mod serve_install;
pub mod serve_uninstall;
pub mod start;
pub mod status;
pub mod stop;
