//! Valheim Dedicated Server's Linux launch contract.

use std::fs::OpenOptions;
use std::process::Stdio;

use anyhow::{Context, Result};
use tokio::process::Command;

use crate::instance::Instance;
use crate::paths::{self, Paths};

/// Prepends `prefix` to whatever `var` Odin inherited, matching the historic
/// server launcher without discarding operator-provided values.
fn colon_prepend(var: &str, prefix: &str) -> String {
    match std::env::var(var) {
        Ok(existing) if !existing.is_empty() => format!("{prefix}:{existing}"),
        _ => prefix.to_string(),
    }
}

/// Builds Valheim Dedicated Server's command without spawning it. Process
/// ownership, PID tracking, and signals are game-neutral in `instance::process`.
pub fn build_command(instance: &Instance, paths: &Paths) -> Result<Command> {
    let install_dir = paths.shared_install_dir();
    let console_log = paths::instance_logs_dir(&instance.dir).join("console.log");
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&console_log)
        .with_context(|| format!("failed to open {}", console_log.display()))?;
    let stderr = stdout
        .try_clone()
        .context("failed to duplicate console.log handle for stderr")?;

    let mut command = Command::new(install_dir.join("valheim_server.x86_64"));
    command
        .current_dir(&install_dir)
        .env(
            "LD_LIBRARY_PATH",
            colon_prepend(
                "LD_LIBRARY_PATH",
                &install_dir.join("linux64").to_string_lossy(),
            ),
        )
        .env("SteamAppId", "892970")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .process_group(0);

    if instance.state.bepinex_installed {
        let bepinex_dir = paths::instance_bepinex_dir(&instance.dir);
        let doorstop_libs_dir = instance.dir.join("doorstop_libs");
        command
            .env("DOORSTOP_ENABLED", "1")
            .env(
                "DOORSTOP_TARGET_ASSEMBLY",
                bepinex_dir.join("core/BepInEx.Preloader.dll"),
            )
            .env(
                "LD_LIBRARY_PATH",
                colon_prepend(
                    "LD_LIBRARY_PATH",
                    &format!(
                        "{}:{}",
                        doorstop_libs_dir.display(),
                        install_dir.join("linux64").display()
                    ),
                ),
            )
            .env(
                "LD_PRELOAD",
                colon_prepend("LD_PRELOAD", "libdoorstop_x64.so"),
            );
    }

    command
        .arg("-nographics")
        .arg("-batchmode")
        .arg("-name")
        .arg(&instance.state.name)
        .arg("-port")
        .arg(instance.state.port.to_string())
        .arg("-world")
        .arg(&instance.state.world_name)
        .arg("-savedir")
        .arg(paths::instance_saves_dir(&instance.dir))
        .arg("-public")
        .arg(if instance.state.public { "1" } else { "0" });
    if let Some(password) = &instance.state.password {
        command.arg("-password").arg(password);
    }
    if let Some(seconds) = instance.state.save_interval {
        command.arg("-saveinterval").arg(seconds.to_string());
    }
    if let Some(count) = instance.state.backups {
        command.arg("-backups").arg(count.to_string());
    }
    if let Some(seconds) = instance.state.backup_short {
        command.arg("-backupshort").arg(seconds.to_string());
    }
    if let Some(seconds) = instance.state.backup_long {
        command.arg("-backuplong").arg(seconds.to_string());
    }
    if instance.state.crossplay {
        command.arg("-crossplay");
    }
    if let Some(id) = &instance.state.playfab_instance_id {
        command.arg("-instanceid").arg(id);
    }
    if let Some(preset) = instance.state.preset {
        command.arg("-preset").arg(preset.as_arg());
    }
    let modifiers = &instance.state.modifiers;
    for (name, value) in [
        ("combat", modifiers.combat.map(|value| value.as_arg())),
        (
            "deathpenalty",
            modifiers.death_penalty.map(|value| value.as_arg()),
        ),
        ("resources", modifiers.resources.map(|value| value.as_arg())),
        ("raids", modifiers.raids.map(|value| value.as_arg())),
        ("portals", modifiers.portals.map(|value| value.as_arg())),
    ] {
        if let Some(value) = value {
            command.arg("-modifier").arg(name).arg(value);
        }
    }
    for key in &instance.state.set_keys {
        command.arg("-setkey").arg(key.as_arg());
    }

    tracing::info!(
        instance = %instance.state.name,
        port = instance.state.port,
        bepinex = instance.state.bepinex_installed,
        "spawning valheim_server.x86_64"
    );
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::state::{
        InstanceState, ValheimCombatModifier, ValheimDeathPenaltyModifier, ValheimModifiers,
        ValheimPortalModifier, ValheimPreset, ValheimRaidModifier, ValheimResourceModifier,
        ValheimSetKey,
    };

    #[test]
    fn build_command_preserves_the_valheim_launch_contract() {
        let dir =
            std::env::temp_dir().join(format!("odin-valheim-command-{}", uuid::Uuid::new_v4()));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let mut state = InstanceState::new("valheim-server", 2456);
        state.world_name = "my-world".to_string();
        state.password = Some("secret".to_string());
        state.public = false;
        let instance = Instance {
            dir: paths.instance_dir(&state.name),
            state,
        };
        std::fs::create_dir_all(paths::instance_logs_dir(&instance.dir)).unwrap();

        let command = build_command(&instance, &paths).unwrap();
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            args,
            vec![
                "-nographics",
                "-batchmode",
                "-name",
                "valheim-server",
                "-port",
                "2456",
                "-world",
                "my-world",
                "-savedir",
                instance.dir.join("saves").to_string_lossy().as_ref(),
                "-public",
                "0",
                "-password",
                "secret",
            ]
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn build_command_emits_every_configured_official_argument() {
        let dir = std::env::temp_dir().join(format!(
            "odin-valheim-full-command-{}",
            uuid::Uuid::new_v4()
        ));
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let mut state = InstanceState::new("valheim-server", 2456);
        state.save_interval = Some(900);
        state.backups = Some(5);
        state.backup_short = Some(7200);
        state.backup_long = Some(43200);
        state.crossplay = true;
        state.playfab_instance_id = Some("odin-one".into());
        state.preset = Some(ValheimPreset::Immersive);
        state.modifiers = ValheimModifiers {
            combat: Some(ValheimCombatModifier::Hard),
            death_penalty: Some(ValheimDeathPenaltyModifier::Casual),
            resources: Some(ValheimResourceModifier::More),
            raids: Some(ValheimRaidModifier::Less),
            portals: Some(ValheimPortalModifier::VeryHard),
        };
        state.set_keys = vec![ValheimSetKey::NoBuildCost, ValheimSetKey::NoMap];
        let instance = Instance {
            dir: paths.instance_dir(&state.name),
            state,
        };
        std::fs::create_dir_all(paths::instance_logs_dir(&instance.dir)).unwrap();

        let args: Vec<_> = build_command(&instance, &paths)
            .unwrap()
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        for expected in [
            ["-saveinterval", "900"],
            ["-backups", "5"],
            ["-backupshort", "7200"],
            ["-backuplong", "43200"],
            ["-instanceid", "odin-one"],
            ["-preset", "immersive"],
            ["-modifier", "combat"],
            ["-modifier", "deathpenalty"],
            ["-modifier", "resources"],
            ["-modifier", "raids"],
            ["-modifier", "portals"],
            ["-setkey", "nobuildcost"],
            ["-setkey", "nomap"],
        ] {
            assert!(args.windows(2).any(|pair| pair == expected));
        }
        assert!(args.iter().any(|argument| argument == "-crossplay"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
