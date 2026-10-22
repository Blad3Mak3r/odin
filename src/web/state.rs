use std::sync::{Arc, Mutex};

use sysinfo::System;

use crate::activity::ActivityLog;
use crate::db::Db;
use crate::odin_update::SystemUpdate;
use crate::paths::Paths;
use crate::web::auth::LoginThrottle;
use crate::web::jobs::JobRegistry;
use crate::web::log_tail::LogTailRegistry;
use crate::web::players::PlayerRegistry;
use crate::web::runtime::RuntimeRegistry;
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
    pub log_tail: LogTailRegistry,
    pub system_update: SystemUpdate,
    pub update_check_lock: Arc<Mutex<()>>,
    pub login_throttle: LoginThrottle,
}

impl AppState {
    pub fn new(paths: Paths, db: Arc<Db>) -> Self {
        let activity = ActivityLog::load(db.clone());
        let jobs = JobRegistry::load(db.clone());
        let runtime = RuntimeRegistry::new(db.clone());
        Self {
            paths: Arc::new(paths),
            db: db.clone(),
            jobs,
            resources: Arc::new(Mutex::new(System::new_all())),
            runtime,
            activity,
            players: PlayerRegistry::load(db),
            world_saves: WorldSaveRegistry::new(),
            log_tail: LogTailRegistry::new(),
            system_update: SystemUpdate::packaged(),
            update_check_lock: Arc::new(Mutex::new(())),
            login_throttle: LoginThrottle::default(),
        }
    }
}
