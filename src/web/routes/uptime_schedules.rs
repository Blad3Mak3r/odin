//! API for per-instance daily operating windows.

use axum::Json;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

use crate::db::uptime_schedules::{self, UptimeSchedule};
use crate::game::{GameId, instances};
use crate::web::error::{ApiResult, BadRequest, run_blocking};
use crate::web::state::AppState;

const DEFAULT_START_MINUTE: u16 = 8 * 60;
const DEFAULT_STOP_MINUTE: u16 = 22 * 60;

#[derive(Debug, Clone, Serialize)]
pub struct UptimeScheduleView {
    pub enabled: bool,
    /// A `HH:MM` time in the host's local time zone.
    pub start_time: String,
    /// A `HH:MM` time in the host's local time zone.
    pub stop_time: String,
}

impl Default for UptimeScheduleView {
    fn default() -> Self {
        Self {
            enabled: false,
            start_time: format_time(DEFAULT_START_MINUTE),
            stop_time: format_time(DEFAULT_STOP_MINUTE),
        }
    }
}

impl From<UptimeSchedule> for UptimeScheduleView {
    fn from(schedule: UptimeSchedule) -> Self {
        Self {
            enabled: schedule.enabled,
            start_time: format_time(schedule.start_minute),
            stop_time: format_time(schedule.stop_minute),
        }
    }
}

#[derive(Deserialize)]
pub struct SetUptimeScheduleRequest {
    pub enabled: bool,
    pub start_time: String,
    pub stop_time: String,
}

pub async fn get_uptime_schedule(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
) -> ApiResult<Json<UptimeScheduleView>> {
    let paths = state.paths.clone();
    let db = state.db.clone();
    let schedule = run_blocking(move || {
        instances::load(&paths, &db, game, &name)?;
        uptime_schedules::get_for_game(&db, game, &name)
    })
    .await?;
    Ok(Json(schedule.map(Into::into).unwrap_or_default()))
}

pub async fn set_uptime_schedule(
    State(state): State<AppState>,
    Path((game, name)): Path<(GameId, String)>,
    Json(request): Json<SetUptimeScheduleRequest>,
) -> ApiResult<Json<UptimeScheduleView>> {
    let start_minute = parse_time(&request.start_time)?;
    let stop_minute = parse_time(&request.stop_time)?;
    if start_minute == stop_minute {
        return Err(BadRequest("start_time and stop_time must be different".to_string()).into());
    }
    let schedule = UptimeSchedule {
        enabled: request.enabled,
        start_minute,
        stop_minute,
    };
    let schedule_to_save = schedule.clone();
    let paths = state.paths.clone();
    let db = state.db.clone();
    run_blocking(move || {
        instances::load(&paths, &db, game, &name)?;
        uptime_schedules::upsert_for_game(&db, game, &name, &schedule_to_save)
    })
    .await?;
    Ok(Json(schedule.into()))
}

fn parse_time(value: &str) -> Result<u16, BadRequest> {
    let bytes = value.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 2 || byte.is_ascii_digit())
    {
        return Err(BadRequest("time must use the HH:MM format".to_string()));
    }
    let hour = value[0..2].parse::<u16>().expect("validated ASCII digits");
    let minute = value[3..5].parse::<u16>().expect("validated ASCII digits");
    if hour > 23 || minute > 59 {
        return Err(BadRequest(
            "time must be between 00:00 and 23:59".to_string(),
        ));
    }
    Ok(hour * 60 + minute)
}

fn format_time(minute: u16) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_times() {
        assert_eq!(parse_time("00:00").unwrap(), 0);
        assert_eq!(parse_time("23:59").unwrap(), 1439);
        assert_eq!(format_time(8 * 60 + 5), "08:05");
    }

    #[test]
    fn rejects_invalid_times() {
        for value in ["8:00", "24:00", "12:60", "12-00", "noon"] {
            assert!(parse_time(value).is_err(), "{value} should be invalid");
        }
    }
}
