use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::Utc;

use super::{Instance, InstanceError};
use crate::paths::{self, Paths};
use crate::tmux;

const STOP_TIMEOUT: Duration = Duration::from_secs(30);

pub fn is_running(instance: &Instance) -> Result<bool> {
    tmux::has_session(&instance.state.tmux_session)
}

pub fn start(paths: &Paths, name: &str) -> Result<Instance> {
    let mut instance = Instance::load_or_create(paths, name)?;

    if is_running(&instance)? {
        bail!(InstanceError::AlreadyRunning(name.to_string()));
    }

    let server_binary = paths.shared_install_dir().join("valheim_server.x86_64");
    if !server_binary.is_file() {
        bail!(
            "Valheim dedicated server is not installed (expected {}); run `valheim install` first",
            server_binary.display()
        );
    }

    check_port_available(paths, &instance)?;
    prepare_instance_layout(paths, &instance)?;
    let run_script = write_run_script(&instance)?;

    let logs_dir = paths::instance_logs_dir(&instance.dir);
    let console_log = logs_dir.join("console.log");

    // The server binary resolves `linux64/` libs and `steam_appid.txt` relative
    // to the working directory, so the tmux session's cwd must be the shared
    // install dir itself (matching Valheim's own `start_server.sh`), not the
    // per-instance directory.
    let run_cmd = format!("bash {}", run_script.display());
    tmux::new_detached_session(
        &instance.state.tmux_session,
        &paths.shared_install_dir(),
        &run_cmd,
    )
    .with_context(|| format!("failed to start instance '{name}'"))?;
    tmux::pipe_pane_to_file(&instance.state.tmux_session, &console_log)
        .with_context(|| format!("failed to attach log capture for instance '{name}'"))?;

    instance.state.last_started_at = Some(Utc::now());
    instance.save()?;

    Ok(instance)
}

pub fn stop(paths: &Paths, name: &str) -> Result<()> {
    let mut instance = Instance::load_existing(paths, name)?;

    if !is_running(&instance)? {
        bail!(InstanceError::NotRunning(name.to_string()));
    }

    let session = &instance.state.tmux_session;
    tmux::send_ctrl_c(session)?;

    let ended = tmux::wait_for_session_end(session, STOP_TIMEOUT)?;
    if !ended {
        tracing::warn!(
            instance = name,
            "graceful shutdown did not complete within {:?}; killing tmux session (possible data loss)",
            STOP_TIMEOUT
        );
        tmux::kill_session(session)?;
    }

    instance.state.last_stopped_at = Some(Utc::now());
    instance.save()?;

    Ok(())
}

/// Stops the instance if it's running, then starts it again. Requires the
/// instance to already exist (unlike `start`, which creates it on demand).
pub fn restart(paths: &Paths, name: &str) -> Result<Instance> {
    let instance = Instance::load_existing(paths, name)?;
    if is_running(&instance)? {
        stop(paths, name)?;
    }
    start(paths, name)
}

fn check_port_available(paths: &Paths, instance: &Instance) -> Result<()> {
    for other in super::list_all(paths)? {
        if other.state.name == instance.state.name {
            continue;
        }
        if other.state.port == instance.state.port && is_running(&other)? {
            bail!(
                "port {} is already in use by running instance '{}'",
                instance.state.port,
                other.state.name
            );
        }
    }
    Ok(())
}

fn prepare_instance_layout(paths: &Paths, instance: &Instance) -> Result<()> {
    std::fs::create_dir_all(&instance.dir)
        .with_context(|| format!("failed to create instance dir {}", instance.dir.display()))?;
    std::fs::create_dir_all(paths::instance_saves_dir(&instance.dir))?;
    std::fs::create_dir_all(paths::instance_logs_dir(&instance.dir))?;

    let symlink = paths::instance_server_symlink(&instance.dir);
    if !symlink.exists() {
        std::os::unix::fs::symlink(paths.shared_install_dir(), &symlink).with_context(|| {
            format!(
                "failed to symlink {} -> {}",
                symlink.display(),
                paths.shared_install_dir().display()
            )
        })?;
    }

    Ok(())
}

fn write_run_script(instance: &Instance) -> Result<PathBuf> {
    let saves_dir = paths::instance_saves_dir(&instance.dir);
    let bepinex_dir = paths::instance_bepinex_dir(&instance.dir);

    let mut script = String::from(
        "#!/usr/bin/env bash\nset -euo pipefail\n\n\
         export LD_LIBRARY_PATH=\"./linux64:${LD_LIBRARY_PATH:-}\"\n\
         export SteamAppId=892970\n\n",
    );

    if instance.state.bepinex_installed {
        script.push_str(&format!(
            "export DOORSTOP_ENABLE=TRUE\n\
             export DOORSTOP_INVOKE_DLL_PATH=\"{bepinex_dir}/core/BepInEx.Preloader.dll\"\n\
             export DOORSTOP_CORLIB_OVERRIDE_PATH=\"{bepinex_dir}/unstripped_corlib\"\n\
             export LD_LIBRARY_PATH=\"{bepinex_dir}/core:${{LD_LIBRARY_PATH:-}}\"\n\n",
            bepinex_dir = bepinex_dir.display()
        ));
    }

    let mut args = vec![
        "-nographics".to_string(),
        "-batchmode".to_string(),
        "-name".to_string(),
        shell_quote(&instance.state.name),
        "-port".to_string(),
        instance.state.port.to_string(),
        "-world".to_string(),
        shell_quote(&instance.state.world_name),
        "-savedir".to_string(),
        shell_quote(&saves_dir.to_string_lossy()),
        "-public".to_string(),
        if instance.state.public { "1" } else { "0" }.to_string(),
    ];
    if let Some(password) = &instance.state.password {
        args.push("-password".to_string());
        args.push(shell_quote(password));
    }

    script.push_str(&format!(
        "exec ./valheim_server.x86_64 {}\n",
        args.join(" ")
    ));

    let run_script = instance.dir.join("run.sh");
    let mut file = std::fs::File::create(&run_script)
        .with_context(|| format!("failed to create {}", run_script.display()))?;
    file.write_all(script.as_bytes())?;
    let mut perms = file.metadata()?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&run_script, perms)?;

    Ok(run_script)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
