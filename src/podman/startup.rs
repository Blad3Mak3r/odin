//! Why a container start failed. The host only sees a container's startup
//! through Podman, so the evidence is collected here before anything removes it.

use std::fmt;

use super::ContainerExit;

/// Log lines quoted in the error message; the full tail stays on the struct.
const QUOTED_LOG_LINES: usize = 3;

/// A container stopped before its Odin runtime answered. Carries what Podman
/// recorded plus the end of its output, which is where `odin run` prints
/// whatever killed it.
#[derive(Debug)]
pub struct ContainerExited {
    pub container: String,
    /// `None` when the container no longer exists at all.
    pub exit: Option<ContainerExit>,
    pub log_tail: Vec<String>,
}

impl fmt::Display for ContainerExited {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(exit) = &self.exit else {
            write!(f, "container {} disappeared during startup", self.container)?;
            return self.write_output(f);
        };
        write!(f, "container {} exited during startup", self.container)?;
        match exit.exit_code {
            Some(code) => write!(f, " with code {code}")?,
            None => write!(f, " without an exit code")?,
        }
        if exit.oom_killed {
            write!(f, " after being killed for running out of memory")?;
        }
        if let Some(error) = &exit.error {
            write!(f, " (Podman: {error})")?;
        }
        self.write_output(f)
    }
}

impl ContainerExited {
    fn write_output(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let start = self.log_tail.len().saturating_sub(QUOTED_LOG_LINES);
        let lines = &self.log_tail[start..];
        if lines.is_empty() {
            return Ok(());
        }
        write!(f, "; last output: {}", lines.join(" | "))
    }
}

impl std::error::Error for ContainerExited {}

#[cfg(test)]
mod tests {
    use super::*;

    fn exited(exit: Option<ContainerExit>, log_tail: &[&str]) -> String {
        ContainerExited {
            container: "odin-rust-1".into(),
            exit,
            log_tail: log_tail.iter().map(|line| (*line).to_owned()).collect(),
        }
        .to_string()
    }

    #[test]
    fn the_message_names_the_exit_code_and_quotes_the_last_output() {
        let message = exited(
            Some(ContainerExit {
                exit_code: Some(1),
                ..Default::default()
            }),
            &[
                "a",
                "b",
                "c",
                "[ODIN] ERROR: failed to spawn game server process",
            ],
        );
        assert_eq!(
            message,
            "container odin-rust-1 exited during startup with code 1; last output: \
             b | c | [ODIN] ERROR: failed to spawn game server process"
        );
    }

    #[test]
    fn the_message_reports_oom_and_podman_errors() {
        let message = exited(
            Some(ContainerExit {
                exit_code: Some(137),
                oom_killed: true,
                error: Some("crun: cannot start".into()),
                ..Default::default()
            }),
            &[],
        );
        assert_eq!(
            message,
            "container odin-rust-1 exited during startup with code 137 after being killed \
             for running out of memory (Podman: crun: cannot start)"
        );
    }

    #[test]
    fn a_vanished_container_is_reported_as_such() {
        assert_eq!(
            exited(None, &[]),
            "container odin-rust-1 disappeared during startup"
        );
    }
}
