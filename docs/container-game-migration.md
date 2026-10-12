# Migrating a game to the Podman runtime

This guide is for agents and contributors moving a game from "directly
supervised host process" to "runs in an `odin-srv` Podman container". Valheim,
Rust, 7 Days to Die and Palworld are already migrated; read the code of the one
closest to your game before starting:

| If your game... | Copy |
|---|---|
| is a *generic* game (`GenericGameInstance`, native config document, no typed Odin settings) | `src/podman/seven_days_to_die.rs`, `src/podman/palworld.rs` and their `src/game/<game>.rs` |
| has typed Odin settings in SQLite (`RustInstance`, Valheim's `InstanceState`) | `src/podman/rust.rs` / `src/podman/valheim.rs` |

Read [AGENTS.md](../AGENTS.md) first: its *Game server configuration policy*
applies to every migration. This guide only adds the container-specific part.

## 1. Mental model

One image (`ghcr.io/blad3mak3r/odin-srv:<odin version>`, built from
`containers/odin-srv/Dockerfile`) serves every game. It contains Debian, 32-bit
libraries, a SteamCMD seed and the `odin` binary, but **no game payload**.

```
host                                          container (host network)
<data>/runtimes/<game>/<uuid>/  ───────────▶  /srv/odin/runtime      private SteamCMD + game files
<data>/games/<game>/instances/<name>/ ─────▶  /srv/odin/instance     config, saves, mods, logs, backups
<runtime dir>/containers/<uuid>/ ──────────▶  /run/odin              control sockets
```

- `odin run --instance-id <uuid>` is the container's entrypoint. It reads
  `runtime.json` (the *runtime spec*), installs/updates the game with SteamCMD
  into `/srv/odin/runtime/<game>` on first start, then spawns the game server.
- The spec is written by the host (`write_runtime_spec`) and contains only the
  typed launch settings Odin must pass to the process. It is not a second game
  configuration format.
- Host networking means every game port is a host port and loopback services
  (REST, RCON, Telnet) stay reachable from the host at `127.0.0.1`.
- Everything that identifies the container is derived from `GameId::as_str()`:
  the container name `odin-<game>-<uuid>`, the `io.odin.game` label, the
  `runtimes/<game>/` directory, the install dir `/srv/odin/runtime/<game>` and
  the serde tag of the `Launch` variant. They must all agree (see §3).
- Liveness, restart-after-crash (`auto_restart` + `desired_running`), resource
  limits, host-shutdown handling, the dashboard badge/banner/health card and
  the SteamCMD log view are **already game-neutral**. A new game gets them by
  being registered, not by new code.

## 2. Step 0: establish the contract from primary sources

Before writing code, verify (and cite in the PR description):

1. The publisher's dedicated-server documentation: Steam app id, launch
   executable, **documented** arguments, ports, config file path and format.
2. The official image or launch script, if one exists.
3. The templates shipped by the installed server (download it once and look).
4. Match the documented server version. Community reports may explain a
   compatibility gap but never replace an official source.

Then classify every setting:

- Odin must pass it to the process (listener ports, RCON transport arguments)
  → typed field in the driver / SQLite.
- Everything else is the game's own and stays in its native file. No JSON
  mirror, no Odin-specific schema, no copy in SQLite.
- Data paths, log paths, identity and headless flags are derived by the driver.

If the game has no native document, represent its documented launch arguments
as typed, validated fields and omit new optional ones by default.

## 3. Code checklist

Work through these in order. File names are those used by the 7d2d/Palworld
migrations.

1. **Launch contract: `src/game/<game>.rs`.**
   - A `<Game>Launch` struct (`Serialize`/`Deserialize`) with exactly what the
     process needs from Odin (`name`, ports...). Implement `From`/`TryFrom<&GenericGameInstance>`.
   - `build_command_at(install_dir, [instance_dir,] console_log, &launch)`:
     the **single** definition of the full command (program, cwd, ordered
     arguments, stdout+stderr appended to `console_log`, `process_group(0)`).
     Return an error naming "not installed" when the binary is missing.
   - `prepare_runtime(install_dir, instance_dir)`: *container-side* work run
     before every launch (see §4). It must be idempotent.
   - `template_path(paths, identity)` if the game ships a config template: the
     instance's private runtime copy first, the shared install as fallback (an
     instance that is not migrated yet still uses the shared install).
   - A graceful-stop function that takes **only paths** (see §4).
