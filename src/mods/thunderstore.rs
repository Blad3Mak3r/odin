use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use thiserror::Error;

use crate::paths::Paths;

const PACKAGE_INDEX_URL: &str = "https://thunderstore.io/c/valheim/api/v1/package/";
const INDEX_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
pub const BEPINEX_MOD_ID: &str = "denikson-BepInExPack_Valheim";

#[derive(Debug, Error)]
pub enum ThunderstoreError {
    #[error("mod '{0}' not found on Thunderstore")]
    PackageNotFound(String),
    #[error("mod '{mod_id}' has no version '{version}'")]
    VersionNotFound { mod_id: String, version: String },
    #[error(
        "'{0}' is not a valid mod id; expected '<namespace>-<name>' or '<namespace>-<name>-<version>'"
    )]
    InvalidModId(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThunderstorePackage {
    pub name: String,
    pub owner: String,
    pub versions: Vec<ThunderstoreVersion>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThunderstoreVersion {
    pub version_number: String,
    pub download_url: String,
}

pub struct ModRef {
    pub namespace: String,
    pub name: String,
    pub version: Option<String>,
}

impl ModRef {
    pub fn parse(mod_id: &str) -> Result<Self> {
        let parts: Vec<&str> = mod_id.split('-').collect();
        if parts.len() < 2 {
            bail!(ThunderstoreError::InvalidModId(mod_id.to_string()));
        }
        let namespace = parts[0].to_string();
        let rest = &parts[1..];

        let looks_like_version =
            |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.');

        if rest.len() >= 2 && looks_like_version(rest[rest.len() - 1]) {
            let version = rest[rest.len() - 1].to_string();
            let name = rest[..rest.len() - 1].join("-");
            Ok(Self {
                namespace,
                name,
                version: Some(version),
            })
        } else {
            Ok(Self {
                namespace,
                name: rest.join("-"),
                version: None,
            })
        }
    }

    pub fn mod_id(&self) -> String {
        format!("{}-{}", self.namespace, self.name)
    }
}

/// Fetches the Valheim Thunderstore package index, using a locally cached copy
/// if it's younger than `INDEX_CACHE_TTL`.
pub fn fetch_index(paths: &Paths) -> Result<Vec<ThunderstorePackage>> {
    let cache_file = paths.thunderstore_index_cache();

    let is_fresh = std::fs::metadata(&cache_file)
        .and_then(|m| m.modified())
        .map(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .unwrap_or(Duration::MAX)
                < INDEX_CACHE_TTL
        })
        .unwrap_or(false);
    if is_fresh
        && let Ok(raw) = std::fs::read_to_string(&cache_file)
        && let Ok(packages) = serde_json::from_str(&raw)
    {
        return Ok(packages);
    }

    tracing::info!(
        url = PACKAGE_INDEX_URL,
        "fetching Thunderstore package index"
    );
    let raw = reqwest::blocking::get(PACKAGE_INDEX_URL)
        .context("failed to fetch Thunderstore package index")?
        .error_for_status()
        .context("Thunderstore package index request returned an error status")?
        .text()
        .context("failed to read Thunderstore package index response")?;

    let packages: Vec<ThunderstorePackage> =
        serde_json::from_str(&raw).context("failed to parse Thunderstore package index")?;

    if let Some(parent) = cache_file.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&cache_file, &raw).ok();

    Ok(packages)
}

pub fn resolve<'a>(
    mod_ref: &ModRef,
    index: &'a [ThunderstorePackage],
) -> Result<(&'a ThunderstorePackage, &'a ThunderstoreVersion)> {
    let package = index
        .iter()
        .find(|p| p.owner == mod_ref.namespace && p.name == mod_ref.name)
        .ok_or_else(|| ThunderstoreError::PackageNotFound(mod_ref.mod_id()))?;

    let version = match &mod_ref.version {
        Some(v) => package
            .versions
            .iter()
            .find(|ver| &ver.version_number == v)
            .ok_or_else(|| ThunderstoreError::VersionNotFound {
                mod_id: mod_ref.mod_id(),
                version: v.clone(),
            })?,
        // Thunderstore returns versions newest-first.
        None => package
            .versions
            .first()
            .ok_or_else(|| ThunderstoreError::PackageNotFound(mod_ref.mod_id()))?,
    };

    Ok((package, version))
}

/// Downloads a package zip to a temp file and returns its path.
pub fn download_zip(url: &str, dest_dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dest_dir)?;
    let dest_file = dest_dir.join(format!("download-{}.zip", uuid_like()));

    let mut response = reqwest::blocking::get(url)
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("download of {url} returned an error status"))?;

    let mut file = std::fs::File::create(&dest_file)
        .with_context(|| format!("failed to create {}", dest_file.display()))?;
    std::io::copy(&mut response, &mut file).context("failed to write downloaded zip to disk")?;
    file.flush()?;

    Ok(dest_file)
}

fn uuid_like() -> String {
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mod_id_without_version() {
        let m = ModRef::parse("denikson-BepInExPack_Valheim").unwrap();
        assert_eq!(m.namespace, "denikson");
        assert_eq!(m.name, "BepInExPack_Valheim");
        assert_eq!(m.version, None);
        assert_eq!(m.mod_id(), "denikson-BepInExPack_Valheim");
    }

    #[test]
    fn parses_mod_id_with_version() {
        let m = ModRef::parse("denikson-BepInExPack_Valheim-5.4.2100").unwrap();
        assert_eq!(m.namespace, "denikson");
        assert_eq!(m.name, "BepInExPack_Valheim");
        assert_eq!(m.version.as_deref(), Some("5.4.2100"));
    }

    #[test]
    fn parses_hyphenated_name_without_version() {
        let m = ModRef::parse("owner-my-cool-mod").unwrap();
        assert_eq!(m.namespace, "owner");
        assert_eq!(m.name, "my-cool-mod");
        assert_eq!(m.version, None);
    }

    #[test]
    fn rejects_mod_id_without_name() {
        assert!(ModRef::parse("justnamespace").is_err());
    }
}
