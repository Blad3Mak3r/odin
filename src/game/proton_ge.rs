//! Managed Proton-GE runtime used exclusively by V Rising's Windows server.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[cfg(not(unix))]
use anyhow::bail;
use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use serde::Deserialize;

use crate::paths::Paths;

const RELEASE_URL: &str =
    "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases/latest";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub fn binary(paths: &Paths) -> PathBuf {
    paths.data_dir.join("runtimes/proton-ge/current/proton")
}

/// Returns a ready-to-use Proton-GE executable, downloading and caching the
/// latest release only when no activated runtime is available.
pub fn ensure(paths: &Paths) -> Result<PathBuf> {
    let current = binary(paths);
    if current.is_file() {
        return Ok(current);
    }
    let release: Release = crate::http::CLIENT
        .get(RELEASE_URL)
        .header(reqwest::header::USER_AGENT, "odin-server")
        .send()
        .context("failed to request Proton-GE releases")?
        .error_for_status()
        .context("Proton-GE release lookup failed")?
        .json()
        .context("invalid Proton-GE release response")?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name.ends_with(".tar.gz"))
        .context("latest Proton-GE release has no tar.gz runtime asset")?;
    let root = paths.data_dir.join("runtimes/proton-ge");
    let version = root.join("versions").join(&release.tag_name);
    if !version.join("proton").is_file() {
        let mut response = crate::http::CLIENT
            .get(&asset.browser_download_url)
            .header(reqwest::header::USER_AGENT, "odin-server")
            .send()
            .with_context(|| format!("failed to download {}", asset.name))?
            .error_for_status()
            .with_context(|| format!("download of {} failed", asset.name))?;
        let mut bytes = Vec::new();
        response.read_to_end(&mut bytes)?;
        install_archive(&root, &release.tag_name, &bytes)?;
    }
    activate(&root, &release.tag_name)?;
    Ok(current)
}

fn install_archive(root: &Path, tag: &str, bytes: &[u8]) -> Result<()> {
    let staging = root.join(format!(".staging-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&staging)?;
    let decoder = GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(&staging)
        .context("failed to extract Proton-GE archive")?;
    let extracted = fs::read_dir(&staging)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.join("proton").is_file())
        .context("Proton-GE archive does not contain a proton launcher")?;
    let versions = root.join("versions");
    fs::create_dir_all(&versions)?;
    let target = versions.join(tag);
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    fs::rename(&extracted, &target)?;
    fs::remove_dir_all(staging).ok();
    Ok(())
}

#[cfg(unix)]
fn activate(root: &Path, tag: &str) -> Result<()> {
    let current = root.join("current");
    if current.exists() || current.symlink_metadata().is_ok() {
        fs::remove_file(&current).or_else(|_| fs::remove_dir_all(&current))?;
    }
    std::os::unix::fs::symlink(Path::new("versions").join(tag), &current)?;
    Ok(())
}

#[cfg(not(unix))]
fn activate(_: &Path, _: &str) -> Result<()> {
    bail!("managed Proton-GE is only supported on Linux")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_runtime_is_activated_without_network_access() {
        let dir = std::env::temp_dir().join(format!("odin-proton-ge-{}", uuid::Uuid::new_v4()));
        let root = dir.join("runtimes/proton-ge");
        let version = root.join("versions/GE-Proton-test");
        fs::create_dir_all(&version).unwrap();
        fs::write(version.join("proton"), "launcher").unwrap();
        activate(&root, "GE-Proton-test").unwrap();
        assert_eq!(
            fs::read_to_string(root.join("current/proton")).unwrap(),
            "launcher"
        );
        fs::remove_dir_all(dir).ok();
    }
}
