use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;

use anyhow::{Context, Result, bail};
use thiserror::Error;

const STEAMCMD_URL: &str = "https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz";
pub const VALHEIM_DEDICATED_SERVER_APP_ID: &str = "896660";

#[derive(Debug, Error)]
pub enum SteamCmdError {
    #[error("steamcmd exited with a failure status: {0}")]
    CommandFailed(String),
    #[error(
        "steamcmd ran but the expected server binary was not found at {0}; check the log for details"
    )]
    BinaryMissingAfterUpdate(String),
}

pub struct SteamCmd {
    steamcmd_dir: PathBuf,
}

impl SteamCmd {
    pub fn new(steamcmd_dir: PathBuf) -> Self {
        Self { steamcmd_dir }
    }

    fn script_path(&self) -> PathBuf {
        self.steamcmd_dir.join("steamcmd.sh")
    }

    pub fn is_installed(&self) -> bool {
        self.script_path().is_file()
    }

    /// Downloads and unpacks SteamCMD into `steamcmd_dir` if it isn't already there.
    pub fn ensure_installed(&self) -> Result<()> {
        if self.is_installed() {
            return Ok(());
        }

        std::fs::create_dir_all(&self.steamcmd_dir).with_context(|| {
            format!(
                "failed to create steamcmd directory {}",
                self.steamcmd_dir.display()
            )
        })?;

        tracing::info!(url = STEAMCMD_URL, "downloading SteamCMD");
        let response = reqwest::blocking::get(STEAMCMD_URL)
            .context("failed to download SteamCMD")?
            .error_for_status()
            .context("SteamCMD download returned an error status")?;
        let bytes = response
            .bytes()
            .context("failed to read SteamCMD download body")?;

        let tar = flate2::read::GzDecoder::new(bytes.as_ref());
        let mut archive = tar::Archive::new(tar);
        archive
            .unpack(&self.steamcmd_dir)
            .context("failed to unpack SteamCMD archive")?;

        if !self.is_installed() {
            bail!(
                "SteamCMD archive was unpacked but {} was not found",
                self.script_path().display()
            );
        }

        Ok(())
    }

    /// Runs `steamcmd.sh +login anonymous +force_install_dir <install_dir> +app_update <app_id> validate +quit`,
    /// calling `on_line` with each line of combined stdout/stderr as it's
    /// produced (so a caller can stream progress instead of waiting for the
    /// whole, possibly multi-minute, run to finish) and also writing the
    /// same combined output to `log_file`.
    pub fn update_app(
        &self,
        app_id: &str,
        install_dir: &Path,
        log_file: &Path,
        on_line: impl FnMut(&str),
    ) -> Result<()> {
        self.update_app_expect_file(
            app_id,
            install_dir,
            log_file,
            install_dir.join("valheim_server.x86_64"),
            on_line,
        )
    }

    /// Generic variant of [`Self::update_app`] for another game's known
    /// executable. SteamCMD succeeding alone is not enough: a missing binary
    /// normally means an incompatible depot or incomplete install.
    pub fn update_app_expect_file(
        &self,
        app_id: &str,
        install_dir: &Path,
        log_file: &Path,
        expected_file: PathBuf,
        mut on_line: impl FnMut(&str),
    ) -> Result<()> {
        self.ensure_installed()?;
        std::fs::create_dir_all(install_dir)
            .with_context(|| format!("failed to create install dir {}", install_dir.display()))?;
        if let Some(parent) = log_file.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create log dir {}", parent.display()))?;
        }

        let install_dir_str = install_dir.to_string_lossy();
        tracing::info!(app_id, install_dir = %install_dir_str, "running steamcmd app_update");

        let mut child = Command::new(self.script_path())
            .arg(format!("+force_install_dir {install_dir_str}"))
            .arg("+login anonymous")
            .arg(format!("+app_update {app_id} validate"))
            .arg("+quit")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("failed to invoke steamcmd.sh")?;

        let mut log_file_handle = std::fs::File::create(log_file)
            .with_context(|| format!("failed to create log file {}", log_file.display()))?;

        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let stdout_tx = tx.clone();
        let stdout_thread = std::thread::spawn(move || stream_lines(stdout, &stdout_tx));
        let stderr_thread = std::thread::spawn(move || stream_lines(stderr, &tx));

        for line in rx {
            writeln!(log_file_handle, "{line}").ok();
            on_line(&line);
        }

        stdout_thread.join().ok();
        stderr_thread.join().ok();
        let status = child.wait().context("failed to wait on steamcmd.sh")?;

        if !status.success() {
            bail!(SteamCmdError::CommandFailed(format!(
                "exit status {status} (see log at {})",
                log_file.display()
            )));
        }

        if !expected_file.is_file() {
            bail!(SteamCmdError::BinaryMissingAfterUpdate(
                expected_file.display().to_string()
            ));
        }

