//! Linux cgroup v2 support for Odin's per-game resource controls.

use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tokio::process::Command;

use crate::db::resource_limits::ResourceLimits;

const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const MANAGER_SUBGROUP: &str = "manager";
const GAME_GROUP: &str = "games";
const CPU_PERIOD_US: u64 = 100_000;

/// An empty leaf cgroup configured for one game process and all its children.
pub struct GameCgroup {
    path: PathBuf,
    procs_path: CString,
}

impl GameCgroup {
    /// Creates or reuses this instance's empty cgroup. Returns `None` when
    /// neither resource is limited, avoiding any cgroup requirement for the
    /// existing unrestricted workflow.
    pub fn prepare(instance_id: &str, limits: &ResourceLimits) -> Result<Option<Self>> {
        if limits.is_unlimited() {
            return Ok(None);
        }
        limits.validate()?;
        let root = delegated_unit_path(Path::new(CGROUP_ROOT))?;
        let needed = controllers(limits);
        ensure_enabled(&root, &needed)?;

        let games = root.join(GAME_GROUP);
        fs::create_dir_all(&games)
            .with_context(|| format!("failed to create delegated cgroup {}", games.display()))?;
        enable_children(&games, &needed)?;

        if instance_id.is_empty()
            || !instance_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        {
            bail!("instance id is not safe for a cgroup path");
        }
        let path = games.join(instance_id);
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create game cgroup {}", path.display()))?;
        if is_populated(&path)? {
            bail!(
                "game cgroup {} is unexpectedly still populated",
                path.display()
            );
        }
        configure(&path, limits)?;
        let procs_path = CString::new(path.join("cgroup.procs").as_os_str().as_bytes())
            .expect("cgroup paths never contain NUL bytes");
        Ok(Some(Self { path, procs_path }))
    }

    /// Installs the child-side cgroup move. It uses only raw syscalls after
    /// fork, before `exec`, so a multithreaded supervisor cannot deadlock on
    /// Rust runtime state there.
    pub fn attach_to(&self, command: &mut Command) {
        let procs_path = self.procs_path.clone();
        // SAFETY: the closure performs only raw Linux syscalls and operates on
        // stack buffers plus the pre-allocated, NUL-terminated path.
        unsafe {
            command.pre_exec(move || move_current_process(&procs_path));
        }
    }

    pub fn remove(self) -> Result<()> {
        fs::remove_dir(&self.path)
            .with_context(|| format!("failed to remove game cgroup {}", self.path.display()))
    }
}

/// Verifies the installed service has delegated enough of cgroup v2 for a
/// configured limit. Called before the detached supervisor is spawned so the
/// dashboard receives a useful start error instead of an RPC timeout.
pub fn validate_environment(limits: &ResourceLimits) -> Result<()> {
    if limits.is_unlimited() {
        return Ok(());
    }
    limits.validate()?;
    let root = delegated_unit_path(Path::new(CGROUP_ROOT))?;
    ensure_enabled(&root, &controllers(limits))
}

fn controllers(limits: &ResourceLimits) -> Vec<&'static str> {
    let mut controllers = Vec::new();
    if limits.cpu_percent.is_some() {
        controllers.push("cpu");
    }
    if limits.memory_max_bytes.is_some() {
        controllers.push("memory");
    }
    controllers
}

fn delegated_unit_path(cgroup_root: &Path) -> Result<PathBuf> {
    let current_cgroup =
        fs::read_to_string("/proc/self/cgroup").context("failed to read this process's cgroup")?;
    let relative = current_cgroup
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .context("resource limits require a unified cgroup v2 hierarchy")?;
    unit_path_from_relative(cgroup_root, relative)
}

fn unit_path_from_relative(cgroup_root: &Path, relative: &str) -> Result<PathBuf> {
    let current = Path::new(relative.trim_start_matches('/'));
    if current.file_name().and_then(|name| name.to_str()) != Some(MANAGER_SUBGROUP) {
        bail!(
            "resource limits require Odin's systemd service with Delegate=cpu memory and DelegateSubgroup={MANAGER_SUBGROUP}"
        );
    }
    let unit_relative = current
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .context("resource limits could not identify Odin's delegated systemd cgroup")?;
    let unit = cgroup_root.join(unit_relative);
    if !unit.join("cgroup.controllers").is_file() {
        bail!(
            "resource limits require a writable cgroup v2 mount at {}",
            cgroup_root.display()
        );
    }
    Ok(unit)
}

fn ensure_enabled(unit: &Path, needed: &[&str]) -> Result<()> {
    let available = read_words(&unit.join("cgroup.controllers"))?;
    for controller in needed {
        if !available.iter().any(|value| value == controller) {
            bail!("resource limits require the cgroup v2 {controller} controller");
        }
    }
    let enabled = read_words(&unit.join("cgroup.subtree_control"))?;
    for controller in needed {
        if !enabled.iter().any(|value| value == controller) {
            bail!(
                "resource limits require systemd delegation of the {controller} controller; install the updated odin.service and restart Odin"
            );
        }
    }
    Ok(())
}

