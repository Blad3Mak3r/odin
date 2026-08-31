//! Registry for long-running background operations (SteamCMD
//! installs/updates, mod installs) that must not block the HTTP request that
//! kicked them off. A job is spawned onto the blocking thread pool; its
//! status and a capped log buffer are kept in memory and broadcast to any
//! WebSocket subscribers watching it, and persisted to the `jobs` table so
//! history (in particular, whether a job actually succeeded) survives an
//! `odin serve` restart.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::db::Db;
use crate::db::jobs as db_jobs;

pub type JobId = String;

const MAX_LOG_LINES: usize = 2000;
const BROADCAST_CAPACITY: usize = 256;
// How many past jobs to load back into memory on startup. Older history is
// still queryable directly from the database if it's ever needed; this just
// bounds what a restart eagerly reloads.
const MAX_LOADED_JOBS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobKindDescr {
    SteamcmdInstall,
    ModAdd { instance: String, mod_id: String },
    ModUpdate { instance: String },
    ModUpload { instance: String, name: String },
    BackupCreate { instance: String },
    BackupRestore { instance: String, backup_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed { message: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct JobSnapshot {
    pub id: JobId,
    pub kind: JobKindDescr,
    pub status: JobStatus,
    pub started_at: DateTime<Utc>,
    pub log: Vec<String>,
}

/// A `JobSnapshot` without its `log`, for list views that don't need it —
/// avoids cloning every job's full (up to `MAX_LOG_LINES`) log buffer just
/// to discard it.
#[derive(Debug, Clone, Serialize)]
pub struct JobSummary {
    pub id: JobId,
    pub kind: JobKindDescr,
    pub status: JobStatus,
    pub started_at: DateTime<Utc>,
}

/// A message broadcast to live subscribers of a job as it runs.
#[derive(Debug, Clone)]
pub enum JobEvent {
    Line(String),
    Status(JobStatus),
}

struct JobRecord {
    kind: JobKindDescr,
    status: JobStatus,
    started_at: DateTime<Utc>,
    log: Vec<String>,
    sender: broadcast::Sender<JobEvent>,
}

#[derive(Clone)]
pub struct JobRegistry {
    jobs: Arc<Mutex<HashMap<JobId, JobRecord>>>,
    db: Arc<Db>,
}

/// Handed to a running job's closure so it can report progress lines.
#[derive(Clone)]
pub struct JobLogger {
    id: JobId,
    registry: JobRegistry,
}

impl JobLogger {
    pub fn line(&self, line: impl Into<String>) {
        self.registry.push_line(&self.id, line.into());
    }
}

impl JobRegistry {
    /// Loads recent job history from the database into memory (any job left
    /// `queued`/`running` at last shutdown is reconciled to `failed`, since
    /// nothing could have finished it) and prepares to record new jobs
    /// there. Never fails: a query error just starts with empty history,
    /// since job history is a convenience, not state anything else depends
    /// on.
    pub fn load(db: Arc<Db>) -> Self {
        let rows = db_jobs::recent(&db, MAX_LOADED_JOBS).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "failed to load job history");
            Vec::new()
        });

        let mut jobs = HashMap::with_capacity(rows.len());
        for row in rows {
            let kind = match decode_kind(&row.kind_payload) {
                Ok(kind) => kind,
                Err(e) => {
                    tracing::warn!(id = %row.id, error = %e, "skipping unreadable job history row");
                    continue;
                }
            };
            let mut status = decode_status(&row.status_payload).unwrap_or(JobStatus::Failed {
                message: "unreadable status".to_string(),
            });
            if matches!(status, JobStatus::Queued | JobStatus::Running) {
                status = JobStatus::Failed {
                    message: "Interrupted by an odin restart".to_string(),
                };
                if let Ok((tag, payload)) = encode_status(&status)
                    && let Err(e) = db_jobs::update_status(&db, &row.id, &tag, &payload)
                {
                    tracing::warn!(id = %row.id, error = %e, "failed to reconcile interrupted job");
                }
            }

            let (sender, _receiver) = broadcast::channel(BROADCAST_CAPACITY);
            jobs.insert(
                row.id,
                JobRecord {
                    kind,
                    status,
                    started_at: row.started_at,
                    log: row.log,
                    sender,
                },
            );
        }

        Self {
            jobs: Arc::new(Mutex::new(jobs)),
            db,
        }
    }

    /// Spawns `work` onto the blocking thread pool and returns immediately
    /// with the new job's id; `work` reports progress via the `JobLogger` it
    /// receives, and its `Result` determines the job's final status.
    pub fn spawn<F>(&self, kind: JobKindDescr, work: F) -> JobId
    where
        F: FnOnce(&JobLogger) -> anyhow::Result<()> + Send + 'static,
    {
        let id = Uuid::new_v4().to_string();
        let started_at = Utc::now();
        let (sender, _receiver) = broadcast::channel(BROADCAST_CAPACITY);
        {
            let mut jobs = self.jobs.lock().expect("jobs registry lock poisoned");
            jobs.insert(
                id.clone(),
                JobRecord {
                    kind: kind.clone(),
                    status: JobStatus::Queued,
                    started_at,
                    log: Vec::new(),
                    sender,
                },
            );
        }
        if let (Ok((kind_tag, kind_payload)), Ok((status_tag, status_payload))) =
            (encode_kind(&kind), encode_status(&JobStatus::Queued))
            && let Err(e) = db_jobs::insert(
                &self.db,
                &id,
                &kind_tag,
                &kind_payload,
                &status_tag,
                &status_payload,
                started_at,
            )
        {
            tracing::warn!(id = %id, error = %e, "failed to persist new job");
        }

        let registry = self.clone();
        let job_id = id.clone();
        tokio::task::spawn_blocking(move || {
            registry.set_status(&job_id, JobStatus::Running);
            let logger = JobLogger {
                id: job_id.clone(),
                registry: registry.clone(),
            };
            let status = match work(&logger) {
                Ok(()) => JobStatus::Succeeded,
                Err(e) => JobStatus::Failed {
                    message: format!("{e:#}"),
                },
            };
            registry.set_status(&job_id, status);
        });

        id
    }

    fn push_line(&self, id: &str, line: String) {
        let log = {
            let mut jobs = self.jobs.lock().expect("jobs registry lock poisoned");
            let Some(record) = jobs.get_mut(id) else {
                return;
            };
            record.log.push(line.clone());
            if record.log.len() > MAX_LOG_LINES {
                record.log.remove(0);
            }
            let _ = record.sender.send(JobEvent::Line(line));
            record.log.clone()
        };
        if let Err(e) = db_jobs::set_log(&self.db, id, &log) {
            tracing::warn!(id = %id, error = %e, "failed to persist job log");
        }
    }

    fn set_status(&self, id: &str, status: JobStatus) {
        {
            let mut jobs = self.jobs.lock().expect("jobs registry lock poisoned");
            let Some(record) = jobs.get_mut(id) else {
                return;
            };
            record.status = status.clone();
            let _ = record.sender.send(JobEvent::Status(status.clone()));
        }
        if let Ok((tag, payload)) = encode_status(&status)
            && let Err(e) = db_jobs::update_status(&self.db, id, &tag, &payload)
        {
            tracing::warn!(id = %id, error = %e, "failed to persist job status");
        }
    }

    pub fn get(&self, id: &str) -> Option<JobSnapshot> {
        let jobs = self.jobs.lock().expect("jobs registry lock poisoned");
        jobs.get(id).map(|r| snapshot(id, r))
    }

    pub fn list(&self) -> Vec<JobSummary> {
        let jobs = self.jobs.lock().expect("jobs registry lock poisoned");
        let mut summaries: Vec<JobSummary> = jobs.iter().map(|(id, r)| summary(id, r)).collect();
        summaries.sort_by_key(|s| std::cmp::Reverse(s.started_at));
        summaries
    }

    /// Returns the log buffered so far plus a receiver for events from this
    /// point on, so a client that connects mid-job sees the full history.
    pub fn subscribe(
        &self,
        id: &str,
    ) -> Option<(Vec<String>, JobStatus, broadcast::Receiver<JobEvent>)> {
        let jobs = self.jobs.lock().expect("jobs registry lock poisoned");
        jobs.get(id)
            .map(|r| (r.log.clone(), r.status.clone(), r.sender.subscribe()))
    }
}

