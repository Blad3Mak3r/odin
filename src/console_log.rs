//! Instance console-log naming, timestamps, and archive discovery.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};

/// Today's log; also the stable name every reader (tailer, CLI) opens.
pub const CURRENT: &str = "console.log";

const STAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

/// `line` prefixed with `at` as `2026-10-14T09:30:05.123Z <line>`, the format
/// the dashboard parses back into its timestamp box.
pub fn stamp_line(at: DateTime<Utc>, line: &str) -> String {
    format!("{} {line}", at.format(STAMP_FORMAT))
}

fn archive_name(day: NaiveDate) -> String {
    format!("console-{day}.log")
}

fn archive_day(name: &str) -> Option<NaiveDate> {
    let day: NaiveDate = name
        .strip_prefix("console-")?
        .strip_suffix(".log")?
        .parse()
        .ok()?;
    // Reject spellings like `console-2026-1-4.log` that parse but are not ours.
    (archive_name(day) == name).then_some(day)
}

fn modified_day(path: &Path) -> io::Result<NaiveDate> {
    let modified: DateTime<Utc> = fs::metadata(path)?.modified()?.into();
    Ok(modified.date_naive())
}

/// One day of logs available to read.
pub struct LogDay {
    pub date: NaiveDate,
    pub size: u64,
    pub path: PathBuf,
    /// Whether this is today's still-growing `console.log`.
    pub current: bool,
}

/// The days that have a log in `dir`, newest first. `console.log` is listed
/// under the day it was last written.
pub fn list_days(dir: &Path) -> Vec<LogDay> {
    let mut days = Vec::new();
    let current = dir.join(CURRENT);
    if let (Ok(metadata), Ok(date)) = (fs::metadata(&current), modified_day(&current)) {
        days.push(LogDay {
            date,
            size: metadata.len(),
            path: current,
            current: true,
        });
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let Some(date) = entry.file_name().to_str().and_then(archive_day) else {
                continue;
            };
            if days.iter().any(|day| day.date == date) {
                continue;
            }
            days.push(LogDay {
                date,
                size: entry.metadata().map(|m| m.len()).unwrap_or(0),
                path: entry.path(),
                current: false,
            });
        }
    }
    days.sort_by_key(|day| std::cmp::Reverse(day.date));
    days
}

/// The file holding `date`'s log, if there is one.
pub fn path_for_day(dir: &Path, date: NaiveDate) -> Option<PathBuf> {
    list_days(dir)
        .into_iter()
        .find(|day| day.date == date)
        .map(|day| day.path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    fn temp_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("odin-console-log-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap()
    }

    fn day(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
    }

    #[test]
    fn stamps_lines_with_a_parseable_utc_prefix() {
        assert_eq!(
            stamp_line(at(14, 9), "Server started"),
            "2026-10-14T09:00:00.000Z Server started"
        );
    }

    #[test]
    fn lists_days_newest_first_and_resolves_paths() {
        let dir = temp_dir("list");
        fs::write(dir.join("console-2026-10-12.log"), "a").unwrap();
        fs::write(dir.join("console-2026-10-13.log"), "bb").unwrap();
        fs::write(dir.join("console-2026-1-4.log"), "ignored").unwrap();
        fs::write(dir.join("update.log"), "ignored").unwrap();

        let days = list_days(&dir);
        let dates: Vec<_> = days.iter().map(|d| d.date).collect();
        assert_eq!(dates, [day(13), day(12)]);
        assert_eq!(days[0].size, 2);
        assert_eq!(
            path_for_day(&dir, day(12)),
            Some(dir.join("console-2026-10-12.log"))
        );
        assert_eq!(path_for_day(&dir, day(11)), None);
        fs::remove_dir_all(dir).ok();
    }
}
