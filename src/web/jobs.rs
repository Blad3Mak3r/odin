//! In-memory registry for long-running background operations (SteamCMD
//! installs/updates, mod installs) that must not block the HTTP request that
//! kicked them off. A job is spawned onto the blocking thread pool; its
//! status and a capped log buffer are kept in memory and broadcast to any
//! WebSocket subscribers watching it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

pub type JobId = String;

const MAX_LOG_LINES: usize = 2000;
const BROADCAST_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobKindDescr {
    SteamcmdInstall,
    ModAdd { instance: String, mod_id: String },
    ModUpdate { instance: String },
    BackupCreate { instance: String },
    BackupRestore { instance: String, backup_id: String },
}

#[derive(Debug, Clone, Serialize)]
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
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
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
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
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
        let (sender, _receiver) = broadcast::channel(BROADCAST_CAPACITY);
        {
            let mut jobs = self.jobs.lock().expect("jobs registry lock poisoned");
            jobs.insert(
                id.clone(),
                JobRecord {
                    kind,
                    status: JobStatus::Queued,
                    started_at: Utc::now(),
                    log: Vec::new(),
                    sender,
                },
            );
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
        let mut jobs = self.jobs.lock().expect("jobs registry lock poisoned");
        if let Some(record) = jobs.get_mut(id) {
            record.log.push(line.clone());
            if record.log.len() > MAX_LOG_LINES {
                record.log.remove(0);
            }
            let _ = record.sender.send(JobEvent::Line(line));
        }
    }

    fn set_status(&self, id: &str, status: JobStatus) {
        let mut jobs = self.jobs.lock().expect("jobs registry lock poisoned");
        if let Some(record) = jobs.get_mut(id) {
            record.status = status.clone();
            let _ = record.sender.send(JobEvent::Status(status));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn successful_job_ends_succeeded() {
        let registry = JobRegistry::new();
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
        let registry = JobRegistry::new();
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
        let registry = JobRegistry::new();
        let id = registry.spawn(JobKindDescr::SteamcmdInstall, |logger| {
            logger.line("first");
            std::thread::sleep(std::time::Duration::from_millis(50));
            Ok(())
        });

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let (log, _status, _rx) = registry.subscribe(&id).expect("job should exist");
        assert_eq!(log, vec!["first".to_string()]);
    }
}
