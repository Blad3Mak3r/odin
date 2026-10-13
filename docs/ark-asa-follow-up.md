# ARK: Survival Ascended follow-up handoff

This document records the remaining work on the draft PR for ARK: Survival
Ascended. It is intentionally specific so another coding agent can resume it
without re-discovering the design decisions.

## Current state

- Branch: `feature/ark-asa`
- Draft PR: https://github.com/Blad3Mak3r/odin/pull/145
- Keep the PR as a draft unless the user explicitly asks to mark it ready.
- ARK uses the Proton runtime image. It supports isolated and shared installs,
  game-scoped shared INI documents, local-host clusters, and Source RCON
  shutdown.
- Native configuration has both a key editor and a CodeMirror source editor.
  Only declared game documents can be edited; source writes are atomic,
  limited to 1 MiB, validate ARK INI syntax, and cannot modify Odin-managed
  keys.
- Pulling an image never interrupts a running server. Stopped containers are
  recreated after a pull, and `lifecycle::start` also recreates a stopped
  container when its inspected image ID differs from the configured local
  image ID.

## Remaining implementation work

### 1. Complete runtime-image update monitoring

Manual image pulls and local-image application are implemented, but the
planned registry monitor is not.

Implement it as a small background worker under `src/web/`, following the
patterns in `src/web/uptime_scheduler.rs`:

1. Every six hours, and on an explicit dashboard request, check only the
   moving default references `ghcr.io/blad3mak3r/odin-srv:base` and
   `:proton`. Treat every other reference, including immutable tags and
   digests, as pinned and do not contact a registry.
2. Obtain an anonymous GHCR bearer token and issue a manifest `HEAD` request.
   Persist the `Docker-Content-Digest` and check timestamp in `cache`.
3. Extend `GET /settings/runtime-images` with the remote digest, last check,
   and update-available state. Add a `POST /settings/runtime-images/check`
   endpoint and dashboard action.
4. Add the `runtime_images.auto_pull` setting (default false) only if the
   scheduler also handles automatic pulls. Automatic/manual pulls must keep
   the current invariant: never stop a running container.
5. Add pure tests for moving-tag parsing, pinned-reference skipping, cached
   check TTL, digest comparison, and the stopped-container image-ID decision.

There was an uncommitted prototype for this worker when this handoff was
written; do not treat it as complete or add it to the PR without wiring it,
testing it, and validating error handling.

### 2. Real-server validation

The remaining verification needs an actual Linux host with Podman, SteamCMD,
Proton, and enough storage for ASA. Use a disposable data directory and a
port other than 7331, as documented in `AGENTS.md`.

Validate:

- A fresh isolated and shared ARK instance boot successfully.
- Shared instances have isolated `Saved`/`Mods`, shared INIs, and independent
  map/session/mod launch options.
- Cluster members can transfer through the local cluster directory.
- Source RCON `SaveWorld` then `DoExit` performs a graceful stop.
- SteamCMD's Windows platform handling, QueryPort behaviour, and Proton
  compatibility work with the current image.
- Existing Valheim, Rust, Palworld, and 7 Days to Die containers still start
  after the host-binary mount and image split.

### 3. Final PR audit

Before requesting review:

- Run `make check` and `npm --prefix web run build`.
- The sandbox can deny Unix-socket and runtime-layout tests. Record such
  failures separately from product failures; do not claim the full suite
  passed unless it actually did.
- Confirm the PR description still cites the ASA community contract and says
  that Wildcard does not publish an official Linux dedicated-server document.
- Verify `git status --short` is clean and the branch is pushed.

## Relevant files

- `src/podman/lifecycle.rs` — image-ID comparison and container recreation.
- `src/web/routes/settings.rs` — current image list and manual pull job.
- `web/src/pages/SettingsPage.tsx` — image management UI.
- `src/game/config_documents.rs` and `src/web/routes/config_files.rs` — native
  key/source editing and safety rules.
- `src/podman/ark_asa.rs` — shared-install/config/cluster mounts.
- `src/web/routes/ark_clusters.rs` — cluster membership lifecycle handling.
