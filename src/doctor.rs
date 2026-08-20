use std::time::Duration;

use anyhow::Result;

use crate::paths::Paths;
use crate::tmux;

struct Check {
    label: &'static str,
    ok: bool,
    critical: bool,
    detail: Option<String>,
}

pub fn run(paths: &Paths) -> Result<()> {
    let checks = vec![
        Check {
            label: "tmux installed",
            ok: tmux::has_binary(),
            critical: true,
            detail: None,
        },
        Check {
            label: "SteamCMD installed",
            ok: paths.steamcmd_dir().join("steamcmd.sh").is_file(),
            critical: false,
            detail: None,
        },
        Check {
            label: "Valheim dedicated server installed",
            ok: paths
                .shared_install_dir()
                .join("valheim_server.x86_64")
                .is_file(),
            critical: false,
            detail: None,
        },
        Check {
            label: "data directory writable",
            ok: is_writable(&paths.data_dir),
            critical: true,
            detail: Some(paths.data_dir.display().to_string()),
        },
        Check {
            label: "Thunderstore API reachable",
            ok: url_reachable("https://thunderstore.io/c/valheim/api/v1/package/"),
            critical: false,
            detail: None,
        },
        Check {
            label: "Steam CDN reachable",
            ok: url_reachable("https://steamcdn-a.akamaihd.net/"),
            critical: false,
            detail: None,
        },
    ];

    let mut critical_failed = false;
    for check in &checks {
        let mark = if check.ok {
            "OK"
        } else if check.critical {
            "FAIL"
        } else {
            "WARN"
        };
        let suffix = check
            .detail
            .as_deref()
            .map(|d| format!(" ({d})"))
            .unwrap_or_default();
        println!("[{mark:>4}] {}{suffix}", check.label);
        if !check.ok && check.critical {
            critical_failed = true;
        }
    }

    if critical_failed {
        anyhow::bail!("one or more critical checks failed");
    }
    Ok(())
}

fn is_writable(dir: &std::path::Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".doctor-write-probe");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    std::fs::remove_file(&probe).ok();
    ok
}

fn url_reachable(url: &str) -> bool {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .and_then(|client| client.get(url).send())
        .map(|resp| resp.status().is_success())
        .unwrap_or(false)
}
