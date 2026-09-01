# Changelog

All notable changes to Odin are documented in this file.

## [Unreleased]

## [0.10.2] - 2026-09-01

### What's Changed
- Enlarge sidebar theme toggle by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/71
- Stack installed mod card controls below the title by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/72
- Fix checked checkbox contrast in dark mode by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/73
- Ignore generated dashboard build output by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/74


**Full Changelog**: https://github.com/Blad3Mak3r/odin/compare/v0.10.1...v0.10.2

## [0.10.1] - 2026-09-01

### What's Changed
- Move Odin version into the dashboard wordmark by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/69
- Fix dashboard oxlint react warnings by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/70


**Full Changelog**: https://github.com/Blad3Mak3r/odin/compare/v0.10.0...v0.10.1

## [0.10.0] - 2026-09-01

### What's Changed
- Add BepInEx updates to the web dashboard by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/68


**Full Changelog**: https://github.com/Blad3Mak3r/odin/compare/v0.9.0...v0.10.0

## [0.9.0] - 2026-08-31

### What's Changed
- Declare SteamCMD/Valheim runtime library deps in .deb/.rpm by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/64
- Monitor Valheim server updates in odin serve by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/65
- Version mods per instance by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/66
- Fix player tracking for current Valheim logs by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/67


**Full Changelog**: https://github.com/Blad3Mak3r/odin/compare/v0.8.4...v0.9.0

### Added

- `odin serve` now checks Steam for Valheim server updates every 15 minutes and emits a webhook event once for each new build.
- Mods are stored by exact version, so instances can update independently, pin a version, roll back to a cached version, and prune unused versions.

## [0.8.4] - 2026-08-31

### What's Changed
- Block mod install/remove/toggle while an instance is running by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/62
- Add production container image by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/63


**Full Changelog**: https://github.com/Blad3Mak3r/odin/compare/v0.8.3...v0.8.4

## [0.8.3] - 2026-08-31

### What's Changed
- Improve instance lifecycle transition atomicity by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/61


**Full Changelog**: https://github.com/Blad3Mak3r/odin/compare/v0.8.2...v0.8.3

## [0.8.2] - 2026-08-31

### What's Changed
- Add modpack download feature for enabled mods by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/58
- Fix flaky late_subscriber_gets_buffered_log test by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/59
- Add pull request quality checks by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/60


**Full Changelog**: https://github.com/Blad3Mak3r/odin/compare/v0.8.1...v0.8.2

## [0.8.1] - 2026-08-30

### What's Changed
- feat: add automated changelog and dashboard page by @Blad3Mak3r in https://github.com/Blad3Mak3r/odin/pull/57


**Full Changelog**: https://github.com/Blad3Mak3r/odin/compare/v0.8.0...v0.8.1

## [0.8.0] - 2026-08-30

### Added

- Added S3-compatible remote storage for instance backups.
- Added dashboard controls for configuring and managing remote backups.