fn snapshot(id: &str, record: &JobRecord) -> JobSnapshot {
    JobSnapshot {
        id: id.to_string(),
        kind: record.kind.clone(),
        status: record.status.clone(),
        started_at: record.started_at,
        log: record.log.clone(),
    }
}

fn summary(id: &str, record: &JobRecord) -> JobSummary {
    JobSummary {
        id: id.to_string(),
        kind: record.kind.clone(),
        status: record.status.clone(),
        started_at: record.started_at,
    }
}

fn encode_kind(kind: &JobKindDescr) -> Result<(String, String)> {
    let value = serde_json::to_value(kind)?;
    let tag = value
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Ok((tag, value.to_string()))
}

fn decode_kind(payload: &str) -> Result<JobKindDescr> {
    Ok(serde_json::from_str(payload)?)
}

fn encode_status(status: &JobStatus) -> Result<(String, String)> {
    let value = serde_json::to_value(status)?;
    let tag = value
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Ok((tag, value.to_string()))
}

fn decode_status(payload: &str) -> Result<JobStatus> {
    Ok(serde_json::from_str(payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn temp_registry(label: &str) -> JobRegistry {
        let dir = std::env::temp_dir().join(format!(
            "odin-jobs-test-{label}-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Arc::new(
            Db::open(&Paths {
                data_dir: dir.clone(),
                config_dir: dir,
            })
            .unwrap(),
        );
        JobRegistry::load(db)
    }

    #[tokio::test]
    async fn successful_job_ends_succeeded() {
        let registry = temp_registry("success");
        let id = registry.spawn(JobKindDescr::SteamcmdInstall, |logger| {
            logger.line("working");
            Ok(())
        });

        for _ in 0..100 {
            if matches!(registry.get(&id).unwrap().status, JobStatus::Succeeded) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let snapshot = registry.get(&id).unwrap();
        assert!(matches!(snapshot.status, JobStatus::Succeeded));
        assert_eq!(snapshot.log, vec!["working".to_string()]);
    }

    #[tokio::test]
    async fn failed_job_carries_message() {
        let registry = temp_registry("failure");
        let id = registry.spawn(JobKindDescr::SteamcmdInstall, |_logger| {
            anyhow::bail!("boom")
        });

        for _ in 0..100 {
            if !matches!(
                registry.get(&id).unwrap().status,
                JobStatus::Queued | JobStatus::Running
            ) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        match registry.get(&id).unwrap().status {
            JobStatus::Failed { message } => assert!(message.contains("boom")),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn late_subscriber_gets_buffered_log() {
        let registry = temp_registry("late-subscriber");
        let id = registry.spawn(JobKindDescr::SteamcmdInstall, |logger| {
            logger.line("first");
            std::thread::sleep(std::time::Duration::from_millis(50));
            Ok(())
        });

        let mut log = Vec::new();
        for _ in 0..100 {
            let (current_log, _status, _rx) = registry.subscribe(&id).expect("job should exist");
            if !current_log.is_empty() {
                log = current_log;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(log, vec!["first".to_string()]);
    }

    #[tokio::test]
    async fn job_history_survives_a_reload() {
        let dir = std::env::temp_dir().join(format!(
            "odin-jobs-test-reload-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Arc::new(
            Db::open(&Paths {
                data_dir: dir.clone(),
                config_dir: dir,
            })
            .unwrap(),
        );

        let id = {
            let registry = JobRegistry::load(db.clone());
            let id = registry.spawn(JobKindDescr::SteamcmdInstall, |logger| {
                logger.line("done");
                Ok(())
            });
            for _ in 0..100 {
                if matches!(registry.get(&id).unwrap().status, JobStatus::Succeeded) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            id
        };

        let reloaded = JobRegistry::load(db);
        let snapshot = reloaded.get(&id).expect("job should have been reloaded");
        assert!(matches!(snapshot.status, JobStatus::Succeeded));
        assert_eq!(snapshot.log, vec!["done".to_string()]);
    }

    #[tokio::test]
    async fn interrupted_job_is_reconciled_to_failed_on_reload() {
        let dir = std::env::temp_dir().join(format!(
            "odin-jobs-test-interrupted-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Arc::new(
            Db::open(&Paths {
                data_dir: dir.clone(),
                config_dir: dir,
            })
            .unwrap(),
        );
        let (kind_tag, kind_payload) = encode_kind(&JobKindDescr::SteamcmdInstall).unwrap();
        let (status_tag, status_payload) = encode_status(&JobStatus::Running).unwrap();
        db_jobs::insert(
            &db,
            "stuck-job",
            &kind_tag,
            &kind_payload,
            &status_tag,
            &status_payload,
            Utc::now(),
        )
        .unwrap();

        let registry = JobRegistry::load(db);
        let snapshot = registry.get("stuck-job").unwrap();
        match snapshot.status {
            JobStatus::Failed { message } => assert!(message.contains("restart")),
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
