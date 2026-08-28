use std::sync::{Arc, Mutex};

use sysinfo::System;

use crate::activity::ActivityLog;
use crate::db::Db;
use crate::paths::Paths;
use crate::web::jobs::JobRegistry;
use crate::web::log_tail::LogTailRegistry;
use crate::web::players::PlayerRegistry;
use crate::web::runtime::RuntimeRegistry;
use crate::web::supervisor::Supervisor;

#[derive(Clone)]
pub struct AppState {
    pub paths: Arc<Paths>,
    pub db: Arc<Db>,
    pub jobs: JobRegistry,
    pub resources: Arc<Mutex<System>>,
    pub runtime: RuntimeRegistry,
    pub activity: ActivityLog,
    pub players: PlayerRegistry,
    pub supervisor: Supervisor,
    pub log_tail: LogTailRegistry,
}

impl AppState {
    pub fn new(paths: Paths, db: Arc<Db>) -> Self {
        let activity = ActivityLog::load(db.clone());
        let jobs = JobRegistry::load(db.clone());
        Self {
            paths: Arc::new(paths),
            db,
            jobs,
            resources: Arc::new(Mutex::new(System::new_all())),
            runtime: RuntimeRegistry::new(),
            activity,
            players: PlayerRegistry::new(),
            supervisor: Supervisor::new(),
            log_tail: LogTailRegistry::new(),
        }
    }
}
