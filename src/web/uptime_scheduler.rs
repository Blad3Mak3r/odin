//! Keeps scheduled game instances inside their daily operating windows.

use std::time::Duration;

use chrono::{Local, Timelike};

use crate::activity::ActivityKind;
use crate::db::uptime_schedules::{self, ScheduledInstance};
use crate::game::instances;
use crate::web::runtime::InstanceTransition;
use crate::web::state::AppState;

const CHECK_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Start,
    Stop,
}

struct ScheduledAction {
    instance: ScheduledInstance,
    action: Action,
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            let tick_state = state.clone();
            let actions = tokio::task::spawn_blocking(move || due_actions(&tick_state))
                .await
                .unwrap_or_default();
            for action in actions {
                apply(&state, action).await;
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

fn due_actions(state: &AppState) -> Vec<ScheduledAction> {
    let minute = minute_of_day();
    let schedules = match uptime_schedules::enabled(&state.db) {
        Ok(schedules) => schedules,
        Err(error) => {
            tracing::warn!(%error, "failed to load uptime schedules");
            return Vec::new();
        }
    };

    schedules
        .into_iter()
        .filter_map(|instance| {
            let desired_running = instance.schedule.includes(minute);
            match instances::is_running(&state.paths, &state.db, instance.game, &instance.name) {
                Ok(running) if running != desired_running => Some(ScheduledAction {
                    action: if desired_running { Action::Start } else { Action::Stop },
                    instance,
                }),
                Ok(_) => None,
                Err(error) => {
                    tracing::warn!(game = %instance.game, instance = %instance.name, %error, "failed to inspect scheduled instance");
                    None
                }
            }
        })
        .collect()
}

async fn apply(state: &AppState, scheduled: ScheduledAction) {
    let ScheduledAction { instance, action } = scheduled;
    let transition = match action {
        Action::Start => InstanceTransition::Starting,
        Action::Stop => InstanceTransition::Stopping,
    };
    let _transition = match state.runtime.begin_game_transition(
        instance.game,
        &instance.name,
        transition,
    ) {
        Ok(transition) => transition,
        Err(error) => {
            tracing::debug!(game = %instance.game, instance = %instance.name, %error, "scheduled lifecycle action skipped");
            return;
        }
    };

    let result = match action {
        Action::Start => instances::start(&state.paths, &state.db, instance.game, &instance.name)
            .await
            .map(|_| ()),
        Action::Stop => {
            instances::stop(&state.paths, &state.db, instance.game, &instance.name).await
        }
    };
    match result {
        Ok(()) => state.activity.record_for(
            instance.game,
            match action {
                Action::Start => ActivityKind::InstanceStarted,
                Action::Stop => ActivityKind::InstanceStopped,
            },
            Some(instance.name),
        ),
        Err(error) => {
            tracing::warn!(game = %instance.game, instance = %instance.name, %error, "scheduled lifecycle action failed")
        }
    }
}

fn minute_of_day() -> u16 {
    let now = Local::now();
    (now.hour() * 60 + now.minute()) as u16
}

#[cfg(test)]
mod tests {
    use crate::db::uptime_schedules::UptimeSchedule;

    #[test]
    fn recognizes_a_daytime_window() {
        let schedule = UptimeSchedule {
            enabled: true,
            start_minute: 8 * 60,
            stop_minute: 22 * 60,
        };
        assert!(!schedule.includes(7 * 60 + 59));
        assert!(schedule.includes(8 * 60));
        assert!(schedule.includes(21 * 60 + 59));
        assert!(!schedule.includes(22 * 60));
    }

    #[test]
    fn recognizes_a_window_that_crosses_midnight() {
        let schedule = UptimeSchedule {
            enabled: true,
            start_minute: 22 * 60,
            stop_minute: 6 * 60,
        };
        assert!(schedule.includes(23 * 60));
        assert!(schedule.includes(2 * 60));
        assert!(!schedule.includes(12 * 60));
    }
}
