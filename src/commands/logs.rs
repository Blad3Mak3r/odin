use std::io::{Read as _, Seek as _, SeekFrom};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::instance::Instance;
use crate::paths::{self, Paths};

pub fn run(paths: &Paths, server_name: &str, follow: bool, lines: usize) -> Result<()> {
    let instance = Instance::load_existing(paths, server_name)?;
    let log_file = paths::instance_logs_dir(&instance.dir).join("console.log");
    if !log_file.is_file() {
        bail!("no logs yet for '{server_name}'; start it first with `valheim start {server_name}`");
    }

    let content = std::fs::read_to_string(&log_file)
        .with_context(|| format!("failed to read {}", log_file.display()))?;
    let tail: Vec<&str> = content.lines().rev().take(lines).collect();
    for line in tail.into_iter().rev() {
        println!("{line}");
    }

    if follow {
        let mut file = std::fs::File::open(&log_file)
            .with_context(|| format!("failed to open {}", log_file.display()))?;
        file.seek(SeekFrom::End(0))?;
        let mut buf = Vec::new();
        loop {
            buf.clear();
            file.read_to_end(&mut buf)?;
            if !buf.is_empty() {
                print!("{}", String::from_utf8_lossy(&buf));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    Ok(())
}
