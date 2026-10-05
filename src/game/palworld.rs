//! Local, authenticated access to Palworld's dedicated-server REST API.
//!
//! Odin deliberately never proxies this service to an arbitrary host: every
//! request is made to the managed instance's loopback REST port with the
//! password held in Odin's private configuration store.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::blocking::RequestBuilder;
use serde_json::{Value, json};

use crate::db::game_instances::GenericGameInstance;

const REST_TIMEOUT: Duration = Duration::from_secs(10);

pub fn rest_enabled(instance: &GenericGameInstance) -> bool {
    instance
        .config
        .settings
        .get("rest_api_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn request(instance: &GenericGameInstance, endpoint: &str) -> Result<RequestBuilder> {
    let settings = &instance.config.settings;
    if !rest_enabled(instance) {
        bail!("Palworld REST API is disabled for this instance");
    }
    let password = settings
        .get("admin_password")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .context("Palworld REST API requires an administrator password")?;
    let port = instance
        .config
        .admin_port
        .context("Palworld REST API port is not configured")?;
    let url = format!("http://127.0.0.1:{port}/v1/api/{endpoint}");
    Ok(crate::http::CLIENT
        .post(url)
        .basic_auth("admin", Some(password))
        .timeout(REST_TIMEOUT))
}

fn get(instance: &GenericGameInstance, endpoint: &str) -> Result<RequestBuilder> {
    let settings = &instance.config.settings;
    if !rest_enabled(instance) {
        bail!("Palworld REST API is disabled for this instance");
    }
    let password = settings
        .get("admin_password")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .context("Palworld REST API requires an administrator password")?;
    let port = instance
        .config
        .admin_port
        .context("Palworld REST API port is not configured")?;
    let url = format!("http://127.0.0.1:{port}/v1/api/{endpoint}");
    Ok(crate::http::CLIENT
        .get(url)
        .basic_auth("admin", Some(password))
        .timeout(REST_TIMEOUT))
}

fn send(request: RequestBuilder, operation: &str) -> Result<Value> {
    let response = request
        .send()
        .with_context(|| format!("failed to contact Palworld REST API for {operation}"))?;
    let status = response.status();
    let body = response
        .text()
        .with_context(|| format!("failed to read Palworld REST API response for {operation}"))?;
    if !status.is_success() {
        bail!("Palworld REST API {operation} failed with {status}: {body}");
    }
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&body)
        .with_context(|| format!("Palworld REST API returned invalid JSON for {operation}"))
}

pub fn players(instance: &GenericGameInstance) -> Result<Value> {
    send(get(instance, "players")?, "list players")
}

pub fn metrics(instance: &GenericGameInstance) -> Result<Value> {
    send(get(instance, "metrics")?, "read metrics")
}

pub fn announce(instance: &GenericGameInstance, message: &str) -> Result<Value> {
    send(
        request(instance, "announce")?.json(&json!({"message": message})),
        "announce",
    )
}

pub fn save(instance: &GenericGameInstance) -> Result<Value> {
    send(request(instance, "save")?.json(&json!({})), "save world")
}

pub fn kick(instance: &GenericGameInstance, user_id: &str, message: Option<&str>) -> Result<Value> {
    player_action(instance, "kick", user_id, message)
}

pub fn ban(instance: &GenericGameInstance, user_id: &str, message: Option<&str>) -> Result<Value> {
    player_action(instance, "ban", user_id, message)
}

pub fn unban(instance: &GenericGameInstance, user_id: &str) -> Result<Value> {
    send(
        request(instance, "unban")?.json(&json!({"userid": user_id})),
        "unban player",
    )
}

fn player_action(
    instance: &GenericGameInstance,
    action: &str,
    user_id: &str,
    message: Option<&str>,
) -> Result<Value> {
    let mut payload = serde_json::Map::from_iter([(String::from("userid"), json!(user_id))]);
    if let Some(message) = message.filter(|message| !message.is_empty()) {
        payload.insert(String::from("message"), json!(message));
    }
    send(request(instance, action)?.json(&payload), action)
}

pub fn shutdown(
    instance: &GenericGameInstance,
    wait_time: Option<u32>,
    message: Option<&str>,
) -> Result<Value> {
    let mut payload = serde_json::Map::new();
    if let Some(wait_time) = wait_time {
        payload.insert(String::from("waittime"), json!(wait_time));
    }
    if let Some(message) = message.filter(|message| !message.is_empty()) {
        payload.insert(String::from("message"), json!(message));
    }
    send(
        request(instance, "shutdown")?.json(&payload),
        "shutdown server",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::game_instances::{GameInstanceIdentity, GenericGameConfig};
    use crate::game::GameId;
    use chrono::Utc;

    #[test]
    fn disabled_rest_api_is_never_contacted() {
        let instance = GenericGameInstance {
            identity: GameInstanceIdentity {
                id: "id".into(),
                game: GameId::Palworld,
                name: "pal".into(),
                created_at: Utc::now(),
                tags: vec![],
            },
            config: GenericGameConfig {
                port: 8211,
                query_port: None,
                admin_port: Some(8212),
                settings: json!({"rest_api_enabled": false}),
                auto_restart: false,
            },
            pid: None,
            pid_started_at: None,
            last_started_at: None,
            last_stopped_at: None,
        };
        assert!(
            players(&instance)
                .unwrap_err()
                .to_string()
                .contains("disabled")
        );
    }
}
