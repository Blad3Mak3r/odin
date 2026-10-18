//! What happened while a container started, and why it failed. The host only
//! sees a container's startup through Podman, so the evidence is collected
//! here, recorded per attempt, and kept after the container is gone.

use std::fmt;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use super::ContainerExit;
use crate::db::{Db, startup_attempts};

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
        match &self.exit {
            Some(exit) => write!(
                f,
                "container {} exited during startup {}",
                self.container,
                exit_summary(exit)
            )?,
            None => write!(f, "container {} disappeared during startup", self.container)?,
        }
        write!(f, "{}", quote_output(&self.log_tail))
    }
}

impl std::error::Error for ContainerExited {}

/// "with code 1 after being killed ... (Podman: ...)".
fn exit_summary(exit: &ContainerExit) -> String {
    let mut summary = match exit.exit_code {
        Some(code) => format!("with code {code}"),
        None => "without an exit code".to_string(),
    };
    if exit.oom_killed {
        summary.push_str(" after being killed for running out of memory");
    }
    if let Some(error) = &exit.error {
        summary.push_str(&format!(" (Podman: {error})"));
    }
    summary
}

/// "; last output: a | b | c", or nothing when there is no output.
fn quote_output(log_tail: &[String]) -> String {
    let start = log_tail.len().saturating_sub(QUOTED_LOG_LINES);
    match &log_tail[start..] {
        [] => String::new(),
        lines => format!("; last output: {}", lines.join(" | ")),
    }
}

