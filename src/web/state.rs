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
use crate::web::world_saves::WorldSaveRegistry;

#[derive(Clone)]
pub struct AppState {
    pub paths: Arc<Paths>,
    pub db: Arc<Db>,
    pub jobs: JobRegistry,
    pub resources: Arc<Mutex<System>>,
    pub runtime: RuntimeRegistry,
    pub activity: ActivityLog,
    pub players: PlayerRegistry,
    pub world_saves: WorldSaveRegistry,
    pub supervisor: Supervisor,
    pub log_tail: LogTailRegistry,
}

impl AppState {
    pub fn new(paths: Paths, db: Arc<Db>) -> Self {
        let activity = ActivityLog::load(db.clone());
        let jobs = JobRegistry::load(db.clone());
        let runtime = RuntimeRegistry::new(db.clone());
        Self {
            paths: Arc::new(paths),
            db,
            jobs,
            resources: Arc::new(Mutex::new(System::new_all())),
            runtime,
            activity,
            players: PlayerRegistry::new(),
            world_saves: WorldSaveRegistry::new(),
            supervisor: Supervisor::new(),
            log_tail: LogTailRegistry::new(),
        }
    }
}