2. **Lifecycle: `src/podman/<game>.rs`.** `pub(super) struct <Game>;`
   implementing `ContainerGame` (`src/podman/lifecycle.rs`): `load`,
   `identity`, `instance_dir`, `ports` (`claimed_ports`), `prepare_layout`
   (create instance subdirs + runtime dir + socket dir, then
   `write_runtime_spec`; never create the game's document and never call
   `prepare_runtime`, which runs in the container), `extra_mounts` (usually
   empty), `running_directly` (`is_running_directly()`), `stop_directly`
   (the old host stop, used for instances not migrated yet). Add
   `pub mod <game>;` in `src/podman.rs`.
3. **`src/podman.rs`.** Add a `Launch::<Game>(<Game>Launch)` variant and its
   arms in `game`, `name`, `server_command` (calls `prepare_runtime` then
   `build_command_at`) and `request_graceful_stop`. The variant tag is
   `GameId::as_str()` converted by `rename_all = "snake_case"`; if the id is
   not valid snake_case (`7d2d`), add `#[serde(rename = "...")]`.
4. **`src/podman/dispatch.rs`.** Add the `for_container_game!` arm and remove
   the game from `otherwise`. The test `container_games_match_the_capability_flag`
   fails if this and the driver disagree.
5. **Driver: `src/game/mod.rs`.** `container_runtime: true`, and
   `stop_timeout_secs()` if the game needs more than 30 s to save (the ceiling
   used so far is 120 s).
6. **Retire the host launch: `src/game/generic.rs`.** `prepare_start` bails with
   "runs in Podman; start it through its container runtime", `build_command`
   becomes `unreachable!` for the game, and per-game helpers that only served
   the host launch are removed. **Keep** the host stop (`generic::stop`): it is
   `stop_directly` for instances that have not been migrated yet. Move the
   command tests to the new `build_command_at`.
7. **Config documents: `src/game/config_documents.rs`.** Make `initialize`
   read the template through `template_path`. Add a path-based value reader
   (`xml_value`, `palworld_value`) if graceful stop needs a setting.
8. **Liveness call sites.** Anything that asks "is this running?" must use
   `crate::game::instances::is_running(paths, db, game, name)`. A bare
   `GenericGameInstance::is_running_directly()` is a bug for a containerized
   game (it only sees host pids). `grep -rn is_running_directly src` and review
   every hit. Palworld's pre-backup save flush was one.
9. **Frontend.** nothing to do unless the game has its own tabs; the badge, migration banner, runtime health card, update button and
   SteamCMD log are driven by `capabilities.container_runtime` and
   `execution`/`migration_required` from the API.

Things that look necessary but are **not**: DB migrations (`podman_instances`
is game-neutral), `containers/` changes (unless the server needs an apt
package the image lacks), `CHANGELOG.md` (written by release commits),
`web/src` game-id branches, new CLI commands.

## 4. Recurring design problems and the solution used

**The server writes data relative to its install tree.** The install tree is
the container's private volume, but saves and the native config must live in
the instance directory (backups, the save browser and the config editor read
there, and existing worlds are already there). Create a **symlink inside the
container** from the game's data path to the instance directory (Rust:
`install/server/<identity>` → `/srv/odin/instance/server`; Palworld:
`install/Pal/Saved` → `/srv/odin/instance/runtime/Pal/Saved`) in
`prepare_runtime`. Do not use a bind mount: Podman would create the mountpoint
inside the volume as root and the unprivileged SteamCMD could no longer write
there. Refuse to replace anything that is not a symlink.

**Seeding the native document.** The template comes from the game's private
install, which only exists after the first start, so the container seeds it in
`prepare_runtime` and `initialize` seeds it from the same template when
present. Always "create if missing"; never overwrite an operator file. Edits
go through the advanced editor (existing keys only, atomic writes, server
stopped).

**Graceful stop.** `Launch::request_graceful_stop(instance_dir)` runs
*inside* the container, which has no `Paths` and no database. Whatever the
game needs to stop cleanly (RCON password, REST port, Telnet password) must be
readable from files under `instance_dir`: add a path-based reader instead of
passing secrets in the spec. Use the game's own save-and-exit command (Rust
RCON `quit`, 7d2d console `shutdown`, Palworld REST `shutdown`); if it is
unavailable or disabled return `false` and the runtime signals the process.
Wrap blocking clients in `spawn_blocking` and keep the whole attempt under
`CONSOLE_STOP_TIMEOUT`. Games that save on SIGINT (Valheim) return `false`.

