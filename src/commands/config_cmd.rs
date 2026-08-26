use anyhow::{Result, bail};

use crate::cli::ConfigAction;
use crate::db::Db;
use crate::instance::{Instance, lifecycle};
use crate::paths::Paths;

pub fn run(paths: &Paths, db: &Db, server_name: &str, action: ConfigAction) -> Result<()> {
    let mut instance = Instance::load_existing(paths, db, server_name)?;

    match action {
        ConfigAction::Get => {
            println!("name:     {}", instance.state.name);
            println!("world:    {}", instance.state.world_name);
            println!("port:     {}", instance.state.port);
            println!(
                "password: {}",
                instance.state.password.as_deref().unwrap_or("-")
            );
            println!("public:   {}", instance.state.public);
        }
        ConfigAction::Set {
            world,
            port,
            password,
            public,
        } => {
            if world.is_none() && port.is_none() && password.is_none() && public.is_none() {
                bail!("nothing to set; pass at least one of --world, --port, --password, --public");
            }
            if let Some(password) = &password
                && password.len() < 5
            {
                bail!("password must be at least 5 characters (Valheim's own minimum)");
            }

            if let Some(world) = world {
                instance.state.world_name = world;
            }
            if let Some(port) = port {
                instance.state.port = port;
            }
            if let Some(password) = password {
                instance.state.password = Some(password);
            }
            if let Some(public) = public {
                instance.state.public = public;
            }
            instance.save(db)?;

            if lifecycle::is_running(&instance)? {
                tracing::warn!(
                    instance = server_name,
                    "instance is currently running; changes take effect on next `odin restart {server_name}`"
                );
            }

            println!("updated config for '{server_name}'");
        }
    }

    Ok(())
}
