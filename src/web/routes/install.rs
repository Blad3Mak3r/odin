use axum::Json;
use axum::extract::State;

use crate::activity::ActivityKind;
use crate::instance;
use crate::steamcmd::{SteamCmd, VALHEIM_DEDICATED_SERVER_APP_ID};
use crate::web::jobs::JobKindDescr;
use crate::web::routes::mods::JobHandle;
use crate::web::state::AppState;

// Returns a bare `Json<JobHandle>` rather than `ApiResult<...>` like other
// mutating routes: spawning a job onto the registry can't fail synchronously
// today, so there's nothing for `ApiResult` to wrap. This is intentional,
// not an oversight.
pub async fn install_server(State(state): State<AppState>) -> Json<JobHandle> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let activity = state.activity.clone();
    let id = state
        .jobs
        .spawn(JobKindDescr::SteamcmdInstall, move |logger| {
            let running = instance::running_instance_names(&paths, &db)?;
            if !running.is_empty() {
                anyhow::bail!(
                    "refusing to install/update while instance(s) are running: {}",
                    running.join(", ")
                );
            }

            let steamcmd = SteamCmd::new(paths.steamcmd_dir());
            let install_dir = paths.shared_install_dir();
            let log_file = paths.data_dir.join("logs").join("steamcmd-install.log");

            steamcmd.update_app(
                VALHEIM_DEDICATED_SERVER_APP_ID,
                &install_dir,
                &log_file,
                |line| {
                    logger.line(line);
                },
            )?;

            logger.line("done");
            activity.record(ActivityKind::ServerInstalled, None);
            Ok(())
        });
    Json(JobHandle { id })
}
