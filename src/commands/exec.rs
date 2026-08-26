use anyhow::{Result, bail};
use tokio::io::AsyncWriteExt as _;

use crate::db::Db;
use crate::instance::{Instance, InstanceError, lifecycle, process};
use crate::paths::{self, Paths};

pub fn run(paths: &Paths, db: &Db, server_name: &str, command: &str) -> Result<()> {
    let instance = Instance::load_existing(paths, db, server_name)?;
    if !lifecycle::is_running(&instance)? {
        bail!(InstanceError::NotRunning(server_name.to_string()));
    }

    let fifo = paths::instance_console_fifo(&instance.dir);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let mut writer = process::open_console_writer(&fifo).await?;
            writer.write_all(format!("{command}\n").as_bytes()).await?;
            anyhow::Ok(())
        })?;

    println!("sent to '{server_name}': {command}");
    Ok(())
}