/// How the container that was replaced by this start had ended, kept because
/// recreating the container destroys that evidence.
pub fn previous_run_note(exit: &ContainerExit, log_tail: &[String]) -> String {
    format!(
        "Previous container run ended {}{}",
        exit_summary(exit),
        quote_output(log_tail)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupPhase {
    /// Locks, port conflicts, layout and Podman reachability.
    Preflight,
    Image,
    /// Removing a stale container, then create and start.
    Container,
    /// Waiting for the Odin runtime inside the container to answer.
    Runtime,
    /// Waiting for the game itself to report ready (ARK's RCON).
    GameReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Running,
    Ok,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupEvent {
    pub at: DateTime<Utc>,
    pub phase: StartupPhase,
    pub status: EventStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupOutcome {
    Running,
    Succeeded,
    Failed,
}

impl StartupOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            _ => Self::Running,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupFailure {
    pub phase: StartupPhase,
    pub reason: String,
    pub exit_code: Option<i64>,
    pub oom_killed: bool,
    pub hint: Option<String>,
    pub log_tail: Vec<String>,
}

/// One persisted start attempt, as the API returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StartupAttemptView {
    pub id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub outcome: StartupOutcome,
    pub events: Vec<StartupEvent>,
    pub failure: Option<StartupFailure>,
}

/// The instance's recorded attempts, newest first.
pub fn load_attempts(db: &Db, instance_id: &str) -> anyhow::Result<Vec<StartupAttemptView>> {
    Ok(startup_attempts::list(db, instance_id)?
        .into_iter()
        .map(|row| StartupAttemptView {
            id: row.id,
            started_at: row.started_at,
            finished_at: row.finished_at,
            outcome: StartupOutcome::parse(&row.outcome),
            events: serde_json::from_str(&row.events).unwrap_or_default(),
            failure: row
                .failure
                .and_then(|failure| serde_json::from_str(&failure).ok()),
        })
        .collect())
}

/// Called once when Odin starts: an attempt still `running` belongs to a
/// process that no longer exists.
pub fn fail_interrupted_attempts(db: &Db) -> anyhow::Result<()> {
    let failure = StartupFailure {
        phase: StartupPhase::Preflight,
        reason: "Odin restarted while the instance was starting".into(),
        exit_code: None,
        oom_killed: false,
        hint: None,
        log_tail: Vec::new(),
    };
    startup_attempts::fail_interrupted(
        db,
        &timestamp(Utc::now()),
        &serde_json::to_string(&failure)?,
    )?;
    Ok(())
}

fn timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Records one start attempt. Recording is best effort: a database problem is
/// logged and never replaces the real start result.
pub struct StartupAttempt<'a> {
    db: &'a Db,
    id: Option<i64>,
    events: Vec<StartupEvent>,
}

impl<'a> StartupAttempt<'a> {
    pub fn begin(db: &'a Db, instance_id: &str) -> Self {
        let id = startup_attempts::insert_running(db, instance_id, &timestamp(Utc::now()))
            .inspect_err(|error| tracing::warn!(%error, "could not record startup attempt"))
            .ok();
        Self {
            db,
            id,
            events: Vec::new(),
        }
    }

    /// Starts `phase`, closing the one that was running.
    pub fn phase(&mut self, phase: StartupPhase, detail: impl Into<String>) {
        self.close_running(EventStatus::Ok);
        self.push(phase, EventStatus::Running, detail.into());
    }

    /// Adds a finished observation to the current phase's timeline.
    pub fn note(&mut self, phase: StartupPhase, detail: impl Into<String>) {
        self.push(phase, EventStatus::Ok, detail.into());
    }

    pub fn succeed(mut self) {
        self.close_running(EventStatus::Ok);
        self.finish(StartupOutcome::Succeeded, None);
    }

    /// The start was refused because the instance is already running, so
    /// there was no attempt to record.
    pub fn discard(self) {
        if let Some(id) = self.id
            && let Err(error) = startup_attempts::delete(self.db, id)
        {
            tracing::warn!(%error, "could not discard startup attempt");
        }
    }

    pub fn fail(mut self, error: &anyhow::Error) {
        let phase = self
            .events
            .iter()
            .rfind(|event| event.status == EventStatus::Running)
            .map_or(StartupPhase::Preflight, |event| event.phase);
        self.close_running(EventStatus::Failed);
        let exited = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<ContainerExited>());
        let exit = exited.and_then(|exited| exited.exit.as_ref());
        let mut failure = StartupFailure {
            phase,
            reason: format!("{error:#}"),
            exit_code: exit.and_then(|exit| exit.exit_code),
            oom_killed: exit.is_some_and(|exit| exit.oom_killed),
            hint: None,
            log_tail: exited
                .map(|exited| exited.log_tail.clone())
                .unwrap_or_default(),
        };
        failure.hint = hint_for(&failure).map(str::to_owned);
        self.finish(StartupOutcome::Failed, Some(&failure));
    }

    fn push(&mut self, phase: StartupPhase, status: EventStatus, detail: String) {
        self.events.push(StartupEvent {
            at: Utc::now(),
            phase,
            status,
            detail: Some(detail).filter(|detail| !detail.is_empty()),
        });
        self.persist_events();
    }

    fn close_running(&mut self, status: EventStatus) {
        if let Some(event) = self
            .events
            .iter_mut()
            .rfind(|event| event.status == EventStatus::Running)
        {
            event.status = status;
        }
    }

    fn persist_events(&self) {
        if let Some(id) = self.id
            && let Err(error) = serde_json::to_string(&self.events)
                .map_err(anyhow::Error::from)
                .and_then(|events| startup_attempts::update_events(self.db, id, &events))
        {
            tracing::warn!(%error, "could not save startup progress");
        }
    }

    fn finish(&self, outcome: StartupOutcome, failure: Option<&StartupFailure>) {
        let Some(id) = self.id else { return };
        let result = (|| {
            startup_attempts::finish(
                self.db,
                id,
                outcome.as_str(),
                &timestamp(Utc::now()),
                &serde_json::to_string(&self.events)?,
                failure.map(serde_json::to_string).transpose()?.as_deref(),
            )
        })();
        if let Err(error) = result {
            tracing::warn!(%error, "could not finish startup attempt");
        }
    }
}

/// A next step for the operator, from the failure's shape. Never guesses
/// beyond what the evidence supports.
fn hint_for(failure: &StartupFailure) -> Option<&'static str> {
    if failure.oom_killed {
        return Some(
            "The container was killed for running out of memory. Raise the instance's memory limit or free memory on the host.",
        );
    }
    match (failure.phase, failure.exit_code) {
        (_, Some(125)) => Some(
            "Podman could not run the container. Check the Podman error in the details and the [podman] settings.",
        ),
        (_, Some(126 | 127)) => Some(
            "The Odin binary could not be executed inside the container. Check [podman].odin_binary and that the runtime image provides its libraries.",
        ),
        (StartupPhase::Image, _) => Some(
            "The runtime image could not be pulled or found. Check the network and the image names in [podman].",
        ),
        (StartupPhase::Runtime, Some(_)) => Some(
            "The runtime exited before answering. The container's last output is shown; the instance's Logs tab has the rest.",
        ),
        (StartupPhase::Runtime, None) => Some(
            "The container is up but its Odin runtime never answered. Check Runtime health for a version mismatch between Odin and the runtime image.",
        ),
        (StartupPhase::GameReady, _) => Some(
            "The game server started but never became ready. Check the Logs tab for the game's own errors.",
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn db_with_instance() -> (Db, String, Paths) {
        let dir = std::env::temp_dir().join(format!("odin-startup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        };
        let db = Db::open(&paths).unwrap();
        let crate::game::instances::GameInstance::Rust(instance) =
            crate::game::instances::create(&paths, &db, crate::game::GameId::Rust, "rusty")
                .unwrap()
        else {
            unreachable!("created a Rust instance")
        };
        (db, instance.identity.id, paths)
    }

    #[test]
    fn a_failed_attempt_keeps_the_phase_it_died_in_and_the_container_evidence() {
        let (db, id, paths) = db_with_instance();
        let mut attempt = StartupAttempt::begin(&db, &id);
        attempt.phase(StartupPhase::Preflight, "checking");
        attempt.phase(StartupPhase::Runtime, "waiting");
        let error = anyhow::Error::new(ContainerExited {
            container: "odin-rust-1".into(),
            exit: Some(ContainerExit {
                exit_code: Some(137),
                oom_killed: true,
                ..Default::default()
            }),
            log_tail: vec!["killed".into()],
        })
        .context("container odin-rust-1 started but its Odin runtime did not respond");

        attempt.fail(&error);

        let [view] = load_attempts(&db, &id).unwrap().try_into().unwrap();
        assert_eq!(view.outcome, StartupOutcome::Failed);
        let statuses: Vec<_> = view.events.iter().map(|event| event.status).collect();
        assert_eq!(statuses, [EventStatus::Ok, EventStatus::Failed]);
        let failure = view.failure.unwrap();
        assert_eq!(failure.phase, StartupPhase::Runtime);
        assert_eq!(failure.exit_code, Some(137));
        assert!(failure.oom_killed);
        assert_eq!(failure.log_tail, ["killed"]);
        assert!(
            failure
                .reason
                .starts_with("container odin-rust-1 started but")
        );
        assert!(failure.hint.unwrap().contains("running out of memory"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn a_failure_without_container_evidence_still_names_its_phase() {
        let (db, id, paths) = db_with_instance();
        let mut attempt = StartupAttempt::begin(&db, &id);
        attempt.phase(StartupPhase::Image, "pulling");

        attempt.fail(&anyhow::anyhow!(
            "Podman API returned 404: manifest unknown"
        ));

        let [view] = load_attempts(&db, &id).unwrap().try_into().unwrap();
        let failure = view.failure.unwrap();
        assert_eq!(failure.phase, StartupPhase::Image);
        assert_eq!(failure.exit_code, None);
        assert!(failure.hint.unwrap().contains("image"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn progress_is_visible_while_the_attempt_is_running() {
        let (db, id, paths) = db_with_instance();
        let mut attempt = StartupAttempt::begin(&db, &id);
        attempt.phase(StartupPhase::Image, "pulling");

        let [view] = load_attempts(&db, &id).unwrap().try_into().unwrap();
        assert_eq!(view.outcome, StartupOutcome::Running);
        assert_eq!(view.events.len(), 1);
        assert_eq!(view.events[0].status, EventStatus::Running);
        assert!(view.finished_at.is_none());

        attempt.succeed();
        let [view] = load_attempts(&db, &id).unwrap().try_into().unwrap();
        assert_eq!(view.outcome, StartupOutcome::Succeeded);
        assert_eq!(view.events[0].status, EventStatus::Ok);
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn restarting_odin_fails_attempts_it_left_running() {
        let (db, id, paths) = db_with_instance();
        let attempt = StartupAttempt::begin(&db, &id);
        std::mem::forget(attempt);

        fail_interrupted_attempts(&db).unwrap();

        let [view] = load_attempts(&db, &id).unwrap().try_into().unwrap();
        assert_eq!(view.outcome, StartupOutcome::Failed);
        assert!(view.failure.unwrap().reason.contains("Odin restarted"));
        std::fs::remove_dir_all(paths.data_dir).ok();
    }

    #[test]
    fn hints_follow_the_evidence() {
        let failure = |phase, exit_code, oom_killed| StartupFailure {
            phase,
            reason: String::new(),
            exit_code,
            oom_killed,
            hint: None,
            log_tail: Vec::new(),
        };
        let hint = |failure| hint_for(&failure).unwrap_or_default();
        assert!(hint(failure(StartupPhase::Runtime, Some(137), true)).contains("memory"));
        assert!(hint(failure(StartupPhase::Container, Some(125), false)).contains("Podman"));
        assert!(hint(failure(StartupPhase::Container, Some(127), false)).contains("Odin binary"));
        assert!(hint(failure(StartupPhase::Image, None, false)).contains("image"));
        assert!(hint(failure(StartupPhase::Runtime, Some(1), false)).contains("exited"));
        assert!(hint(failure(StartupPhase::Runtime, None, false)).contains("never answered"));
        assert!(hint(failure(StartupPhase::GameReady, None, false)).contains("never became ready"));
        assert_eq!(
            hint_for(&failure(StartupPhase::Preflight, None, false)),
            None
        );
    }

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