fn enable_children(parent: &Path, controllers: &[&str]) -> Result<()> {
    let enabled = read_words(&parent.join("cgroup.subtree_control"))?;
    let missing: Vec<_> = controllers
        .iter()
        .filter(|controller| !enabled.iter().any(|value| value == **controller))
        .map(|controller| format!("+{controller}"))
        .collect();
    if !missing.is_empty() {
        fs::write(parent.join("cgroup.subtree_control"), missing.join(" ")).with_context(|| {
            format!("failed to delegate controllers below {}", parent.display())
        })?;
    }
    Ok(())
}

fn configure(path: &Path, limits: &ResourceLimits) -> Result<()> {
    if let Some(cpu_percent) = limits.cpu_percent {
        let quota = (cpu_percent * 1000.0).round().max(1.0) as u64;
        fs::write(path.join("cpu.max"), format!("{quota} {CPU_PERIOD_US}"))
            .with_context(|| format!("failed to set CPU limit for {}", path.display()))?;
    } else if path.join("cpu.max").is_file() {
        fs::write(path.join("cpu.max"), format!("max {CPU_PERIOD_US}"))
            .with_context(|| format!("failed to clear CPU limit for {}", path.display()))?;
    }
    if let Some(memory_max) = limits.memory_max_bytes {
        let memory_high = (memory_max / 10).saturating_mul(9).max(1);
        fs::write(path.join("memory.high"), memory_high.to_string()).with_context(|| {
            format!(
                "failed to set memory pressure threshold for {}",
                path.display()
            )
        })?;
        fs::write(path.join("memory.max"), memory_max.to_string())
            .with_context(|| format!("failed to set memory limit for {}", path.display()))?;
    } else if path.join("memory.max").is_file() {
        fs::write(path.join("memory.high"), "max").with_context(|| {
            format!(
                "failed to clear memory pressure threshold for {}",
                path.display()
            )
        })?;
        fs::write(path.join("memory.max"), "max")
            .with_context(|| format!("failed to clear memory limit for {}", path.display()))?;
    }
    Ok(())
}

fn is_populated(path: &Path) -> Result<bool> {
    Ok(fs::read_to_string(path.join("cgroup.events"))?
        .lines()
        .any(|line| line.trim() == "populated 1"))
}

fn read_words(path: &Path) -> Result<Vec<String>> {
    Ok(fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?
        .split_whitespace()
        .map(str::to_owned)
        .collect())
}

fn move_current_process(procs_path: &CString) -> io::Result<()> {
    // SAFETY: all calls below are raw syscalls with stack-only buffers.
    unsafe {
        let fd = libc::open(procs_path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut digits = [0_u8; 32];
        let mut cursor = digits.len() - 1;
        digits[cursor] = b'\n';
        let mut pid = libc::getpid() as u32;
        loop {
            cursor -= 1;
            digits[cursor] = b'0' + (pid % 10) as u8;
            pid /= 10;
            if pid == 0 {
                break;
            }
        }
        let bytes = &digits[cursor..];
        let written = libc::write(fd, bytes.as_ptr().cast(), bytes.len());
        let write_error = if written == bytes.len() as isize {
            None
        } else {
            Some(io::Error::last_os_error())
        };
        libc::close(fd);
        write_error.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_quota_uses_one_hundred_millisecond_period() {
        let dir = std::env::temp_dir().join(format!("odin-cgroup-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cpu.max"), "").unwrap();
        let limits = ResourceLimits {
            cpu_percent: Some(125.5),
            memory_max_bytes: None,
        };
        configure(&dir, &limits).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("cpu.max")).unwrap(),
            "125500 100000"
        );
    }

    #[test]
    fn memory_high_is_ninety_percent_of_maximum() {
        let dir = std::env::temp_dir().join(format!("odin-cgroup-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("memory.high"), "").unwrap();
        std::fs::write(dir.join("memory.max"), "").unwrap();
        let limits = ResourceLimits {
            cpu_percent: None,
            memory_max_bytes: Some(101),
        };
        configure(&dir, &limits).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("memory.high")).unwrap(),
            "90"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("memory.max")).unwrap(),
            "101"
        );
    }

    #[test]
    fn delegated_path_uses_the_service_cgroup_above_manager() {
        let root = std::env::temp_dir().join(format!("odin-cgroup-test-{}", uuid::Uuid::new_v4()));
        let unit = root.join("system.slice/odin.service");
        std::fs::create_dir_all(&unit).unwrap();
        std::fs::write(unit.join("cgroup.controllers"), "cpu memory").unwrap();
        assert_eq!(
            unit_path_from_relative(&root, "/system.slice/odin.service/manager").unwrap(),
            unit
        );
        assert!(unit_path_from_relative(&root, "/system.slice/odin.service").is_err());
    }
}
