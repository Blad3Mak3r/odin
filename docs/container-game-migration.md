# Adding a game to the Podman runtime

This guide is for agents and contributors adding a game that runs in an
`odin-srv` Podman container. Every game does: Valheim, Rust, 7 Days to Die and
Palworld are already there, and there is no host-process mode. Read the code of
the one closest to your game before starting:

| If your game... | Copy |
|---|---|
| is a *generic* game (`GenericGameInstance`, native config document, no typed Odin settings) | `src/podman/seven_days_to_die.rs`, `src/podman/palworld.rs` and their `src/game/<game>.rs` |
| has typed Odin settings in SQLite (`RustInstance`, Valheim's `InstanceState`) | `src/podman/rust.rs` / `src/podman/valheim.rs` |

Read [AGENTS.md](../AGENTS.md) first: its *Game server configuration policy*
applies to every game. This guide only adds the container-specific part.

## 1. Mental model

The `ghcr.io/blad3mak3r/odin-srv:base` image serves native games, while
`:proton` extends it for Windows-only servers. They are built from
`containers/odin-srv/{base,proton}.Dockerfile`, contain Debian, their runtime
dependencies and a SteamCMD seed, but **no game payload or Odin binary**. The
host bind-mounts its current Odin binary into every game container.

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
  limits, host-shutdown handling, the dashboard health card and the SteamCMD
  log view are **already game-neutral**. A new game gets them by
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
drivers.

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
     instance's private runtime copy once it is installed, otherwise a template
     placed in the host install directory.
   - A graceful-stop function that takes **only paths** (see §4).
2. **Lifecycle: `src/podman/<game>.rs`.** `pub(super) struct <Game>;`
   implementing `ContainerGame` (`src/podman/lifecycle.rs`): `load`,
   `identity`, `instance_dir`, `ports` (`claimed_ports`), `prepare_layout`
   (create instance subdirs + runtime dir + socket dir, then
   `write_runtime_spec`; never create the game's document and never call
   `prepare_runtime`, which runs in the container), `extra_mounts` (usually
   empty). Add `pub mod <game>;` in `src/podman.rs`.
3. **`src/podman.rs`.** Add a `Launch::<Game>(<Game>Launch)` variant and its
   arms in `game`, `name`, `server_command` (calls `prepare_runtime` then
   `build_command_at`) and `request_graceful_stop`. The variant tag is
   `GameId::as_str()` converted by `rename_all = "snake_case"`; if the id is
   not valid snake_case (`7d2d`), add `#[serde(rename = "...")]`.
4. **`src/podman/dispatch.rs`.** Add the `for_container_game!` arm; the match is
   exhaustive, so the compiler points at the other places that name every game.
5. **Driver: `src/game/mod.rs`.** Register the driver, declare
   `runtime_image()` when the game needs Proton, and set
   `stop_timeout_secs()` if the game needs more than 30 s to save (the ceiling
   used so far is 120 s).
6. **Config documents: `src/game/config_documents.rs`.** Make `initialize`
   read the template through `template_path`. Add a path-based value reader
   (`xml_value`, `palworld_value`) if graceful stop needs a setting.
7. **Liveness call sites.** Anything that asks "is this running?" must use
   `crate::game::instances::is_running(paths, db, game, name)`; it asks the
   container. There is no host pid to check.
8. **Frontend.** Add game-specific controls and native configuration routes
   where the server contract needs them. The runtime health card, update button
   and SteamCMD log are game-neutral, but shared-install games require their
   game-scoped install and configuration controls.

Things that look necessary but are **not**: DB migrations (`instance_runtime_state`
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

## Shared installs

A game can let its instances reuse one copy of the game files instead of each
downloading a private one (ARK: Survival Ascended and 7 Days to Die do; the
others do not yet). 7 Days to Die is the minimal example: its server writes
everything under `-UserDataFolder`, so it needs the shared mount and nothing
else.
The game-neutral part lives in `src/podman/shared_install.rs`; a game opts in
with `shared_install: true` in its driver capabilities and supplies only what is
specific to it.

Game-neutral, nothing to add per game:

- `POST /games/{game}/install` runs the installer job, `GET` reports whether the
  server binary exists and the Steam build id. The installer is a short-lived
  `odin-<game>-install` container running `odin install-shared --game <id>` in
  the driver's runtime image; it refuses to run while a shared instance of the
  game is running.
- The `install_mode` of a new instance is accepted for every game that declares
  the capability, stored in `game_instance_install`, and an instance's own
  update is refused when it is shared.
- The dashboard shows the install card on the game's overview and the install
  mode selector in the create dialog from the capability alone.

Moving an existing isolated instance onto the shared files
(`POST /instances/{id}/shared-install`, `shared_install::migrate_to_shared`) is
opt-in per game through `shared_install_migration`, because it must not change
what the instance's settings mean. ARK does not opt in: its shared
configuration would be mounted over the instance's own INI files. A game opts
in once its per-instance state is entirely outside the install tree. The
migration needs a stopped instance, removes only its container (a stopped
container keeps its old mounts when started again), and either adopts the
instance's own game files as the shared installation when none exists, which
is a rename, or leaves them in place. They are deleted only when the request
asks (`delete_private_files`).

What the game provides in `src/podman/<game>.rs`:

- `ensure_ready_to_start` calls `shared_install::ensure_ready`.
- `prepare_layout` writes the runtime spec with
  `write_runtime_spec_with_install_dir(.., Some(shared_install::MOUNT_DIR))`
  when `game_instances::is_shared_install`.
- `extra_mounts` pushes `shared_install::mount` **first** and then mounts every
  per-instance path the server writes inside the install tree (saves, mods,
  identity directories) on top of it. A symlink created inside the install tree
  cannot do this job: the tree is shared, so the link target would be too.
  Create the mountpoints on the host before the first start; otherwise Podman
  creates them inside the shared tree as root.

## 5. Existing data

When a game's layout changes, existing instances keep working:

- Keep existing paths whenever possible so no files move. Where they must
  move, the move has to be safe to resume after a failed startup.
- Native file values keep precedence; only add values that are missing. Back
  up retired data instead of importing it over newer file edits.
- Never delete data automatically; leftovers of an earlier layout stay on disk
  and are inert.
- Instance creation must work offline with no game installed and no document
  written.

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
  `PermissionDenied`** (restricted sandboxes). Copy that guard. To make an
  instance count as running in a route-level test use
  `test_support::podman_reporting_running`.
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
  expected fields and leaves an operator document untouched; preparing the
  layout is idempotent and keeps existing data; stop sends the control request
  with the driver's `timeout_secs` and clears `desired_running`.
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
- Update `README.md` (Podman section text and the data layout) and
  `docs/auto-start-after-reboot.md` (list of container games).
- Branch `feature/<game>-podman-runtime`, incremental commits, PR text citing
  the official sources used for the contract, no AI authorship in commits or PR.
