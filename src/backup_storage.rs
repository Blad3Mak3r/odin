use std::fs::File;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::blocking::{Client, Response};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use serde::{Deserialize, Serialize};

const SIGNED_URL_TTL: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageProvider {
    AwsS3,
    CloudflareR2,
}

impl StorageProvider {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::AwsS3 => "aws_s3",
            Self::CloudflareR2 => "cloudflare_r2",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "aws_s3" => Some(Self::AwsS3),
            "cloudflare_r2" => Some(Self::CloudflareR2),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupStorageConfig {
    pub provider: StorageProvider,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteObject {
    pub provider: StorageProvider,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub key: String,
}

impl BackupStorageConfig {
    pub fn object_for(&self, instance_name: &str, backup_id: &str) -> RemoteObject {
        let filename = format!("{instance_name}/{backup_id}.zip");
        let key = if self.prefix.is_empty() {
            filename
        } else {
            format!("{}/{filename}", self.prefix)
        };
        RemoteObject {
            provider: self.provider,
            endpoint: self.endpoint.clone(),
            region: self.region.clone(),
            bucket: self.bucket.clone(),
            key,
        }
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.bucket.len() < 3
            || self.bucket.len() > 63
            || !self.bucket.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte)
            })
            || !self
                .bucket
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            || !self
                .bucket
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
        {
            return Err(
                "bucket must be 3-63 lowercase letters, numbers, periods, or hyphens and start/end with a letter or number"
                    .to_string(),
            );
        }
        if self.prefix.len() > 200 {
            return Err("prefix must be at most 200 characters".to_string());
        }
        if self.access_key_id.trim().is_empty() {
            return Err("access_key_id is required".to_string());
        }
        if self.secret_access_key.trim().is_empty() {
            return Err("secret_access_key is required".to_string());
        }
        if self.region.trim().is_empty()
            || !self
                .region
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err("region is invalid".to_string());
        }
        let endpoint = reqwest::Url::parse(&self.endpoint)
            .map_err(|_| "endpoint must be a valid HTTPS URL".to_string())?;
        if endpoint.scheme() != "https"
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.path() != "/"
        {
            return Err(
                "endpoint must be an HTTPS origin without credentials, path, query, or fragment"
                    .to_string(),
            );
        }
        Ok(())
    }
}

pub fn upload(config: &BackupStorageConfig, object: &RemoteObject, source: &Path) -> Result<()> {
    let (bucket, credentials) = signed_bucket(config, object)?;
    let url = bucket
        .put_object(Some(&credentials), &object.key)
        .sign(SIGNED_URL_TTL);
    let file = File::open(source)
        .with_context(|| format!("failed to open backup {} for upload", source.display()))?;
    let size = file.metadata()?.len();
    let response = http_client()?
        .put(url)
        .header(reqwest::header::CONTENT_LENGTH, size)
        .body(file)
        .send()
        .map_err(reqwest::Error::without_url)
        .context("backup upload request failed")?;
    ensure_success(response, "backup upload")
}

pub fn download(
    config: &BackupStorageConfig,
    object: &RemoteObject,
    destination: &Path,
) -> Result<()> {
    let (bucket, credentials) = signed_bucket(config, object)?;
    let url = bucket
        .get_object(Some(&credentials), &object.key)
        .sign(SIGNED_URL_TTL);
    let mut response = http_client()?
        .get(url)
        .send()
        .map_err(reqwest::Error::without_url)
        .context("backup download request failed")?;
    if !response.status().is_success() {
        return ensure_success(response, "backup download");
    }
    let mut file = File::create(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;
    std::io::copy(&mut response, &mut file)
        .with_context(|| format!("failed to download backup to {}", destination.display()))?;
    Ok(())
}

pub fn delete(config: &BackupStorageConfig, object: &RemoteObject) -> Result<()> {
    let (bucket, credentials) = signed_bucket(config, object)?;
    let url = bucket
        .delete_object(Some(&credentials), &object.key)
        .sign(SIGNED_URL_TTL);
    let response = http_client()?
        .delete(url)
        .send()
        .map_err(reqwest::Error::without_url)
        .context("remote backup deletion request failed")?;
    ensure_success(response, "remote backup deletion")
}

fn signed_bucket(
    config: &BackupStorageConfig,
    object: &RemoteObject,
) -> Result<(Bucket, Credentials)> {
    let endpoint = object
        .endpoint
        .parse()
        .with_context(|| format!("invalid backup storage endpoint '{}'", object.endpoint))?;
    let url_style = if object.bucket.contains('.') {
        UrlStyle::Path
    } else {
        UrlStyle::VirtualHost
    };
    let bucket = Bucket::new(
        endpoint,
        url_style,
        object.bucket.clone(),
        object.region.clone(),
    )
    .context("invalid backup storage bucket")?;
    let credentials = Credentials::new(
        config.access_key_id.clone(),
        config.secret_access_key.clone(),
    );
    Ok((bucket, credentials))
}

fn http_client() -> Result<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60 * 60))
        .build()
        .context("failed to build backup storage HTTP client")
}

fn ensure_success(response: Response, operation: &str) -> Result<()> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let detail = response.text().unwrap_or_default();
    let detail = detail.trim();
    if detail.is_empty() {
        bail!("{operation} failed with HTTP {status}");
    }
    bail!("{operation} failed with HTTP {status}: {detail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_for_uses_prefix_instance_and_backup_id() {
        let config = BackupStorageConfig {
            provider: StorageProvider::AwsS3,
            endpoint: "https://s3.eu-west-1.amazonaws.com".to_string(),
            region: "eu-west-1".to_string(),
            bucket: "odin-backups".to_string(),
            prefix: "valheim/prod".to_string(),
            access_key_id: "access".to_string(),
            secret_access_key: "secret".to_string(),
            enabled: true,
        };

        let object = config.object_for("my-server", "20260101T000000Z");

        assert_eq!(object.key, "valheim/prod/my-server/20260101T000000Z.zip");
    }

    #[test]
    fn validate_rejects_endpoints_with_embedded_credentials() {
        let config = BackupStorageConfig {
            provider: StorageProvider::CloudflareR2,
            endpoint: "https://user:password@account.r2.cloudflarestorage.com".to_string(),
            region: "auto".to_string(),
            bucket: "odin-backups".to_string(),
            prefix: "valheim".to_string(),
            access_key_id: "access".to_string(),
            secret_access_key: "secret".to_string(),
            enabled: true,
        };

        assert!(config.validate().is_err());
    }
}
