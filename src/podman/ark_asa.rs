//! ARK: Survival Ascended's Podman lifecycle facts.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::db::Db;
use crate::db::game_instances::{self, GameInstanceIdentity, GenericGameInstance};
use crate::game::{GameId, RuntimeImage};
use crate::paths::Paths;

use super::lifecycle::ContainerGame;
use super::{
    ContainerSpec, CreateContainerRequest, Launch, Mount, Namespace, PodmanClient, bind, bind_ro,
    write_runtime_spec, write_runtime_spec_with_install_dir,
};

const SHARED_INSTALL_CONTAINER: &str = "odin-ark-asa-install";

pub fn install_shared(paths: &Paths, mut logger: impl FnMut(&str)) -> Result<()> {
    let config = crate::config::GlobalConfig::load(paths)?
        .podman
        .context("Podman must be configured before installing ARK: Survival Ascended")?;
    let client = PodmanClient::from_config(&config)?;
    let shared = paths.shared_install_dir(GameId::ArkSurvivalAscended);
    let steamcmd = shared.join(".odin-steamcmd");
    std::fs::create_dir_all(&shared)?;
    let mut labels = std::collections::BTreeMap::new();
    labels.insert(
        "io.odin.game".into(),
        GameId::ArkSurvivalAscended.to_string(),
    );
    labels.insert("io.odin.job".into(), "install".into());
    let spec = ContainerSpec {
        request: CreateContainerRequest {
            name: SHARED_INSTALL_CONTAINER.into(),
            image: config.image(RuntimeImage::Proton),
            entrypoint: vec!["/usr/local/bin/odin".into()],
            command: vec!["install-ark-shared".into()],
            env: std::collections::BTreeMap::from([
                ("ODIN_SHARED_INSTALL_DIR".into(), "/srv/odin/shared".into()),
                (
                    "ODIN_SHARED_STEAMCMD_DIR".into(),
                    "/srv/odin/steamcmd".into(),
                ),
                (
                    "ODIN_SHARED_UPDATE_LOG".into(),
                    "/srv/odin/shared/logs/update.log".into(),
                ),
                ("HOME".into(), "/srv/odin/steamcmd".into()),
            ]),
            labels,
            user: format!("{}:{}", unsafe { libc::geteuid() }, unsafe {
                libc::getegid()
            }),
            mounts: vec![
                bind(&shared, Path::new("/srv/odin/shared")),
                bind(&steamcmd, Path::new("/srv/odin/steamcmd")),
                bind_ro(&config.odin_binary(), Path::new("/usr/local/bin/odin")),
            ],
            netns: Namespace {
                nsmode: "host".into(),
            },
            resource_limits: None,
        },
    };
    client.ensure_supported_blocking()?;
    client.ensure_image_blocking(&spec.request.image)?;
    client.remove_blocking(SHARED_INSTALL_CONTAINER, true)?;
    logger("creating shared ARK installer container");
    client.create_blocking(&spec)?;
    client.start_blocking(SHARED_INSTALL_CONTAINER)?;
    let status = client.wait_blocking(SHARED_INSTALL_CONTAINER)?;
    client.remove_blocking(SHARED_INSTALL_CONTAINER, false)?;
    if status != 0 {
        bail!("shared ARK installation container exited with status {status}");
    }
    logger("shared ARK installation completed");
    Ok(())
}

pub fn shared_install_build_id(paths: &Paths) -> Option<String> {
    let manifest = paths
        .shared_install_dir(GameId::ArkSurvivalAscended)
        .join("steamapps/appmanifest_2430930.acf");
    let raw = std::fs::read_to_string(manifest).ok()?;
    raw.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("\"buildid\"")?
            .split('"')
            .nth(1)
            .map(str::to_owned)
    })
}

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
        if game_instances::install_mode(db, &instance.identity.id)?
            == game_instances::InstallMode::Shared
        {
            let binary = paths
                .shared_install_dir(Self::GAME)
                .join(crate::game::driver(Self::GAME).server_binary());
            anyhow::ensure!(
                binary.is_file(),
                "Install the game files from Games › ARK: Survival Ascended before starting a shared instance"
            );
        }
        Ok(())
    }

    fn prepare_layout(
        paths: &Paths,
        db: &Db,
        identity: &GameInstanceIdentity,
        instance: &GenericGameInstance,
    ) -> Result<()> {
        let shared =
            game_instances::install_mode(db, &identity.id)? == game_instances::InstallMode::Shared;
        // Save data and native configuration always belong to one instance.
        // In shared mode only the installed game files move to the common root.
        let root = Self::instance_dir(paths, instance);
        for directory in ["Saved", "Mods", "logs"] {
            std::fs::create_dir_all(root.join(directory))?;
        }
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
        std::fs::create_dir_all(paths.container_runtime_dir(Self::GAME, &identity.id))?;
        std::fs::create_dir_all(paths.container_socket_dir(&identity.id))?;
        let launch = Launch::ArkSurvivalAscended(game_instances::ark_launch(db, instance)?);
        if shared {
            write_runtime_spec_with_install_dir(
                paths,
                identity,
                launch,
                Some(PathBuf::from("/srv/odin/shared")),
            )?;
        } else {
            write_runtime_spec(paths, identity, launch)?;
        }
        Ok(())
    }

    fn extra_mounts(paths: &Paths, db: &Db, instance: &GenericGameInstance) -> Result<Vec<Mount>> {
        let root = Self::instance_dir(paths, instance);
        let shared = game_instances::install_mode(db, &instance.identity.id)?
            == game_instances::InstallMode::Shared;
        let install = if shared {
            "/srv/odin/shared"
        } else {
            "/srv/odin/runtime/ark-asa"
        };
        // Parent mounts must precede submounts so the per-instance state is
        // visible when the install tree itself is shared.
        let mut mounts = Vec::new();
        if shared {
            mounts.push(bind(
                &paths.shared_install_dir(Self::GAME),
                Path::new("/srv/odin/shared"),
            ));
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
        Ok(mounts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_shared_steam_build_id() {
        let root = std::env::temp_dir().join(format!("odin-ark-build-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: root.clone(),
            config_dir: root,
        };
        let manifest = paths
            .shared_install_dir(GameId::ArkSurvivalAscended)
            .join("steamapps/appmanifest_2430930.acf");
        std::fs::create_dir_all(manifest.parent().unwrap()).unwrap();
        std::fs::write(&manifest, "\"AppState\"\n{\n\t\"buildid\"\t\"123456\"\n}\n").unwrap();

        assert_eq!(shared_install_build_id(&paths).as_deref(), Some("123456"));
    }
}
