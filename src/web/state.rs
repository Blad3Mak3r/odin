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
    pub paths: Paths,
    pub db: Arc<Db>,
    pub jobs: JobRegistry,
    pub resources: Arc<Mutex<System>>,
    pub runtime: RuntimeRegistry,
    pub activity: ActivityLog,
    pub players: PlayerRegistry,
    #[expect(
        dead_code,
        reason = "wiring point for the LogTailRegistry event bridge landing in a follow-up phase; see web::supervisor's doc comment"
    )]
    pub supervisor: Supervisor,
    pub log_tail: LogTailRegistry,
}

impl AppState {
    pub fn new(paths: Paths, db: Arc<Db>) -> Self {
        let activity = ActivityLog::load(db.clone());
        Self {
            paths,
            db,
            jobs: JobRegistry::new(),
            resources: Arc::new(Mutex::new(System::new_all())),
            runtime: RuntimeRegistry::new(),
            activity,
            players: PlayerRegistry::new(),
            supervisor: Supervisor::new(),
            log_tail: LogTailRegistry::new(),
        }
    }
}
