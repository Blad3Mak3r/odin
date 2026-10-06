//! Instance-local 7 Days to Die mod archives.

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Serialize;

use crate::game::GameId;
use crate::paths::Paths;

pub const MAX_ARCHIVE_ENTRIES: usize = 10_000;
pub const MAX_UNCOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct ModInfo {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub website: Option<String>,
}

fn mods_dir(paths: &Paths, instance: &str) -> PathBuf {
    paths
        .game_instance_dir(GameId::SevenDaysToDie, instance)
        .join("Mods")
}

fn value(xml: &str, tag: &str) -> Option<String> {
    let pattern = Regex::new(&format!(
        r#"(?is)<{}\s+[^>]*\bvalue\s*=\s*[\"']([^\"']*)[\"'][^>]*/?>"#,
        regex::escape(tag)
    ))
    .ok()?;
    pattern
        .captures(xml)
        .map(|captures| captures[1].trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn parse_mod_info(xml: &str) -> Result<ModInfo> {
    let name = value(xml, "Name").context("ModInfo.xml is missing Name")?;
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        bail!("ModInfo.xml Name may only contain letters, numbers, '_' and '-'");
    }
    Ok(ModInfo {
        name,
        display_name: value(xml, "DisplayName").context("ModInfo.xml is missing DisplayName")?,
        version: value(xml, "Version").context("ModInfo.xml is missing Version")?,
        description: value(xml, "Description"),
        author: value(xml, "Author"),
        website: value(xml, "Website"),
    })
}

fn archive_root_and_info(zip_path: &Path) -> Result<(String, ModInfo)> {
    let file = fs::File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).context("uploaded file is not a valid ZIP archive")?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        bail!("ZIP contains more than {MAX_ARCHIVE_ENTRIES} entries");
    }
    let mut roots = HashSet::new();
    let mut names = HashSet::new();
    let mut total = 0u64;
    let mut info = None;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let enclosed = entry
            .enclosed_name()
            .context("ZIP contains an unsafe path")?
            .to_path_buf();
        if enclosed.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            bail!("ZIP contains an unsafe path");
        }
        if !names.insert(enclosed.clone()) {
            bail!("ZIP contains duplicate paths");
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            bail!("ZIP contains a symbolic link");
        }
        total = total
            .checked_add(entry.size())
            .context("ZIP is too large")?;
        if total > MAX_UNCOMPRESSED_BYTES {
            bail!("ZIP expands beyond 2 GiB");
        }
        let mut components = enclosed.components();
        let Some(Component::Normal(root)) = components.next() else {
            bail!("ZIP entries must be inside one mod directory");
        };
        roots.insert(root.to_string_lossy().to_string());
        let rest = components.collect::<Vec<_>>();
        if rest.is_empty() && !entry.is_dir() {
            bail!("ZIP files must be inside one mod directory");
        }
        if rest.len() == 1
            && matches!(rest.first(), Some(Component::Normal(name)) if *name == "ModInfo.xml")
        {
            let mut xml = String::new();
            entry
                .read_to_string(&mut xml)
                .context("could not read ModInfo.xml")?;
            info = Some(parse_mod_info(&xml)?);
        }
    }
    if roots.len() != 1 {
        bail!("ZIP must contain exactly one mod directory");
    }
    Ok((
        roots.into_iter().next().expect("one root"),
        info.context("ZIP must contain ModInfo.xml directly inside its mod directory")?,
    ))
}

pub fn inspect_archive(zip_path: &Path) -> Result<ModInfo> {
    archive_root_and_info(zip_path).map(|(_, info)| info)
}

pub fn list(paths: &Paths, instance: &str) -> Result<Vec<ModInfo>> {
    let directory = mods_dir(paths, instance);
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut mods = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Ok(xml) = fs::read_to_string(entry.path().join("ModInfo.xml"))
            && let Ok(info) = parse_mod_info(&xml)
        {
            mods.push(info);
        }
    }
    mods.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(mods)
}

pub fn install(
    paths: &Paths,
    db: &crate::db::Db,
    instance: &str,
    zip_path: &Path,
    replace: bool,
) -> Result<ModInfo> {
    if crate::game::instances::is_running(paths, db, GameId::SevenDaysToDie, instance)? {
        bail!("stop the 7 Days to Die instance before changing mods");
    }
    let (root, info) = archive_root_and_info(zip_path)?;
    let mods = mods_dir(paths, instance);
    fs::create_dir_all(&mods)?;
    let target = mods.join(&info.name);
    if target.exists() && !replace {
        bail!(
            "mod '{}' is already installed; retry with replace=true",
            info.name
        );
    }
    let staging = mods.join(format!(".odin-install-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&staging)?;
    let result = (|| -> Result<()> {
        let file = fs::File::open(zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let relative = entry
                .enclosed_name()
                .context("ZIP contains an unsafe path")?
                .to_path_buf();
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                bail!("ZIP contains a symbolic link");
            }
            let destination = staging.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&destination)?;
            } else {
                let parent = destination.parent().context("ZIP path has no parent")?;
                fs::create_dir_all(parent)?;
                let mut output = fs::File::create(destination)?;
                std::io::copy(&mut entry, &mut output)?;
                output.flush()?;
            }
        }
        let extracted = staging.join(root);
        let extracted_info = parse_mod_info(&fs::read_to_string(extracted.join("ModInfo.xml"))?)?;
        if extracted_info.name != info.name {
            bail!("ModInfo.xml changed during extraction");
        }
        let backup = mods.join(format!(".odin-replaced-{}", uuid::Uuid::new_v4()));
        let had_previous = target.exists();
        if had_previous {
            fs::rename(&target, &backup)?;
        }
        if let Err(error) = fs::rename(&extracted, &target) {
            if had_previous {
                let _ = fs::rename(&backup, &target);
            }
            return Err(error.into());
        }
        if had_previous {
            fs::remove_dir_all(backup)?;
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&staging);
    result?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOD_INFO: &str = r#"<xml><Name value="Example_Mod"/><DisplayName value="Example Mod"/><Version value="1.2.3"/><Author value="Odin"/></xml>"#;

    #[test]
    fn parses_required_v2_metadata() {
        let info = parse_mod_info(MOD_INFO).unwrap();
        assert_eq!(info.name, "Example_Mod");
        assert_eq!(info.display_name, "Example Mod");
        assert_eq!(info.version, "1.2.3");
    }

    #[test]
    fn rejects_unsafe_internal_name() {
        assert!(
            parse_mod_info(
                r#"<xml><Name value="../bad"/><DisplayName value="Bad"/><Version value="1"/></xml>"#
            )
            .is_err()
        );
    }

    #[test]
    fn inspects_single_root_mod_archive() {
        let path = std::env::temp_dir().join(format!("odin-7d2d-mod-{}.zip", uuid::Uuid::new_v4()));
        let file = fs::File::create(&path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file(
                "Example_Mod/ModInfo.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(MOD_INFO.as_bytes()).unwrap();
        archive.finish().unwrap();
        assert_eq!(inspect_archive(&path).unwrap().name, "Example_Mod");
        fs::remove_file(path).unwrap();
    }
}
