//! `odin serve`'s handle onto `crate::supervisor` (the `odin run` RPC
//! layer). `instance::lifecycle` already talks to it directly for
//! start/stop, and `web::routes::resources::compute_instance_snapshot`
//! pings it for liveness — this type is currently just the wiring point
//! `AppState` holds for what comes next: bridging each running instance's
//! pushed log/exit events into `LogTailRegistry`, replacing
//! `web::log_tail`'s file-polling for any instance with a live control
//! socket. Until then, `web::log_tail`'s poller keeps doing that job for
//! every instance, same as before this module existed.

#[derive(Clone, Default)]
pub struct Supervisor;

impl Supervisor {
    pub fn new() -> Self {
        Self
    }
}