        Ok(())
    }

    /// Runs `steamcmd.sh +login anonymous +app_info_print <app_id> +quit`
    /// and returns its stdout (Valve's VDF-formatted app info) verbatim, for
    /// callers that need Steam's live metadata for an app — e.g. the
    /// current `buildid` on a branch — without downloading/validating the
    /// app itself. Unlike `update_app`, this doesn't need a
    /// `force_install_dir` and runs to completion in a few seconds.
    pub fn app_info_print(&self, app_id: &str) -> Result<String> {
        self.ensure_installed()?;
        tracing::info!(app_id, "running steamcmd app_info_print");

        let output = Command::new(self.script_path())
            .arg("+login anonymous")
            .arg(format!("+app_info_print {app_id}"))
            .arg("+quit")
            .output()
            .context("failed to invoke steamcmd.sh")?;

        if !output.status.success() {
            bail!(SteamCmdError::CommandFailed(format!(
                "exit status {} while running app_info_print",
                output.status
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Reads the `buildid` field out of the ACF manifest SteamCMD writes at
/// `<install_dir>/steamapps/appmanifest_<app_id>.acf` after an `app_update`.
/// Returns `None` if the app isn't installed there (no manifest, or no
/// `buildid` field), which callers use to distinguish "not installed" from
/// "installed at build N".
pub fn installed_build_id(install_dir: &Path, app_id: &str) -> Option<u64> {
    let manifest_path = install_dir
        .join("steamapps")
        .join(format!("appmanifest_{app_id}.acf"));
    let contents = std::fs::read_to_string(manifest_path).ok()?;
    extract_quoted_u64_field(&contents, "buildid")
}

/// Scans VDF/ACF text (Valve's simple `"key" "value"` keyfile format) for a
/// top-level-in-this-text `"key"    "123"` line and parses its value. Used
/// both on a flat ACF manifest and, by `crate::valheim_update`, on a single
/// isolated block sliced out of a larger `app_info_print` VDF dump via
/// [`find_vdf_block`].
pub(crate) fn extract_quoted_u64_field(vdf_text: &str, key: &str) -> Option<u64> {
    let key_pat = format!("\"{key}\"");
    vdf_text.lines().find_map(|line| {
        let line = line.trim();
        if !line.starts_with(&key_pat) {
            return None;
        }
        line.split('"').nth(3).and_then(|value| value.parse().ok())
    })
}

/// Finds `"key" { ... }` in `vdf_text` and returns the `...` slice (braces
/// excluded), tracking brace depth so nested blocks don't confuse the match.
/// Returns `None` if `key` or a following `{...}` block isn't found.
pub(crate) fn find_vdf_block<'a>(vdf_text: &'a str, key: &str) -> Option<&'a str> {
    let key_pat = format!("\"{key}\"");
    let key_start = vdf_text.find(&key_pat)?;
    let after_key = &vdf_text[key_start + key_pat.len()..];
    let brace_offset = after_key.find('{')?;
    let block_start = key_start + key_pat.len() + brace_offset + 1;

    let bytes = vdf_text.as_bytes();
    let mut depth = 1i32;
    let mut idx = block_start;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&vdf_text[block_start..idx]);
                }
            }
            _ => {}
        }
        idx += 1;
    }
    None
}

/// Reads `source` line by line, forwarding each to `tx` as it arrives. Runs
/// on its own thread so stdout and stderr can be drained concurrently
/// without either pipe's OS buffer filling up and deadlocking the child.
fn stream_lines(source: impl Read, tx: &Sender<String>) {
    for line in BufReader::new(source).lines() {
        match line {
            Ok(line) => {
                if tx.send(line).is_err() {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MANIFEST: &str = r#""AppState"
{
	"appid"		"896660"
	"universe"		"1"
	"name"		"Valheim Dedicated Server"
	"StateFlags"		"4"
	"installdir"		"valheim"
	"LastUpdated"		"1735689600"
	"buildid"		"12345678"
	"SizeOnDisk"		"1234567890"
}
"#;

    const SAMPLE_APP_INFO: &str = r#""896660"
{
	"common"
	{
		"name"		"Valheim Dedicated Server"
	}
	"depots"
	{
		"branches"
		{
			"public"
			{
				"buildid"		"21981590"
				"timeupdated"		"1771576792"
			}
			"default_old"
			{
				"buildid"		"20460518"
				"description"		"Previous stable"
			}
		}
	}
}
"#;

    #[test]
    fn extract_quoted_u64_field_reads_the_buildid_field() {
        assert_eq!(
            extract_quoted_u64_field(SAMPLE_MANIFEST, "buildid"),
            Some(12345678)
        );
    }

    #[test]
    fn extract_quoted_u64_field_missing_field_is_none() {
        assert_eq!(
            extract_quoted_u64_field("\"AppState\"\n{\n\t\"appid\"\t\"896660\"\n}\n", "buildid"),
            None
        );
    }

    #[test]
    fn find_vdf_block_extracts_a_nested_branch_and_ignores_siblings() {
        let branches = find_vdf_block(SAMPLE_APP_INFO, "branches").expect("branches block");
        let public = find_vdf_block(branches, "public").expect("public block");
        assert_eq!(extract_quoted_u64_field(public, "buildid"), Some(21981590));

        let default_old = find_vdf_block(branches, "default_old").expect("default_old block");
        assert_eq!(
            extract_quoted_u64_field(default_old, "buildid"),
            Some(20460518)
        );
    }

    #[test]
    fn find_vdf_block_missing_key_is_none() {
        assert_eq!(find_vdf_block(SAMPLE_APP_INFO, "nonexistent"), None);
    }

    #[test]
    fn installed_build_id_missing_manifest_is_none() {
        let dir = std::env::temp_dir().join(format!(
            "odin-steamcmd-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        assert_eq!(installed_build_id(&dir, "896660"), None);
    }

    #[test]
    fn installed_build_id_reads_from_manifest_file() {
        let dir = std::env::temp_dir().join(format!(
            "odin-steamcmd-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let steamapps = dir.join("steamapps");
        std::fs::create_dir_all(&steamapps).unwrap();
        std::fs::write(steamapps.join("appmanifest_896660.acf"), SAMPLE_MANIFEST).unwrap();

        assert_eq!(installed_build_id(&dir, "896660"), Some(12345678));

        std::fs::remove_dir_all(&dir).ok();
    }
}
