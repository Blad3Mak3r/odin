use std::sync::{Arc, Mutex};

use sysinfo::System;

use crate::activity::ActivityLog;
use crate::paths::Paths;
use crate::web::jobs::JobRegistry;
use crate::web::players::PlayerRegistry;
use crate::web::runtime::RuntimeRegistry;

#[derive(Clone)]
pub struct AppState {
    pub paths: Paths,
    pub jobs: JobRegistry,
    pub resources: Arc<Mutex<System>>,
    pub runtime: RuntimeRegistry,
    pub activity: ActivityLog,
    pub players: PlayerRegistry,
}

impl AppState {
    pub fn new(paths: Paths) -> Self {
        let activity = ActivityLog::load(&paths.data_dir);
        Self {
            paths,
            jobs: JobRegistry::new(),
            resources: Arc::new(Mutex::new(System::new_all())),
            runtime: RuntimeRegistry::new(),
            activity,
            players: PlayerRegistry::new(),
        }
    }
}