**Readiness.** In a container the dashboard reports ready when the game
process has started (the events socket is not used). Do not invent a parser for
containers unless there is a concrete need.

**Ports.** `claimed_ports` decides what conflicts. Ports configured only inside
the game's own file (for example Palworld's REST port and `PublicPort`) are
not claimed; this is a known gap shared by all native-config games, so document
it rather than copying game settings into SQLite.

**Steam client library.** SteamCMD creates `$HOME/.steam/sdk64` itself when it
runs with `HOME=/srv/odin/runtime`. Rust additionally maintains the link before
every start because Steamworks needs it; only add that for a game that is
shown to need it (`SteamCmd::ensure_sdk64_client_at`).

## 5. Migrating existing instances

`lifecycle::migrate::<G>` is generic and **resumable**: it takes the lifecycle
lock, refuses a server still running on the host, runs `prepare_layout`
(idempotent) and writes the `podman_instances` marker last. A failure leaves
the instance unmigrated and a retry safe. For your game:

- Keep existing paths whenever possible so no files move. Where they must
  move, the move has to be safe to resume after a failed startup.
- Native file values keep precedence; only add values that are missing. Back
  up retired data instead of importing it over newer file edits.
- Never copy, modify or delete the shared host installation. The container
  downloads its own copy; the shared one becomes unused.
- Files created by the old host launch stay on disk (for example Palworld's
  hardlink tree under `<instance>/runtime/`). They are inert; do not delete
  data automatically.
- New instances are born containerized (`mark_new_instance` from
  `instances::create`), and instance creation must work offline with no game
  installed and no document written.

## 6. Easy-to-miss details

- Guards that used to live in the DB/config layers (`is_running` in
  `update_generic_config`, `config_documents::initialize/apply`) now live in the
  routes (`config_files.rs::ensure_stopped`, `games.rs`) and are container-aware.
  Do not reintroduce a host-pid check in lower layers.
- `instances::delete` calls `dispatch::remove` (container + runtime volume)
  before removing the instance directory; rename calls `dispatch::sync_spec`.
- The container-side tests of `ports.rs`, `web/mod.rs` and `config_files.rs`
  already cover shared behaviour; add one case for the new game only where the
  behaviour depends on it.
- All Podman API tests bind a fake Unix socket and **return early on
  `PermissionDenied`** (restricted sandboxes). Copy that guard.
- Writing everything in English applies to code, comments, commits and PRs.

## 7. Tests to add

Assert the complete behaviour, not fragments. Reuse `src/podman/test_support.rs`
(`temp_paths`, `write_podman_config`, `fake_podman`, `fake_runtime`).

- `src/game/<game>.rs`: the full command (program, cwd, every argument in
  order), error when not installed; `prepare_runtime` creates the link, is
  idempotent, refuses a real directory, never overwrites an existing native
  file or existing saves, seeds a missing document, fails for a missing template
  only when a document is needed; `template_path` prefers the private copy.
- `src/podman/<game>.rs`: start creates `odin-<game>-<uuid>` with the
  `io.odin.game` label, host netns and the three mounts, writes a spec with the
  expected fields and leaves an operator document untouched; migration prepares
  the layout before marking, is idempotent, keeps existing data, and refuses a
  host-running server; stop sends the control request with the driver's
  `timeout_secs` and clears `desired_running`.
- `src/podman.rs`: the runtime spec round-trips under the game's id (guards the
  serde tag against `GameId::as_str`).
- `src/podman/runtime.rs`: graceful stop uses the game's own command and sends
  no signal; falls back to SIGINT when it is unreachable; signals directly when
  it is disabled.
- `src/game/ports.rs`: a running container reserves its ports.
- `src/web/mod.rs`: a crashed container is restarted with `auto_restart`, and
  not after an operator stop.

## 8. Verification and PR

- Targeted first: `cargo test <game>`, `cargo test podman::`,
  `cargo test dispatch`. A migration crosses subsystems, so finish with
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and a full
  `cargo test` (or `make check`).
- Manual run only with the recipe in AGENTS.md (`sudo env ODIN_DATA_DIR=<scratch>
  ./target/debug/odin serve --port 7332`); never port 7331. A real container
  start needs Podman 5 and downloads the game; say explicitly when you could
  not run it.
- Update `README.md` (Podman section title/text and the data layout) and
  `docs/auto-start-after-reboot.md` (list of container games).
- Branch `feature/<game>-podman-runtime`, incremental commits, PR text citing
  the official sources used for the contract, no AI authorship in commits or PR.
