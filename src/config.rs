use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::game::RuntimeImage;
use crate::paths::Paths;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Override for the data dir; if None, `Paths::resolve` falls back to the
    /// XDG default (or the `ODIN_DATA_DIR` env var).
    #[serde(default)]
    pub data_dir: Option<PathBuf>,
    /// Podman's API socket. It is deliberately opt-in: access to this socket
    /// is equivalent to authority over the service user's containers.
    #[serde(default)]
    pub podman: Option<PodmanConfig>,
    /// Optional CurseForge credentials for the ARK: Survival Ascended mod
    /// picker. Without a key the dashboard falls back to typing mod ids.
    #[serde(default)]
    pub curseforge: Option<CurseForgeConfig>,
    /// Optional dashboard settings.
    #[serde(default)]
    pub server: Option<ServerConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerConfig {
    /// The origin users type to reach the dashboard when it sits behind a
    /// reverse proxy, e.g. `https://odin.example.com`. Browsers' `Origin`
    /// headers for it are accepted, and an `https` value marks the session
    /// cookie `Secure` and enables HSTS.
    #[serde(default)]
    pub public_url: Option<String>,
}

/// Environment variable that overrides `[server].public_url`.
pub const PUBLIC_URL_ENV: &str = "ODIN_PUBLIC_URL";

/// A validated `scheme://host[:port]` origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicOrigin {
    pub origin: String,
    pub https: bool,
}

