//! ARK: Survival Ascended's Podman lifecycle facts.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::db::Db;
use crate::db::game_instances::{self, GameInstanceIdentity, GenericGameInstance};
use crate::game::GameId;
use crate::paths::Paths;

use super::lifecycle::ContainerGame;
use super::shared_install;
use super::{Launch, Mount, bind, write_runtime_spec, write_runtime_spec_with_install_dir};

pub(super) struct ArkSurvivalAscended;

impl ContainerGame for ArkSurvivalAscended {
    const GAME: GameId = GameId::ArkSurvivalAscended;
    type Instance = GenericGameInstance;

    fn load(_paths: &Paths, db: &Db, name: &str) -> Result<GenericGameInstance> {
        game_instances::load_generic(db, Self::GAME, name)?
            .context("ARK: Survival Ascended instance does not exist")
    }

    fn identity(_db: &Db, instance: &GenericGameInstance) -> Result<GameInstanceIdentity> {
        Ok(instance.identity.clone())
    }

    fn instance_dir(paths: &Paths, instance: &GenericGameInstance) -> PathBuf {
        paths.game_instance_dir(Self::GAME, instance.name())
    }

    fn ports(instance: &GenericGameInstance) -> Result<Vec<u16>> {
        game_instances::claimed_ports(Self::GAME, &instance.config)
    }

    fn ensure_ready_to_start(paths: &Paths, db: &Db, instance: &GenericGameInstance) -> Result<()> {
        shared_install::ensure_ready(paths, db, &instance.identity)
    }

    fn prepare_layout(
        paths: &Paths,
        db: &Db,
        identity: &GameInstanceIdentity,
        instance: &GenericGameInstance,
    ) -> Result<()> {
        let shared = game_instances::is_shared_install(db, &identity.id)?;
        // Saves are per instance. A shared install instead owns its native
        // configuration so every server using it gets the same game settings.
        let root = Self::instance_dir(paths, instance);
        for directory in ["Saved", "Mods", "logs"] {
            std::fs::create_dir_all(root.join(directory))?;
        }
        if shared {
            crate::game::config_documents::initialize_shared(paths, Self::GAME)?;
        } else {
            for file in [
                "Saved/Config/WindowsServer/GameUserSettings.ini",
                "Saved/Config/WindowsServer/Game.ini",
            ] {
                let path = root.join(file);
                std::fs::create_dir_all(path.parent().context("ARK config path has no parent")?)?;
                if !path.exists() {
                    std::fs::File::create(path)?;
                }
            }
        }
        std::fs::create_dir_all(paths.container_runtime_dir(Self::GAME, &identity.id))?;
        std::fs::create_dir_all(paths.container_socket_dir(&identity.id))?;
        let launch = Launch::ArkSurvivalAscended(game_instances::ark_launch(db, instance)?);
        if shared {
            write_runtime_spec_with_install_dir(
                paths,
                identity,
                launch,
                Some(PathBuf::from(shared_install::MOUNT_DIR)),
            )?;
        } else {
            write_runtime_spec(paths, identity, launch)?;
        }
        Ok(())
    }

    fn extra_mounts(paths: &Paths, db: &Db, instance: &GenericGameInstance) -> Result<Vec<Mount>> {
        let root = Self::instance_dir(paths, instance);
        let shared = game_instances::is_shared_install(db, &instance.identity.id)?;
        let install = if shared {
            shared_install::MOUNT_DIR
        } else {
            "/srv/odin/runtime/ark-asa"
        };
        // Parent mounts must precede submounts so the per-instance state is
        // visible when the install tree itself is shared.
        let mut mounts = Vec::new();
        if shared {
            mounts.push(shared_install::mount(paths, Self::GAME));
        }
        if let Some(cluster) = game_instances::ark_cluster(db, &instance.identity.id)? {
            let host = paths.ark_cluster_dir(&cluster.cluster_id);
            std::fs::create_dir_all(&host)?;
            mounts.push(bind(
                &host,
                &PathBuf::from(format!("/srv/odin/clusters/{}", cluster.cluster_id)),
            ));
        }
        mounts.extend([
            bind(
                &root.join("Saved"),
                &PathBuf::from(format!("{install}/ShooterGame/Saved")),
            ),
            bind(
                &root.join("Mods"),
                &PathBuf::from(format!(
                    "{install}/ShooterGame/Binaries/Win64/ShooterGame/Mods"
                )),
            ),
        ]);
        if shared {
            mounts.push(bind(
                &paths
                    .shared_install_dir(Self::GAME)
                    .join("ShooterGame/Saved/Config"),
                &Path::new(shared_install::MOUNT_DIR).join("ShooterGame/Saved/Config"),
            ));
        }
        Ok(mounts)
    }
}
