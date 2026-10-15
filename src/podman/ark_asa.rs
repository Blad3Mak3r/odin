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
        // Saves remain per instance. Shared installs own one configuration
        // directory; ARK itself creates the INI files inside it.
        let root = Self::instance_dir(paths, instance);
        for directory in ["Saved", "Mods", "logs"] {
            std::fs::create_dir_all(root.join(directory))?;
        }
        let config_root = if shared {
            paths.shared_install_dir(Self::GAME).join("ShooterGame")
        } else {
            root.clone()
        };
        crate::game::config_documents::cleanup_empty_ark_config(&config_root)?;
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
            let shared_config = paths
                .shared_install_dir(Self::GAME)
                .join("ShooterGame/Saved/Config");
            std::fs::create_dir_all(&shared_config)?;
            mounts.push(bind(
                &shared_config,
                &Path::new(shared_install::MOUNT_DIR).join("ShooterGame/Saved/Config"),
            ));
        }
        Ok(mounts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::game_instances::{InstallMode, create_generic, set_install_mode};

    fn fixture(label: &str) -> (Paths, Db, GenericGameInstance, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("odin-ark-layout-{label}-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.join("data"),
            config_dir: dir.join("config"),
        };
        let db = Db::open(&paths).unwrap();
        let instance =
            create_generic(&paths, &db, GameId::ArkSurvivalAscended, "layout-test").unwrap();
        (paths, db, instance, dir)
    }

    #[test]
    fn shared_layout_leaves_ini_creation_to_ark_and_cleans_only_empty_files() {
        let (paths, db, instance, dir) = fixture("shared");
        set_install_mode(&db, &instance.identity.id, InstallMode::Shared).unwrap();
        let config = paths
            .shared_install_dir(GameId::ArkSurvivalAscended)
            .join("ShooterGame/Saved/Config/WindowsServer");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("GameUserSettings.ini"), "").unwrap();
        std::fs::write(config.join("Game.ini"), "[ServerSettings]\nFoo=bar\n").unwrap();

        crate::game::config_documents::cleanup_empty_ark_config(
            &paths
                .shared_install_dir(GameId::ArkSurvivalAscended)
                .join("ShooterGame"),
        )
        .unwrap();
        ArkSurvivalAscended::extra_mounts(&paths, &db, &instance).unwrap();

        assert!(!config.join("GameUserSettings.ini").exists());
        assert_eq!(
            std::fs::read_to_string(config.join("Game.ini")).unwrap(),
            "[ServerSettings]\nFoo=bar\n"
        );
        assert!(config.is_dir());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn isolated_layout_does_not_create_ark_ini_files() {
        let (paths, _db, instance, dir) = fixture("isolated");
        let config = paths
            .game_instance_dir(GameId::ArkSurvivalAscended, instance.name())
            .join("Saved/Config/WindowsServer");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("Game.ini"), "").unwrap();

        crate::game::config_documents::cleanup_empty_ark_config(
            &paths.game_instance_dir(GameId::ArkSurvivalAscended, instance.name()),
        )
        .unwrap();

        assert!(!config.join("Game.ini").exists());
        assert!(!config.join("GameUserSettings.ini").exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn isolated_instance_mounts_its_own_saved_tree_without_replacing_native_files() {
        let (paths, db, instance, dir) = fixture("isolated-persistence");
        let saved = paths
            .game_instance_dir(GameId::ArkSurvivalAscended, instance.name())
            .join("Saved");
        let ini = saved.join("Config/WindowsServer/GameUserSettings.ini");
        std::fs::create_dir_all(ini.parent().unwrap()).unwrap();
        std::fs::write(&ini, "operator config").unwrap();
        std::fs::write(saved.join("world.ark"), "operator save").unwrap();

        let mounts = ArkSurvivalAscended::extra_mounts(&paths, &db, &instance).unwrap();

        assert!(mounts.iter().any(|mount| {
            mount.destination == "/srv/odin/runtime/ark-asa/ShooterGame/Saved"
                && mount.source == saved.to_string_lossy()
        }));
        assert_eq!(std::fs::read_to_string(ini).unwrap(), "operator config");
        assert_eq!(
            std::fs::read_to_string(saved.join("world.ark")).unwrap(),
            "operator save"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn shared_instance_keeps_saved_per_instance_and_mounts_shared_config() {
        let (paths, db, instance, dir) = fixture("shared-persistence");
        set_install_mode(&db, &instance.identity.id, InstallMode::Shared).unwrap();
        let saved = paths
            .game_instance_dir(GameId::ArkSurvivalAscended, instance.name())
            .join("Saved");
        let shared_config = paths
            .shared_install_dir(GameId::ArkSurvivalAscended)
            .join("ShooterGame/Saved/Config");
        std::fs::create_dir_all(&saved).unwrap();
        std::fs::create_dir_all(&shared_config).unwrap();
        std::fs::write(saved.join("world.ark"), "instance save").unwrap();
        std::fs::write(shared_config.join("operator.ini"), "shared config").unwrap();

        let mounts = ArkSurvivalAscended::extra_mounts(&paths, &db, &instance).unwrap();

        assert!(mounts.iter().any(|mount| {
            mount.destination == "/srv/odin/shared/ShooterGame/Saved"
                && mount.source == saved.to_string_lossy()
        }));
        assert!(mounts.iter().any(|mount| {
            mount.destination == "/srv/odin/shared/ShooterGame/Saved/Config"
                && mount.source == shared_config.to_string_lossy()
        }));
        assert_eq!(
            std::fs::read_to_string(saved.join("world.ark")).unwrap(),
            "instance save"
        );
        assert_eq!(
            std::fs::read_to_string(shared_config.join("operator.ini")).unwrap(),
            "shared config"
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
