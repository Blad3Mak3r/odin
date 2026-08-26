use anyhow::{Result, bail};

pub fn run(_paths: &crate::paths::Paths, _db: &crate::db::Db, server_name: &str) -> Result<()> {
    bail!(
        "'odin console' no longer attaches a terminal (tmux is gone) — use \
         `odin logs --follow {server_name}` to watch the console and `odin exec {server_name} \
         <command>` to send commands, or the web dashboard for both at once"
    )
}