impl PublicOrigin {
    pub fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim().trim_end_matches('/');
        let (scheme, authority) = raw
            .split_once("://")
            .with_context(|| format!("public_url '{raw}' must look like https://host[:port]"))?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme != "http" && scheme != "https" {
            bail!("public_url '{raw}' must use http or https");
        }
        let valid = !authority.is_empty()
            && authority
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'));
        if !valid {
            bail!("public_url '{raw}' must be an origin only: no path, query or credentials");
        }
        Ok(Self {
            origin: format!("{scheme}://{}", authority.to_ascii_lowercase()),
            https: scheme == "https",
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CurseForgeConfig {
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Environment variable that overrides `[curseforge].api_key`.
pub const CURSEFORGE_API_KEY_ENV: &str = "CURSE_FORGE_API_KEY";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodmanConfig {
    pub socket: PathBuf,
    /// Deprecated alias for `base_image` retained for existing installations.
    #[serde(default)]
    pub runtime_image: Option<String>,
    #[serde(default)]
    pub base_image: Option<String>,
    #[serde(default)]
    pub proton_image: Option<String>,
    #[serde(default)]
    pub odin_binary: Option<PathBuf>,
}

impl PodmanConfig {
    pub fn image(&self, runtime: RuntimeImage) -> String {
        let base = || {
            self.base_image
                .as_ref()
                .or(self.runtime_image.as_ref())
                .cloned()
                .unwrap_or_else(|| "ghcr.io/blad3mak3r/odin-srv:base".into())
        };
        match runtime {
            RuntimeImage::Base => base(),
            RuntimeImage::Proton => self
                .proton_image
                .clone()
                .unwrap_or_else(|| "ghcr.io/blad3mak3r/odin-srv:proton".into()),
        }
    }

    /// Host path of the `odin` binary that containers bind-mount. It is
    /// resolved here, not left as `/proc/self/exe`: Podman resolves bind
    /// sources in its own process, where that link points at Podman itself.
    /// Fails early when the file cannot be executed, rather than letting the
    /// container die at start with an opaque OCI error.
    pub fn odin_binary(&self) -> Result<PathBuf> {
        let path = match &self.odin_binary {
            Some(path) => path.clone(),
            None => {
                let exe = std::env::current_exe().context("failed to locate the odin binary")?;
                // After a self-update replaces the binary, the kernel reports
                // the old, unlinked inode as "<path> (deleted)".
                match exe.to_str().and_then(|p| p.strip_suffix(" (deleted)")) {
                    Some(live) => PathBuf::from(live),
                    None => exe,
                }
            }
        };
        validate_odin_binary(&path)?;
        Ok(path)
    }
}

fn validate_odin_binary(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if !path.is_absolute() {
        bail!(
            "odin_binary {} must be an absolute path on the Podman host",
            path.display()
        );
    }
    let meta = std::fs::metadata(path).with_context(|| {
        format!(
            "odin binary {} is not accessible; it is bind-mounted into every game container (set [podman].odin_binary)",
            path.display()
        )
    })?;
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
        bail!("odin binary {} is not an executable file", path.display());
    }
    Ok(())
}

impl GlobalConfig {
    pub fn load(paths: &Paths) -> Result<Self> {
        let config_file = paths.config_file();
        if !config_file.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&config_file)
            .with_context(|| format!("failed to read config file {}", config_file.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("failed to parse config file {}", config_file.display()))
    }

    /// The dashboard's public origin: `ODIN_PUBLIC_URL` wins over
    /// `[server].public_url`. Blank values count as unset; an invalid value is
    /// an error so a typo cannot silently weaken the origin checks.
    pub fn public_origin(&self) -> Result<Option<PublicOrigin>> {
        self.public_origin_with(std::env::var(PUBLIC_URL_ENV).ok())
    }

    fn public_origin_with(&self, env: Option<String>) -> Result<Option<PublicOrigin>> {
        let clean = |value: String| Some(value.trim().to_string()).filter(|v| !v.is_empty());
        env.and_then(clean)
            .or_else(|| {
                self.server
                    .as_ref()
                    .and_then(|server| server.public_url.clone())
                    .and_then(clean)
            })
            .map(|raw| PublicOrigin::parse(&raw))
            .transpose()
    }

    /// The CurseForge API key: the `CURSE_FORGE_API_KEY` environment
    /// variable wins over `[curseforge].api_key`. Blank values count as unset.
    pub fn curseforge_api_key(&self) -> Option<String> {
        self.curseforge_api_key_with(std::env::var(CURSEFORGE_API_KEY_ENV).ok())
    }

    fn curseforge_api_key_with(&self, env: Option<String>) -> Option<String> {
        let clean = |key: String| Some(key.trim().to_string()).filter(|key| !key.is_empty());
        env.and_then(clean).or_else(|| {
            self.curseforge
                .as_ref()
                .and_then(|config| config.api_key.clone())
                .and_then(clean)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn podman_config() -> PodmanConfig {
        PodmanConfig {
            socket: PathBuf::from("/run/podman/podman.sock"),
            runtime_image: Some("example/legacy:base".into()),
            base_image: None,
            proton_image: None,
            odin_binary: None,
        }
    }

    #[test]
    fn runtime_images_use_defaults_and_the_legacy_base_alias() {
        let config = podman_config();
        assert_eq!(config.image(RuntimeImage::Base), "example/legacy:base");
        assert_eq!(
            config.image(RuntimeImage::Proton),
            "ghcr.io/blad3mak3r/odin-srv:proton"
        );
    }

    #[test]
    fn explicit_images_take_precedence() {
        let mut config = podman_config();
        config.base_image = Some("example/base@sha256:abc".into());
        config.proton_image = Some("example/proton@sha256:def".into());
        assert_eq!(config.image(RuntimeImage::Base), "example/base@sha256:abc");
        assert_eq!(
            config.image(RuntimeImage::Proton),
            "example/proton@sha256:def"
        );
    }

    #[test]
    fn odin_binary_defaults_to_the_running_executable() {
        let resolved = podman_config().odin_binary().unwrap();
        assert_eq!(resolved, std::env::current_exe().unwrap());
        assert_ne!(resolved, Path::new("/proc/self/exe"));
    }

    #[test]
    fn odin_binary_rejects_unusable_paths() {
        let dir = std::env::temp_dir().join(format!("odin-bin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let plain = dir.join("odin");
        std::fs::write(&plain, b"").unwrap();

        let mut config = podman_config();
        for bad in [
            dir.join("missing"),
            dir.clone(),
            plain.clone(),
            "odin".into(),
        ] {
            config.odin_binary = Some(bad.clone());
            assert!(config.odin_binary().is_err(), "{} accepted", bad.display());
        }

        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o755)).unwrap();
        config.odin_binary = Some(plain.clone());
        assert_eq!(config.odin_binary().unwrap(), plain);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn curseforge_key_prefers_the_environment_over_the_config_file() {
        let config: GlobalConfig =
            toml::from_str("[curseforge]\napi_key = \"$2a$10$file\"\n").unwrap();
        assert_eq!(
            config.curseforge_api_key_with(None).as_deref(),
            Some("$2a$10$file")
        );
        assert_eq!(
            config
                .curseforge_api_key_with(Some("  from-env \n".into()))
                .as_deref(),
            Some("from-env")
        );
    }

    #[test]
    fn curseforge_key_ignores_blank_values_and_a_missing_section() {
        let config: GlobalConfig = toml::from_str("[curseforge]\napi_key = \"  \"\n").unwrap();
        assert_eq!(config.curseforge_api_key_with(Some(" ".into())), None);
        assert_eq!(GlobalConfig::default().curseforge_api_key_with(None), None);
        let empty: GlobalConfig = toml::from_str("[curseforge]\n").unwrap();
        assert_eq!(empty.curseforge_api_key_with(None), None);
    }

    #[test]
    fn public_origin_is_validated_and_env_wins() {
        let config = GlobalConfig {
            server: Some(ServerConfig {
                public_url: Some("HTTPS://Odin.Example.com/".into()),
            }),
            ..Default::default()
        };
        let parsed = config.public_origin_with(None).unwrap().unwrap();
        assert_eq!(parsed.origin, "https://odin.example.com");
        assert!(parsed.https);

        let env = config
            .public_origin_with(Some("http://10.0.0.2:8080".into()))
            .unwrap()
            .unwrap();
        assert_eq!(env.origin, "http://10.0.0.2:8080");
        assert!(!env.https);

        assert!(
            config
                .public_origin_with(Some("   ".into()))
                .unwrap()
                .is_some()
        );
        assert!(
            GlobalConfig::default()
                .public_origin_with(None)
                .unwrap()
                .is_none()
        );
        for bad in [
            "odin.example.com",
            "ftp://x",
            "https://x/path",
            "https://u:p@x",
        ] {
            assert!(PublicOrigin::parse(bad).is_err(), "{bad}");
        }
    }
}
