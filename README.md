# Odin

**Odin** is a self-hosted web service for orchestrating [Valheim](https://www.valheimgame.com/)
dedicated servers on Linux. In Norse mythology, Odin is the All-Father who
watches over the nine realms; this Odin watches over your Valheim realms —
installing and updating the game server, running and supervising multiple
server instances, managing their mods, config, backups, and access lists —
all from one binary and one web dashboard, so you don't have to babysit
background processes and SteamCMD invocations by hand.

Odin ships as a single binary with an embedded web dashboard
(`odin serve`) — that dashboard is the primary, actively developed way to
operate it. A legacy CLI is still bundled for scripting and one-off use,
but new capabilities land in the web API and dashboard, not the CLI. See
[Project direction](#project-direction).

## Why Odin exists

Running a Valheim dedicated server on Linux usually means stitching together
several separate tools yourself: SteamCMD to install and update the server,
some way to keep it running after you log out, manual `BepInEx`/Thunderstore
downloads if you want mods, and a pile of shell scripts or ad-hoc notes to
remember port numbers, world names, and passwords across restarts — and it
only gets messier once you're running more than one server. Odin folds all
of that into one small binary that runs as a background service and exposes
a single web dashboard for orchestrating a whole fleet of servers side by
side: creating and controlling instances, installing mods, editing config
and access lists, watching live consoles and logs, and taking backups —
all from a browser, on one host or several.

## Features

- **A web dashboard as the primary interface.** `odin serve` starts a
  single self-contained HTTP server — the built frontend is embedded in
  the binary, no separate process or database — for orchestrating every
  instance from a browser: create and control instances, edit config and
  access lists, search and install mods, watch a live console and logs, and
  see host/instance resource usage. See [Web dashboard](#web-dashboard)
  below.
- **One binary, no runtime dependencies** beyond (transparently managed)
  SteamCMD and a handful of OS shared libraries — no Python, no Docker, no
  terminal multiplexer required.
- **Multiple named instances.** Orchestrate several independent Valheim
  servers on one host, each with its own world, port, password,
  visibility, and mod set, sharing a single downloaded copy of the game
  binaries.
- **Detached by default, and restart-proof.** Every instance runs as its
  own directly-supervised background process, so it keeps running after you
  disconnect — and after `odin serve` itself is restarted or upgraded.
  Watch it live from the dashboard's console view, or with `odin logs
  --follow` via the legacy CLI.
- **Mod support out of the box.** Bootstraps
  [BepInEx](https://github.com/BepInEx/BepInEx) automatically and installs
  mods straight from the [Thunderstore](https://thunderstore.io/c/valheim)
  API by name — no manual unzipping into the right folder. Mods download
  once per version into a shared global store and get symlinked into every
  instance that wants them. Each instance can use and pin its own exact
  version without duplicating downloads or changing another server.
- **Ready-to-share connect info.** The dashboard (and `odin status`) shows
  each instance's public address and password alongside its live state, so
  getting a friend into your world doesn't mean a round-trip through raw
  config and "what's my IP".
- **Backups with a safety net.** Snapshot a world's save files to a zip
  archive at any time; restoring always takes a fresh snapshot of the
  current state first, so a restore is never a one-way, unrecoverable
  action. Per-instance remote storage can send new backups to AWS S3 or
  Cloudflare R2; Odin removes the temporary local zip only after the upload
  succeeds and keeps it locally if the upload fails.
- **State you can trust.** An instance's "running" status is always derived
  live from the OS process itself (its pid, cross-checked against its own
  start time so a reused pid never lies to you), never from a flag that can
  silently go stale after a crash or a reboot.
- **A `doctor` command** that checks your environment (SteamCMD, the game
  install, disk permissions, network reachability) so a broken setup fails
  with a clear diagnosis instead of a cryptic error three commands later.
- **Optional system-wide install.** The `.deb`/`.rpm` packages (`make
  install`) set up a dedicated `odin` system account, `/etc/odin` +
  `/var/lib/odin`, and a system `odin.service` running the dashboard as a
  long-lived orchestration service — run ad-hoc instance commands as that
  account with `sudo -u odin odin <command>` (`nologin` only blocks
  interactive login, not `sudo -u` exec), or manage everything from the
  dashboard day to day.

## Project direction

The web dashboard (`odin serve`) is where this project is headed: it's the
primary, and increasingly the *only*, supported way to operate Odin. New
user-facing capabilities are built as web API routes plus dashboard UI, not
as new CLI subcommands. The CLI documented below still works and is kept
around for scripting and quick one-off commands, but it's being treated as
legacy — don't expect new features to show up there first.

## Requirements

- **Linux only** (x86_64). Odin spawns the Valheim dedicated server
  directly and shells out to SteamCMD's Linux distribution; there is no
  Windows or macOS support.
- **SteamCMD is handled for you.** Odin downloads and installs it
  automatically on first `odin install`; you don't need to have it set up
  beforehand.
- **Outbound network access** to Steam's content servers (to install/update
  the game) and to `thunderstore.io` (to install/update mods), plus the
  configured AWS S3 or Cloudflare R2 endpoint when remote backups are enabled.
- **A handful of OS shared libraries.** SteamCMD's Linux distribution ships
  a 32-bit binary (needs `lib32gcc-s1`/`lib32gcc1` + `lib32stdc++6` on
  Debian/Ubuntu, or `glibc.i686` + `libstdc++.i686` on Fedora/RHEL), and
  Valheim's Unity engine loads ALSA/PulseAudio for its audio backend even
  headless (`libasound2`/`libasound2t64` + `libpulse0` on Debian/Ubuntu, or
  `alsa-lib` + `pulseaudio-libs` on Fedora/RHEL). The `.deb`/`.rpm`
  packages declare these as dependencies and apt/dnf install them for you;
  this only matters if you're building/installing from source on a
  minimal system.
- **[Node.js](https://nodejs.org/) is only needed to build the web
  dashboard's frontend** (`make web-build`, run automatically by `make
  build`/`release`). The compiled `odin` binary itself has no Node.js or
  npm runtime dependency — the dashboard is embedded static assets, not a
  separate process.

## Installation

**Recommended** — download and install the latest release with one
command. This detects your distro (Debian/Ubuntu or Fedora/RHEL-family,
x86_64 only), pulls the matching `.deb`/`.rpm` from the [latest GitHub
release](https://github.com/Blad3Mak3r/odin/releases/latest), and installs
it via `apt`/`dnf`. No Rust toolchain needed. Like any `curl | sh` install,
review [`install.sh`](install.sh) first if you want to see exactly what it
does before running it:

```sh
curl -sSL https://raw.githubusercontent.com/Blad3Mak3r/odin/main/install.sh | sh
```

This installs the same system package described below: a dedicated `odin`
system user, `/etc/odin` and `/var/lib/odin`, and a system `odin.service`
systemd unit running the dashboard as that user (see [Running as a
systemd service](#running-as-a-systemd-service)).

Alternatively, grab the `.deb`/`.rpm` yourself from the [Releases
page](https://github.com/Blad3Mak3r/odin/releases) and install it with
`apt install ./odin-server_*.deb` or `dnf install ./odin-server-*.rpm`.

### Container

Release images are published for `linux/amd64` at
`ghcr.io/blad3mak3r/odin`. Pin a version in production; `latest` is useful
for testing but changes whenever a new release is published:

```sh
ODIN_IMAGE=ghcr.io/blad3mak3r/odin:x.y.z
docker pull "$ODIN_IMAGE"
docker volume create odin-data
docker run -d \
  --name odin \
  --restart unless-stopped \
  --network host \
  --stop-timeout 60 \
  --tmpfs /run/odin:rw,nosuid,nodev,noexec,size=16m,mode=0750,uid=10001,gid=10001 \
  --mount source=odin-data,target=/var/lib/odin \
  "$ODIN_IMAGE"
```

Open `http://127.0.0.1:7331` on the host. The image deliberately binds the
dashboard to loopback by default because it has no authentication. Use an
SSH tunnel or an authenticating reverse proxy if it must be reachable from
another machine.

Host networking (`--network host`; `network_mode: host` in Compose or
`Network=host` in a Quadlet) is the recommended production setup on Linux.
Each Valheim instance uses its configured UDP port plus the next two ports,
and Odin assigns a new three-port block to every additional instance. Host
networking lets that dynamic allocation work without pre-publishing a large
UDP range or recreating the container whenever another instance is added.
Port publishing with `-p` is ignored while host networking is enabled.

Bridge networking is still useful for dashboard-only testing. Bind the
dashboard to all container interfaces and explicitly publish both its TCP
port and enough UDP ports for every instance you intend to run:

```sh
docker run --rm \
  --publish 127.0.0.1:7331:7331 \
  --publish 2456-2470:2456-2470/udp \
  --mount source=odin-data,target=/var/lib/odin \
  ghcr.io/blad3mak3r/odin:latest \
  serve --bind 0.0.0.0 --port 7331
```

The container filesystem contract is intentionally small:

| Path | Persistence | Purpose |
|---|---|---|
| `/var/lib/odin` | Required volume | Database, SteamCMD, shared Valheim install, worlds, backups, mods, and logs. |
| `/etc/odin/config.toml` | Image default; optional read-only bind mount | Global configuration. Its default data directory is `/var/lib/odin`. |
| `/run/odin` | Ephemeral `tmpfs` | Per-instance supervisor sockets and pidfiles. |

Keep `/var/lib/odin` as one volume: Odin's shared install and mod store use
links into per-instance directories, and the SQLite database coordinates all
of that state. The image runs as the fixed unprivileged user and group
`10001:10001`; a host bind mount must be writable by that identity. The
binary and configuration remain owned by root inside the image.

`docker stop odin` asks every running instance to save and stop concurrently
before the container exits. The 60-second stop timeout above gives Valheim's
30-second graceful shutdown window enough room. To upgrade Odin, pull the new
version and recreate the container with the same `odin-data` volume; do not
install packages or replace the binary inside a running container.

### Build from source

Only needed if you want a version other than the latest release, or your
distro isn't Debian/Fedora-family. Requires
[Rust](https://www.rust-lang.org/tools/install) (2024 edition) and the
included `Makefile`.

**System-wide** — builds a `.deb` or `.rpm` (whichever fits the host
distro) and installs it via the system package manager, same result as
the one-line install above. Needs `sudo`, and requires `cargo install
cargo-deb` or `cargo install cargo-generate-rpm` beforehand, matching the
host distro:

```sh
git clone git@github.com:Blad3Mak3r/odin.git
cd odin
make install    # builds + installs a system package (Debian/Fedora-family only)
```

**Per-user** — installs just the binary to `~/.local/bin` (or `PREFIX`),
with no system user, no service, and the original per-user XDG data/config
paths (see [Configuration](#configuration)):

```sh
make install-user                       # installs to ~/.local/bin/odin by default
make install-user PREFIX=/usr/local     # or override the prefix
```

Make sure the install directory is on your `PATH`. See [`make
help`](#development) for every available target.

## Quick start

```sh
# Start the orchestration service (binds 127.0.0.1:7331 by default)
odin serve
```

Then open `http://127.0.0.1:7331` in a browser: install/update the game
server, create and start named instances, search and install mods, edit
config and access lists, watch a live console, and take backups — all from
the dashboard. See [Web dashboard](#web-dashboard) for details.

For scripting or quick one-off changes, the legacy CLI covers the same
ground:

```sh
# Install SteamCMD and the Valheim dedicated server (safe to re-run to update later)
odin install

# Create and start a server named "my-server" (always runs detached in the background)
odin start my-server

# Check on it
odin status
odin logs my-server --follow

# Send it a console command without attaching to anything
odin exec my-server "save"

# Add a mod and restart to load it
odin mods search my-server valheim-plus
odin restart my-server

# Snapshot the world, then stop the server
odin backup my-server
odin stop my-server

# Rename it, or tear it down entirely when you're done with it
odin rename my-server my-old-server
odin delete my-old-server
```

## Command reference

> **Note:** the CLI is legacy. It remains fully functional for scripting
> and one-off use, but new capabilities are added to the [web
> dashboard](#web-dashboard) only — see [Project
> direction](#project-direction).

Server names are positional arguments (never a `-n`/`--name` flag) and must
be **DNS-friendly**: lowercase letters, digits, and hyphens only, and they
can't start or end with a hyphen (e.g. `my-server`, not `My Server`).

### Server binaries

| Command | Description |
|---|---|
| `odin install` | Install SteamCMD if missing, then install/update the Valheim dedicated server. Refuses to run while any instance is currently running (updating binaries under a live server is unsafe), and safe to re-run any time to pick up a new game version. |

### Instance lifecycle

| Command | Description |
|---|---|
| `odin create <server-name>` | Create a new instance (auto-assigning a free port and a random password) without starting it. Fails if the name is already taken — use this when you want to `config set` a world/port/password before the first start. |
| `odin start <server-name>` | Create the instance if it doesn't exist yet (auto-assigning a free port and a random password) and start it, always detached as its own supervised background process. |
| `odin stop <server-name>` | Gracefully stop a running instance (sends `SIGINT` for a clean world save, then `SIGKILL`s it if it doesn't exit in time). |
| `odin restart <server-name>` | Stop the instance if it's running, then start it again. Useful after installing mods or changing config. |
| `odin rename <old-name> <new-name>` | Rename an instance. Must be stopped first. Only its identity changes — the world name and save files are left untouched. |
| `odin delete <server-name> [-y\|--yes] [--keep-backups]` | Permanently delete an instance. Must be stopped first, and asks for confirmation unless `-y` is given. `--keep-backups` deletes everything except the `backups/` directory. |
| `odin status` | List every known instance with its live status, address (public IP:port), world, uptime, mod count, and password — everything needed to hand a friend a join string. |
| `odin console <server-name>` | No longer attaches an interactive terminal — prints a pointer to `odin logs --follow`, `odin exec`, or the web dashboard instead. |
| `odin logs <server-name> [-f\|--follow] [-n\|--lines N]` | Print (and optionally follow) the instance's captured console output. Default 50 lines. |
| `odin exec <server-name> <command>` | Send a line of input to a running instance's console without attaching — handy for scripting. |

### Configuration

| Command | Description |
|---|---|
| `odin config <server-name> get` | Print the instance's world name, port, password, and public/private visibility. |
| `odin config <server-name> set [--world W] [--port P] [--password PW] [--public true\|false]` | Update one or more of those fields. Changes apply on the instance's next `odin restart`. |

### Backups

| Command | Description |
|---|---|
| `odin backup <server-name>` | Zip the instance's save files into a timestamped archive. If remote backup storage was configured in the dashboard, upload it there and remove the local zip after a successful upload. |
| `odin restore <server-name> [backup-id]` | With no id, lists available backups. With an id, restores it — the instance must be stopped first, and Odin always takes a fresh backup of the current state before overwriting it. |

### Mods

| Command | Description |
|---|---|
| `odin mods search <server-name> <query> [-l\|--list]` | Search the Thunderstore package index by name or author, ranked by relevance (name match beats owner match, ties broken by downloads), showing whether each result is already installed on `<server-name>`. Interactive by default — prompts for a result number to install afterward; pass `-l`/`--list` to just list. |
| `odin mods add <server-name> <mod-id>` | Install a mod by its Thunderstore id (`namespace-name` or `namespace-name-version`). Bootstraps BepInEx into the instance automatically on first use. Downloads into the shared global mod store only if it isn't already there. |
| `odin mods update <server-name>` | Update an instance's unpinned mods to their latest available versions. Other instances keep their exact versions. |
| `odin mods list <server-name>` | List installed mods, their versions, and whether each is currently enabled (reads local state only, no network call). |
| `odin mods manage <server-name>` | Interactively toggle which installed mods are enabled via a checkbox list (space to toggle, enter to confirm, esc to cancel). Doesn't install new mods — use `odin mods search` for that. |
| `odin mods enable <server-name> <mod-id>` | Re-enable a previously disabled mod — relinks it from the global store, no reinstall needed. |
| `odin mods disable <server-name> <mod-id>` | Disable a mod without uninstalling it, so BepInEx stops loading it for this instance. |
| `odin mods remove <server-name> <mod-id>` | Uninstall a mod from this instance (the shared download stays in the global store for other instances still using it). |

### Diagnostics

| Command | Description |
|---|---|
| `odin doctor` | Check that SteamCMD is available, the game is installed, the data directory is writable, and Thunderstore/Steam are reachable. Exits non-zero only on a critical failure. |

### Shell completions

| Command | Description |
|---|---|
| `odin completions <shell>` | Print a completion script for `bash`, `zsh`, `fish`, `elvish`, or `powershell` to stdout — e.g. `odin completions zsh > ~/.zfunc/_odin`. |

### Web dashboard

| Command | Description |
|---|---|
| `odin serve [--bind ADDR] [--port PORT]` | Start the web dashboard (JSON API + embedded frontend). Defaults to `127.0.0.1:7331`. No authentication — see [Web dashboard](#web-dashboard). |

## Web dashboard

`odin serve` starts a JSON API plus the built frontend from one binary —
this is the primary way to operate Odin:

```sh
odin serve                            # binds 127.0.0.1:7331 by default
odin serve --bind 0.0.0.0 --port 8080  # or pick your own address/port
```

It covers dependency status, instance
create/start/stop/restart/rename/delete, per-instance config, mod
search/install/enable via Thunderstore, a live console and log tail, and
editing `adminlist.txt`/`bannedlist.txt`/`permittedlist.txt` — plus live
host and per-instance CPU/RAM usage that isn't exposed by the CLI at all.
Each instance's Backups tab also configures automatic backups and optional
AWS S3 or Cloudflare R2 storage. Remote backups remain listed and can be
restored or deleted from Odin; restores download a temporary zip and remove
it again when the operation finishes.

**There is no authentication.** `odin serve` binds to `127.0.0.1` by
default for exactly this reason; if you want to reach it from another
machine, put it behind an SSH tunnel (`ssh -L 8080:localhost:7331
your-host`) or your own authenticating reverse proxy rather than binding it
directly to a public address.

The frontend (`web/`, a Vite + React + TypeScript + Tailwind + shadcn/ui
project) is embedded into the binary at compile time, so building it is a
separate step from `cargo build` — see [Development](#development).

### Running as a systemd service

A system-wide install (via `make install`/the `.deb`/`.rpm` package)
already ships a system `odin.service`, running `odin serve --bind
127.0.0.1 --port 7331` as the dedicated `odin` user and enabled (but not
auto-started) by the package's postinst:

```sh
sudo systemctl start odin.service    # start now
sudo systemctl status odin.service   # check it's running
sudo journalctl -u odin.service -f   # follow logs
sudo systemctl edit odin.service     # override --bind/--port via a drop-in
```

A per-user install (`make install-user`, or a plain `cargo build`) has no
system service; run `odin serve` directly, or manage it yourself under
your own `systemctl --user`/init setup.

## Configuration

Odin stores everything under a single **data directory**. Which mode it
runs in is detected automatically from whether `/etc/odin/config.toml`
exists (it does after a system-wide `.deb`/`.rpm` install, and does not
after `make install-user`/a plain source build):

- **System mode** (`/etc/odin/config.toml` present): config at
  `/etc/odin/config.toml`, data at `/var/lib/odin` by default.
- **Per-user mode** (no system config file): config at
  `~/.config/odin/config.toml`, data at `~/.local/share/odin` by default.

In both modes, the data dir is resolved in this order:

1. `data_dir` in the mode's config file
2. the `ODIN_DATA_DIR` environment variable
3. the mode's default above

### Data layout

```
<data_dir>/
  steamcmd/                     # SteamCMD installation
  install/valheim/               # shared Valheim dedicated server binaries — every instance
                                  # symlinks to this, so `odin install` updates them all at once
  mods/<mod-id>/<version>/       # immutable shared payload — one download per exact version
  servers/<name>/
    state.json                   # instance metadata: port, world, password, visibility, mods, timestamps
    server -> ../../install/valheim
    saves/                       # the world's save files
    backups/<id>.zip              # local snapshots; remote uploads are removed after success
    logs/console.log              # captured console output, tailed by `odin logs`
    console.in                    # named pipe carrying console input (`odin exec`/dashboard)
    BepInEx/plugins/<mod-id> -> ../../../../mods/<mod-id>/<version>  # exact enabled version
  cache/thunderstore-index.json  # cached Thunderstore package index (1 hour TTL)
```

Each instance's game binaries are a symlink into one shared, SteamCMD-managed
install — so every instance always runs the same game version, and updating
is a single `odin install` rather than one download per server. Mods work
the same way, but versioned: `odin mods add` downloads each `(mod, version)`
once into the shared `mods/` store, and every enabled instance symlinks to
its exact version. Updating one instance creates or reuses the newer payload
and only repoints that instance; pinned instances remain unchanged. The
dashboard can switch back to any cached version and prune versions no longer
used by an instance.

On the first start after upgrading, Odin automatically moves the legacy
single-version mod store into this layout and repoints every existing instance;
no migration command or dashboard action is required.

An instance's **running/stopped status is never trusted from a stored
flag** — it's always derived on demand by checking whether its recorded
process id is still alive, cross-checked against that process's own start
time so a reused pid can never lie to you. That way `odin status` can never
be wrong after a crash, a manual `kill -9`, or a host reboot.

## Development

```sh
make build       # debug build (builds the dashboard frontend first)
make release      # optimized release build (ditto)
make test         # run the test suite
make lint          # clippy, denying warnings
make fmt           # format the codebase
make fmt-check     # check formatting without modifying files
make check         # fmt-check + lint + test — run before committing
make deb            # build a .deb package (needs `cargo install cargo-deb`)
make rpm             # build an .rpm package (needs `cargo install cargo-generate-rpm`)
make install          # build + install a system .deb/.rpm package (needs sudo)
make install-user      # install the release binary to ~/.local/bin instead (PREFIX=... to override)
make uninstall          # remove the system package (needs sudo)
make uninstall-user      # remove the binary installed by make install-user
make clean          # remove build artifacts
make web-install     # install the dashboard frontend's npm dependencies
make web-build       # build the dashboard frontend (output embedded into the binary)
make web-dev          # run the frontend's Vite dev server, proxying /api to `odin serve`
make help              # list all targets
```

To test the container image built from the current checkout, stage the same
release binary layout used by the Release workflow and build the
`Containerfile`:

```sh
make release
install -Dm755 target/release/odin dist/odin-linux-amd64
docker build -f Containerfile -t odin:local .
docker run --rm --network host --mount source=odin-test,target=/var/lib/odin odin:local
```

The release profile (`[profile.release]` in `Cargo.toml`) is tuned for a
small, fast binary: full LTO, a single codegen unit, symbol stripping, and
`panic = "abort"`.

A plain `cargo build`/`cargo run` (bypassing the Makefile) still compiles
without Node.js — it embeds whatever is currently in `web/dist/` (a
placeholder page until you run `make web-build` at least once).

To iterate on the frontend without rebuilding the Rust binary on every
change, run `odin serve` in one terminal and `make web-dev` in another; the
Vite dev server proxies `/api` requests (including WebSocket upgrades) to
`odin serve`.

## License

[MIT](LICENSE)
