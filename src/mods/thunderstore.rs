use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use thiserror::Error;

use crate::db::Db;

const PACKAGE_INDEX_URL: &str = "https://thunderstore.io/c/valheim/api/v1/package/";
const INDEX_CACHE_KEY: &str = "thunderstore_index";
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
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub icon: Option<String>,
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
pub fn fetch_index(db: &Db) -> Result<Vec<ThunderstorePackage>> {
    if let Some(entry) = crate::db::cache::get(db, INDEX_CACHE_KEY)?
        && let Ok(age) = (chrono::Utc::now() - entry.fetched_at).to_std()
        && age < INDEX_CACHE_TTL
        && let Ok(packages) = serde_json::from_str(&entry.value)
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

    crate::db::cache::set(db, INDEX_CACHE_KEY, &raw).ok();

    Ok(packages)
}

/// Case-insensitive substring match on package name/owner, best matches first:
/// an exact/prefix/substring match on the name outranks one on the owner, and
/// ties are broken by download count so popular mods surface first.
pub fn search<'a>(index: &'a [ThunderstorePackage], query: &str) -> Vec<&'a ThunderstorePackage> {
    let q = query.to_lowercase();
    let mut scored: Vec<(u8, u64, &ThunderstorePackage)> = index
        .iter()
        .filter_map(|p| {
            let name = p.name.to_lowercase();
            let owner = p.owner.to_lowercase();
            if !name.contains(&q) && !owner.contains(&q) {
                return None;
            }
            let downloads = p.versions.first().map_or(0, |v| v.downloads);
            Some((relevance_rank(&q, &name, &owner), downloads, p))
        })
        .collect();

    scored.sort_by_key(|(rank, downloads, _)| std::cmp::Reverse((*rank, *downloads)));
    scored.into_iter().map(|(_, _, p)| p).collect()
}

fn relevance_rank(query: &str, name_lower: &str, owner_lower: &str) -> u8 {
    if name_lower == query {
        4
    } else if name_lower.starts_with(query) {
        3
    } else if name_lower.contains(query) {
        2
    } else if owner_lower == query || owner_lower.starts_with(query) {
        1
    } else {
        0
    }
}

/// Looks up the icon URL for an already-installed mod: matches `mod_id`'s
/// exact `version` if the package still carries it, falling back to the
/// package's newest version (e.g. the installed version was since pruned
/// from Thunderstore). Returns `None` if the mod isn't in the index at all
/// (deregistered, or the index just failed to fetch and is empty).
pub fn find_icon(index: &[ThunderstorePackage], mod_id: &str, version: &str) -> Option<String> {
    let mod_ref = ModRef::parse(mod_id).ok()?;
    let package = index
        .iter()
        .find(|p| p.owner == mod_ref.namespace && p.name == mod_ref.name)?;
    package
        .versions
        .iter()
        .find(|v| v.version_number == version)
        .or_else(|| package.versions.first())
        .and_then(|v| v.icon.clone())
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
    let dest_file = dest_dir.join(format!("download-{}.zip", uuid::Uuid::new_v4()));

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

    fn package(owner: &str, name: &str, downloads: u64) -> ThunderstorePackage {
        ThunderstorePackage {
            name: name.to_string(),
            owner: owner.to_string(),
            versions: vec![ThunderstoreVersion {
                version_number: "1.0.0".to_string(),
                download_url: String::new(),
                description: String::new(),
                downloads,
                icon: None,
            }],
        }
    }

    #[test]
    fn search_ranks_name_prefix_above_mere_substring_match() {
        let index = vec![
            package("someone", "AutoValheimPlus", 1000),
            package("nathanhwood", "ValheimPlus", 10),
        ];
        let results = search(&index, "ValheimPlus");
        assert_eq!(results[0].name, "ValheimPlus");
        assert_eq!(results[1].name, "AutoValheimPlus");
    }

    #[test]
    fn search_breaks_ties_by_downloads() {
        let index = vec![
            package("owner-a", "CoolMod", 5),
            package("owner-b", "CoolMod", 500),
        ];
        let results = search(&index, "coolmod");
        assert_eq!(results[0].owner, "owner-b");
        assert_eq!(results[1].owner, "owner-a");
    }

    #[test]
    fn search_matches_owner_when_name_does_not_match() {
        let index = vec![package("denikson", "BepInExPack_Valheim", 1)];
        let results = search(&index, "denikson");
        assert_eq!(results.len(), 1);
    }

    fn package_with_versions(
        owner: &str,
        name: &str,
        versions: Vec<(&str, Option<&str>)>,
    ) -> ThunderstorePackage {
        ThunderstorePackage {
            name: name.to_string(),
            owner: owner.to_string(),
            versions: versions
                .into_iter()
                .map(|(version_number, icon)| ThunderstoreVersion {
                    version_number: version_number.to_string(),
                    download_url: String::new(),
                    description: String::new(),
                    downloads: 0,
                    icon: icon.map(str::to_string),
                })
                .collect(),
        }
    }

    #[test]
    fn find_icon_matches_exact_installed_version() {
        let index = vec![package_with_versions(
            "owner",
            "CoolMod",
            vec![
                ("2.0.0", Some("https://example.com/2.0.0.png")),
                ("1.0.0", Some("https://example.com/1.0.0.png")),
            ],
        )];
        assert_eq!(
            find_icon(&index, "owner-CoolMod", "1.0.0").as_deref(),
            Some("https://example.com/1.0.0.png")
        );
    }

    #[test]
    fn find_icon_falls_back_to_newest_version_when_installed_version_is_gone() {
        // Thunderstore returns versions newest-first, mirroring `resolve`.
        let index = vec![package_with_versions(
            "owner",
            "CoolMod",
            vec![("2.0.0", Some("https://example.com/2.0.0.png"))],
        )];
        assert_eq!(
            find_icon(&index, "owner-CoolMod", "1.0.0").as_deref(),
            Some("https://example.com/2.0.0.png")
        );
    }

    #[test]
    fn find_icon_is_none_when_mod_is_not_in_the_index() {
        let index = vec![package_with_versions(
            "owner",
            "CoolMod",
            vec![("1.0.0", Some("https://example.com/1.0.0.png"))],
        )];
        assert_eq!(find_icon(&index, "someone-else-Mod", "1.0.0"), None);
    }
}
