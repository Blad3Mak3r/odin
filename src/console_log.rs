//! Daily-cycled, timestamped instance console logs.
//!
//! The runtime (`odin run`) owns an instance's `logs/` directory: every line
//! the game prints is stamped with the moment it was read and appended to
//! `console.log` (today's file). When the UTC date changes the file is renamed
//! to `console-YYYY-MM-DD.log` and a fresh `console.log` starts; archives older
//! than [`RETENTION_DAYS`] are deleted. The web API lists and reads those
//! archives so the dashboard can show previous days.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, NaiveDate, TimeDelta, Utc};

/// Today's log; also the stable name every reader (tailer, CLI) opens.
pub const CURRENT: &str = "console.log";

/// Days of archived logs kept besides today's.
pub const RETENTION_DAYS: i64 = 14;

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

fn open_current(dir: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(CURRENT))
}

/// Moves `console.log` to the archive for `day`, merging into it if one
/// already exists. An empty log is simply dropped.
fn archive_current(dir: &Path, day: NaiveDate) -> io::Result<()> {
    let current = dir.join(CURRENT);
    let Ok(metadata) = fs::metadata(&current) else {
        return Ok(());
    };
    if metadata.len() == 0 {
        return fs::remove_file(&current);
    }
    let target = dir.join(archive_name(day));
    if target.exists() {
        let mut out = OpenOptions::new().append(true).open(&target)?;
        io::copy(&mut File::open(&current)?, &mut out)?;
        fs::remove_file(&current)
    } else {
        fs::rename(&current, &target)
    }
}

fn prune(dir: &Path, today: NaiveDate) -> io::Result<()> {
    let oldest_kept = today - TimeDelta::days(RETENTION_DAYS);
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let expired = entry
            .file_name()
            .to_str()
            .and_then(archive_day)
            .is_some_and(|day| day < oldest_kept);
        if expired {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// The single writer of an instance's `console.log`.
pub struct ConsoleLog {
    dir: PathBuf,
    file: File,
    day: NaiveDate,
}

impl ConsoleLog {
    /// Opens today's log in `dir`. A `console.log` left over from an earlier
    /// day (instance stopped, or logs written before cycling existed) is
    /// archived under the day it was last written.
    pub fn open(dir: &Path, now: DateTime<Utc>) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let today = now.date_naive();
        if let Ok(day) = modified_day(&dir.join(CURRENT))
            && day < today
        {
            archive_current(dir, day)?;
        }
        prune(dir, today)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            file: open_current(dir)?,
            day: today,
        })
    }

    /// Appends `line` stamped with `at`, cycling the file first if `at` is on
    /// a later day than the current file.
    pub fn write_line(&mut self, at: DateTime<Utc>, line: &str) -> io::Result<()> {
        self.roll(at)?;
        // One write so a concurrent tailer never sees half a line.
        self.file
            .write_all(format!("{}\n", stamp_line(at, line)).as_bytes())
    }

    /// Cycles to a new file when `now` is past the current file's day.
    pub fn roll(&mut self, now: DateTime<Utc>) -> io::Result<()> {
        let today = now.date_naive();
        if today <= self.day {
            return Ok(());
        }
        archive_current(&self.dir, self.day)?;
        self.file = open_current(&self.dir)?;
        self.day = today;
        prune(&self.dir, today)
    }

    /// How long until the current file's day ends, so an idle server's log
    /// still cycles at midnight.
    pub fn until_next_day(&self, now: DateTime<Utc>) -> Duration {
        let midnight = (self.day + TimeDelta::days(1))
            .and_hms_opt(0, 0, 0)
            .expect("midnight is a valid time")
            .and_utc();
        // A second of slack so the wake-up lands after midnight, not before.
        (midnight - now).to_std().unwrap_or_default() + Duration::from_secs(1)
    }
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
    fn a_new_day_archives_the_previous_file() {
        let dir = temp_dir("roll");
        let mut log = ConsoleLog::open(&dir, at(14, 23)).unwrap();
        log.write_line(at(14, 23), "late").unwrap();
        log.write_line(at(15, 1), "early").unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("console-2026-10-14.log")).unwrap(),
            "2026-10-14T23:00:00.000Z late\n"
        );
        assert_eq!(
            fs::read_to_string(dir.join(CURRENT)).unwrap(),
            "2026-10-15T01:00:00.000Z early\n"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_stale_console_log_is_archived_under_its_last_write_day() {
        let dir = temp_dir("stale");
        fs::write(dir.join(CURRENT), "old line\n").unwrap();
        let today = Utc::now() + TimeDelta::days(3);
        let written: DateTime<Utc> = fs::metadata(dir.join(CURRENT))
            .unwrap()
            .modified()
            .unwrap()
            .into();

        ConsoleLog::open(&dir, today).unwrap();

        assert!(dir.join(archive_name(written.date_naive())).is_file());
        assert_eq!(fs::read_to_string(dir.join(CURRENT)).unwrap(), "");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn restarting_on_the_same_day_keeps_appending() {
        let dir = temp_dir("restart");
        let now = Utc::now();
        ConsoleLog::open(&dir, now)
            .unwrap()
            .write_line(now, "one")
            .unwrap();
        ConsoleLog::open(&dir, now)
            .unwrap()
            .write_line(now, "two")
            .unwrap();

        let text = fs::read_to_string(dir.join(CURRENT)).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.lines().next().unwrap().ends_with(" one"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn archives_past_retention_are_deleted() {
        let dir = temp_dir("prune");
        let today = day(30);
        let expired = today - TimeDelta::days(RETENTION_DAYS + 1);
        let kept = today - TimeDelta::days(RETENTION_DAYS);
        fs::write(dir.join(archive_name(expired)), "x").unwrap();
        fs::write(dir.join(archive_name(kept)), "x").unwrap();
        fs::write(dir.join("update.log"), "x").unwrap();

        prune(&dir, today).unwrap();

        assert!(!dir.join(archive_name(expired)).exists());
        assert!(dir.join(archive_name(kept)).exists());
        assert!(dir.join("update.log").exists());
        fs::remove_dir_all(dir).ok();
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

    #[test]
    fn idle_logs_wake_just_after_midnight() {
        let dir = temp_dir("idle");
        let log = ConsoleLog::open(&dir, at(14, 23)).unwrap();
        assert_eq!(
            log.until_next_day(at(14, 23)),
            Duration::from_secs(3600 + 1)
        );
        fs::remove_dir_all(dir).ok();
    }
}
