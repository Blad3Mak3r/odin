use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::instance::Instance;
use crate::paths::{self, Paths};

/// Chunk size used to scan backward from the end of the log file when
/// looking for the last `max_lines` newlines.
const TAIL_CHUNK_SIZE: u64 = 64 * 1024;

pub fn run(paths: &Paths, server_name: &str, follow: bool, lines: usize) -> Result<()> {
    let instance = Instance::load_existing(paths, server_name)?;
    let log_file = paths::instance_logs_dir(&instance.dir).join("console.log");
    if !log_file.is_file() {
        bail!("no logs yet for '{server_name}'; start it first with `odin start {server_name}`");
    }

    let tail = read_tail(&log_file, lines)
        .with_context(|| format!("failed to read {}", log_file.display()))?;
    for line in tail.lines() {
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

/// Returns the last `max_lines` lines of `path`, without reading more of the
/// file than needed: scans backward in `TAIL_CHUNK_SIZE` chunks until enough
/// newlines have been seen (or the start of the file is reached), instead of
/// loading the whole file — the console log can grow to many MB over a long
/// server run, and callers here only ever want a small tail of it.
pub fn read_tail(path: &Path, max_lines: usize) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let file_len = file.metadata()?.len();

    let mut pos = file_len;
    let mut newline_count = 0usize;
    let mut buf = Vec::new();
    while pos > 0 && newline_count <= max_lines {
        let read_size = TAIL_CHUNK_SIZE.min(pos);
        pos -= read_size;
        file.seek(SeekFrom::Start(pos))?;
        let mut chunk = vec![0u8; read_size as usize];
        file.read_exact(&mut chunk)?;
        newline_count += chunk.iter().filter(|&&b| b == b'\n').count();
        chunk.extend_from_slice(&buf);
        buf = chunk;
    }

    let text = String::from_utf8_lossy(&buf);
    let tail: Vec<&str> = text.lines().rev().take(max_lines).collect();
    Ok(tail.into_iter().rev().collect::<Vec<_>>().join("\n"))
}
