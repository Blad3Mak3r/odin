use std::sync::{Arc, Mutex};

use sysinfo::System;

use crate::paths::Paths;
use crate::web::jobs::JobRegistry;

#[derive(Clone)]
pub struct AppState {
    pub paths: Paths,
    pub jobs: JobRegistry,
    pub resources: Arc<Mutex<System>>,
}

impl AppState {
    pub fn new(paths: Paths) -> Self {
        Self {
            paths,
            jobs: JobRegistry::new(),
            resources: Arc::new(Mutex::new(System::new_all())),
        }
    }
}
