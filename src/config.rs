use std::net::IpAddr;
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
    /// Reverse proxies (addresses or CIDR ranges) whose `X-Forwarded-For`
    /// header is believed. Loopback is always trusted.
    #[serde(default)]
    pub trusted_proxies: Vec<String>,
    /// A header a trusted proxy fills with the real client address, such as
    /// Cloudflare's `CF-Connecting-IP`. Preferred over `X-Forwarded-For`.
    #[serde(default)]
    pub client_ip_header: Option<String>,
}

/// Environment variable that overrides `[server].public_url`.
pub const PUBLIC_URL_ENV: &str = "ODIN_PUBLIC_URL";

/// Environment variable that overrides `[server].trusted_proxies`
/// (comma-separated addresses or CIDR ranges).
pub const TRUSTED_PROXIES_ENV: &str = "ODIN_TRUSTED_PROXIES";

/// Environment variable that overrides `[server].client_ip_header`.
pub const CLIENT_IP_HEADER_ENV: &str = "ODIN_CLIENT_IP_HEADER";

/// Networks whose `X-Forwarded-For` header is believed, so the client
/// address behind a reverse proxy is the real one. Loopback always counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies(Vec<(IpAddr, u8)>);

impl TrustedProxies {
    pub fn parse<'a>(entries: impl IntoIterator<Item = &'a str>) -> Result<Self> {
        let mut networks = Vec::new();
        for entry in entries.into_iter().map(str::trim).filter(|e| !e.is_empty()) {
            let (addr, prefix) = match entry.split_once('/') {
                Some((addr, prefix)) => (addr, Some(prefix)),
                None => (entry, None),
            };
            let addr: IpAddr = addr.parse().with_context(|| {
                format!("trusted proxy '{entry}' is not an IP address or CIDR range")
            })?;
            let max = if addr.is_ipv4() { 32 } else { 128 };
            let prefix = match prefix {
                Some(prefix) => prefix
                    .parse::<u8>()
                    .ok()
                    .filter(|p| *p <= max)
                    .with_context(|| {
                        format!("trusted proxy '{entry}' has an invalid prefix length")
                    })?,
                None => max,
            };
            networks.push((addr.to_canonical(), prefix));
        }
        Ok(Self(networks))
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        ip.is_loopback()
            || self.0.iter().any(|(network, prefix)| match (network, ip) {
                (IpAddr::V4(n), IpAddr::V4(i)) => {
                    prefix_matches(u32::from(*n).into(), u32::from(i).into(), *prefix, 32)
                }
                (IpAddr::V6(n), IpAddr::V6(i)) => {
                    prefix_matches(u128::from(*n), u128::from(i), *prefix, 128)
                }
                _ => false,
            })
    }
}

fn prefix_matches(network: u128, ip: u128, prefix: u8, bits: u8) -> bool {
    let host_bits = u32::from(bits - prefix);
    host_bits >= 128 || (network >> host_bits) == (ip >> host_bits)
}

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

    /// The header a trusted proxy puts the client address in:
    /// `ODIN_CLIENT_IP_HEADER` wins over `[server].client_ip_header`. A name
    /// that is not a valid header is an error, not silently ignored.
    pub fn client_ip_header(&self) -> Result<Option<axum::http::HeaderName>> {
        self.client_ip_header_with(std::env::var(CLIENT_IP_HEADER_ENV).ok())
    }

    fn client_ip_header_with(&self, env: Option<String>) -> Result<Option<axum::http::HeaderName>> {
        let clean = |value: String| Some(value.trim().to_string()).filter(|v| !v.is_empty());
        env.and_then(clean)
            .or_else(|| {
                self.server
                    .as_ref()
                    .and_then(|server| server.client_ip_header.clone())
                    .and_then(clean)
            })
            .map(|name| {
                axum::http::HeaderName::from_bytes(name.as_bytes()).with_context(|| {
                    format!("client_ip_header '{name}' is not a valid header name")
                })
            })
            .transpose()
    }

    /// Trusted reverse proxies: `ODIN_TRUSTED_PROXIES` wins over
    /// `[server].trusted_proxies`. Invalid entries are an error, so a typo
    /// cannot silently disable the setting.
    pub fn trusted_proxies(&self) -> Result<TrustedProxies> {
        self.trusted_proxies_with(std::env::var(TRUSTED_PROXIES_ENV).ok())
    }

    fn trusted_proxies_with(&self, env: Option<String>) -> Result<TrustedProxies> {
        match env.filter(|value| !value.trim().is_empty()) {
            Some(value) => TrustedProxies::parse(value.split(',')),
            None => TrustedProxies::parse(
                self.server
                    .iter()
                    .flat_map(|server| server.trusted_proxies.iter().map(String::as_str)),
            ),
        }
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
                ..Default::default()
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

    #[test]
    fn trusted_proxies_match_addresses_and_ranges() {
        let trusted = TrustedProxies::parse(["10.0.0.0/8", "192.168.1.5", " fd00::/8 "]).unwrap();
        for yes in [
            "127.0.0.1",
            "::1",
            "10.200.3.4",
            "192.168.1.5",
            "fd12::1",
            "::ffff:10.0.0.9",
        ] {
            assert!(trusted.contains(yes.parse().unwrap()), "{yes}");
        }
        for no in ["11.0.0.1", "192.168.1.6", "2001:db8::1", "8.8.8.8"] {
            assert!(!trusted.contains(no.parse().unwrap()), "{no}");
        }
        assert!(TrustedProxies::default().contains("127.0.0.1".parse().unwrap()));
        assert!(!TrustedProxies::default().contains("10.0.0.1".parse().unwrap()));
        assert!(
            TrustedProxies::parse(["0.0.0.0/0"])
                .unwrap()
                .contains("8.8.8.8".parse().unwrap())
        );
        for bad in ["nope", "10.0.0.0/33", "10.0.0.0/x", "::1/129"] {
            assert!(TrustedProxies::parse([bad]).is_err(), "{bad}");
        }
    }

    #[test]
    fn trusted_proxies_env_wins_over_the_config_file() {
        let config = GlobalConfig {
            server: Some(ServerConfig {
                trusted_proxies: vec!["10.0.0.0/8".into()],
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(
            config
                .trusted_proxies_with(None)
                .unwrap()
                .contains("10.1.1.1".parse().unwrap())
        );
        let env = config
            .trusted_proxies_with(Some("172.16.0.0/12, 192.0.2.1".into()))
            .unwrap();
        assert!(env.contains("172.20.0.1".parse().unwrap()));
        assert!(!env.contains("10.1.1.1".parse().unwrap()));
        assert!(
            config
                .trusted_proxies_with(Some("  ".into()))
                .unwrap()
                .contains("10.1.1.1".parse().unwrap())
        );
    }

    #[test]
    fn client_ip_header_is_validated_and_env_wins() {
        let config = GlobalConfig {
            server: Some(ServerConfig {
                client_ip_header: Some(" CF-Connecting-IP ".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let name = config.client_ip_header_with(None).unwrap().unwrap();
        assert_eq!(name.as_str(), "cf-connecting-ip");
        let env = config
            .client_ip_header_with(Some("X-Real-IP".into()))
            .unwrap()
            .unwrap();
        assert_eq!(env.as_str(), "x-real-ip");
        assert!(
            GlobalConfig::default()
                .client_ip_header_with(None)
                .unwrap()
                .is_none()
        );
        assert!(
            config
                .client_ip_header_with(Some("not a header".into()))
                .is_err()
        );
    }
}
