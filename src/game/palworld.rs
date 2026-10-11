//! Local, authenticated access to Palworld's dedicated-server REST API.
//!
//! Odin deliberately never proxies this service to an arbitrary host: every
//! request is made to the managed instance's loopback REST port. Enablement,
//! port, and password are read directly from PalWorldSettings.ini.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::blocking::RequestBuilder;
use serde_json::{Value, json};

use crate::db::game_instances::GenericGameInstance;

const REST_TIMEOUT: Duration = Duration::from_secs(10);

fn setting(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    key: &str,
) -> Result<String> {
    crate::game::config_documents::value(paths, instance, key)?
        .context("Palworld configuration has not been generated yet")
}

pub fn rest_enabled(paths: &crate::paths::Paths, instance: &GenericGameInstance) -> Result<bool> {
    Ok(matches!(
        setting(paths, instance, "RESTAPIEnabled")?.as_str(),
        "True" | "true" | "1"
    ))
}

fn request(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    endpoint: &str,
) -> Result<RequestBuilder> {
    if !rest_enabled(paths, instance)? {
        bail!("Palworld REST API is disabled for this instance");
    }
    let password = setting(paths, instance, "AdminPassword")?;
    anyhow::ensure!(
        !password.is_empty(),
        "Palworld REST API requires an administrator password"
    );
    let port = setting(paths, instance, "RESTAPIPort")?
        .parse::<u16>()
        .context("Palworld REST API port is invalid")?;
    let url = format!("http://127.0.0.1:{port}/v1/api/{endpoint}");
    Ok(crate::http::CLIENT
        .post(url)
        .basic_auth("admin", Some(password))
        .timeout(REST_TIMEOUT))
}

fn get(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    endpoint: &str,
) -> Result<RequestBuilder> {
    if !rest_enabled(paths, instance)? {
        bail!("Palworld REST API is disabled for this instance");
    }
    let password = setting(paths, instance, "AdminPassword")?;
    anyhow::ensure!(
        !password.is_empty(),
        "Palworld REST API requires an administrator password"
    );
    let port = setting(paths, instance, "RESTAPIPort")?
        .parse::<u16>()
        .context("Palworld REST API port is invalid")?;
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

pub fn players(paths: &crate::paths::Paths, instance: &GenericGameInstance) -> Result<Value> {
    send(get(paths, instance, "players")?, "list players")
}

pub fn metrics(paths: &crate::paths::Paths, instance: &GenericGameInstance) -> Result<Value> {
    send(get(paths, instance, "metrics")?, "read metrics")
}

pub fn announce(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    message: &str,
) -> Result<Value> {
    send(
        request(paths, instance, "announce")?.json(&json!({"message": message})),
        "announce",
    )
}

pub fn save(paths: &crate::paths::Paths, instance: &GenericGameInstance) -> Result<Value> {
    send(
        request(paths, instance, "save")?.json(&json!({})),
        "save world",
    )
}

pub fn kick(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    user_id: &str,
    message: Option<&str>,
) -> Result<Value> {
    player_action(paths, instance, "kick", user_id, message)
}

pub fn ban(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    user_id: &str,
    message: Option<&str>,
) -> Result<Value> {
    player_action(paths, instance, "ban", user_id, message)
}

pub fn unban(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    user_id: &str,
) -> Result<Value> {
    send(
        request(paths, instance, "unban")?.json(&json!({"userid": user_id})),
        "unban player",
    )
}

fn player_action(
    paths: &crate::paths::Paths,
    instance: &GenericGameInstance,
    action: &str,
    user_id: &str,
    message: Option<&str>,
) -> Result<Value> {
    let mut payload = serde_json::Map::from_iter([(String::from("userid"), json!(user_id))]);
    if let Some(message) = message.filter(|message| !message.is_empty()) {
        payload.insert(String::from("message"), json!(message));
    }
    send(request(paths, instance, action)?.json(&payload), action)
}

pub fn shutdown(
    paths: &crate::paths::Paths,
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
        request(paths, instance, "shutdown")?.json(&payload),
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
                admin_port: None,
                auto_restart: false,
            },
            pid: None,
            pid_started_at: None,
            last_started_at: None,
            last_stopped_at: None,
        };
        let dir = std::env::temp_dir().join(format!("odin-palworld-rest-{}", uuid::Uuid::new_v4()));
        let paths = crate::paths::Paths {
            data_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        let config = paths
            .game_instance_dir(GameId::Palworld, "pal")
            .join("runtime/Pal/Saved/Config/LinuxServer/PalWorldSettings.ini");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(config, "[/Script/Pal.PalGameWorldSettings]\nOptionSettings=(RESTAPIEnabled=False,AdminPassword=secret,RESTAPIPort=8212)\n").unwrap();
        assert!(
            players(&paths, &instance)
                .unwrap_err()
                .to_string()
                .contains("disabled")
        );
        std::fs::remove_dir_all(paths.data_dir).ok();
    }
}
