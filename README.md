# Odin

**Odin** is a self-hosted web service for orchestrating dedicated game
servers on Linux. It currently supports [Valheim](https://www.valheimgame.com/),
Rust, ARK: Survival Ascended, Palworld, 7 Days to Die, and Counter-Strike 2 from one binary and one
web dashboard. In Norse mythology, Odin is
the All-Father who watches over the nine realms; this Odin watches over your
game servers — installing and updating them, running and supervising named
instances, and managing each game's configuration, backups, and supported
features without making you babysit background processes and SteamCMD
invocations by hand.

Odin ships as a single binary with an embedded web dashboard
(`odin serve`) — that dashboard is the primary, actively developed way to
operate it. A legacy CLI is still bundled for scripting and one-off use,
but new capabilities land in the web API and dashboard, not the CLI. See
[Project direction](#project-direction).

## Why Odin exists

Running dedicated servers on Linux usually means stitching together several
separate tools: SteamCMD to install and update a server, something to keep it
running after logout, and scripts or notes to track ports and configuration.
Valheim adds `BepInEx`/Thunderstore when you want mods. Odin folds that into
one background service and dashboard for creating and controlling servers,
watching logs, editing game-specific configuration, and taking backups. Game
capabilities appear only where they apply: Valheim includes mods, player
state, and readiness; both games include their native access lists, while
Rust v1 otherwise focuses on core operation.

## Features

- **A web dashboard as the primary interface.** `odin serve` starts a
  single self-contained HTTP server — the built frontend is embedded in
  the binary, no separate process or database — for orchestrating every
  instance from a browser: create and control instances, edit config and
  access lists, search and install mods, watch a live console and logs, and
  see host/instance resource usage. See [Web dashboard](#web-dashboard)
  below.
- **One binary, and Podman.** Every game server runs in its own container
  started through Podman's Unix API, with SteamCMD and the game installed
  inside it — no Python or terminal multiplexer, and no game libraries on the
  host.
- **Multiple games and UUID-addressed instances.** Orchestrate Valheim, Rust,
  ARK: Survival Ascended, Palworld, 7 Days to Die, and Counter-Strike 2 side by side on one host.
  Each instance has a persistent UUID for dashboard navigation and API calls;
  names remain editable labels and may be reused by another game without
  sharing data or ports.

- **Compiled game drivers.** Odin ships drivers rather than loading plugins:
  Palworld uses its local, authenticated REST API and an isolated
  runtime/config/save tree.
- **Detached by default, and restart-proof.** Every instance runs in its
  own container, so it keeps running after you disconnect — and after `odin
  serve` itself is restarted or upgraded. Watch it live from the dashboard's
  console view, or with `odin logs --follow` via the legacy CLI.
- **Valheim mod support out of the box.** Bootstraps
  [BepInEx](https://github.com/BepInEx/BepInEx) automatically and installs
  mods straight from the [Thunderstore](https://thunderstore.io/c/valheim)
  API by name — no manual unzipping into the right folder. Mods download
  once per version into a shared global store and get symlinked into every
  instance that wants them. Each instance can use and pin its own exact
  version without duplicating downloads or changing another server.
  The dashboard also detects newer BepInEx releases per instance and can
  update one stopped server or a selected group while preserving local
  plugin and configuration files.
- **Ready-to-share Valheim connect info.** The dashboard (and `odin status`) shows
  each instance's public address and password alongside its live state, so
  getting a friend into your world doesn't mean a round-trip through raw
  config and "what's my IP".
- **Backups with a safety net.** Snapshot server data to a zip archive and
  restore from it only after Odin first snapshots the current data. Rust v1
  requires its server to be stopped for create and restore operations.
  Both games support schedules, retention, and per-instance remote storage
  through AWS S3 or Cloudflare R2.
- **State you can trust.** An instance's "running" status is always derived
  live from the OS process itself (its pid, cross-checked against its own
  start time so a reused pid never lies to you), never from a flag that can
  silently go stale after a crash or a reboot.
- **A `doctor` command** that checks your environment (SteamCMD, the game
  install, disk permissions, network reachability) so a broken setup fails
  with a clear diagnosis instead of a cryptic error three commands later.
- **Optional system-wide install.** The `.deb`/`.rpm` packages (`make
  install`) set up root-owned `/etc/odin` + `/var/lib/odin` and a system
  `odin.service` running the dashboard as a long-lived orchestration
  service. This lets Odin use the rootful Podman socket without a separate
  rootless account; manage it from the dashboard day to day.

## Project direction

The web dashboard (`odin serve`) is where this project is headed: it's the
primary, and increasingly the *only*, supported way to operate Odin. New
user-facing capabilities are built as web API routes plus dashboard UI, not
as new CLI subcommands. The CLI documented below still works and is kept
around for scripting and quick one-off commands, but it's being treated as
legacy — don't expect new features to show up there first.

## Requirements

- **Linux only** (x86_64), with **Podman 5.0.0 or newer** and its Unix
  socket enabled (see [Podman runtime](#podman-runtime)). There is no Windows
  or macOS support.
- **SteamCMD is handled for you.** Each instance's container downloads it and
  the game on its first start; you don't need to have it set up beforehand.
- **Outbound network access** to Steam's content servers (to install/update
  the game) and to `thunderstore.io` (to install/update mods), plus the
  configured AWS S3 or Cloudflare R2 endpoint when remote backups are enabled.
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

This installs the same system package described below: root-owned
`/etc/odin` and `/var/lib/odin`, plus a root-run `odin.service` systemd unit
(see [Running as a systemd service](#running-as-a-systemd-service)).

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
  --tmpfs /run/odin:rw,nosuid,nodev,noexec,size=16m,mode=0750 \
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
| `/var/lib/odin` | Required volume | Database, SteamCMD, game installs, worlds, backups, mods, and logs. |
| `/etc/odin/config.toml` | Image default; optional read-only bind mount | Global configuration. Its default data directory is `/var/lib/odin`. |
| `/run/odin` | Ephemeral `tmpfs` | Per-instance runtime control sockets. |

Keep `/var/lib/odin` as one volume: Odin's mod store uses links into
per-instance directories, and the SQLite database coordinates all of that
state. The image runs as the fixed unprivileged user and group
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

Then open `http://127.0.0.1:7331` in a browser: install/update the supported
games, create and start instances, edit game-specific configuration, watch
logs, and take backups. Rust supports access lists; Valheim
additionally offers mods, live player state, and readiness. See [Web
dashboard](#web-dashboard) for details.

For scripting or quick one-off Valheim changes, the legacy CLI remains
available:

```sh
# Create and start a server named "my-server" (always runs detached, in its container)
odin start my-server

# Check on it
odin status
odin logs my-server --follow

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

### Instance lifecycle

| Command | Description |
|---|---|
| `odin create <server-name>` | Create a new instance (auto-assigning a free port and a random password) without starting it. Fails if the name is already taken — use this when you want to `config set` a world/port/password before the first start. |
| `odin start <server-name>` | Create the instance if it doesn't exist yet (auto-assigning a free port and a random password) and start it in its Podman container. |
| `odin stop <server-name>` | Gracefully stop a running instance (the game saves its world before its container exits; it is killed if it doesn't stop in time). |
| `odin restart <server-name>` | Stop the instance if it's running, then start it again. Useful after installing mods or changing config. |
| `odin rename <old-name> <new-name>` | Rename an instance. Must be stopped first. Only its identity changes — the world name and save files are left untouched. |
| `odin delete <server-name> [-y\|--yes] [--keep-backups]` | Permanently delete an instance. Must be stopped first, and asks for confirmation unless `-y` is given. `--keep-backups` deletes everything except the `backups/` directory. |
| `odin status` | List every known instance with its live status, address (public IP:port), world and mod count. Use `odin config get` for the password. |
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

It covers dependency status, fleet search, tags, bulk lifecycle actions,
instance create/start/stop/restart/rename/clone/delete, game-specific
configuration, live logs, a save file browser (download, upload and delete while the
server is stopped), scheduled backups, and
live plus historical CPU/RAM usage with CSV export. Each instance also has a
**Limits** tab for optional CPU and memory ceilings: CPU is expressed as a
percentage of one core (so `200%` permits two full cores), and memory is
entered in GiB. Limits take effect on the next start, including an automatic
restart. Memory pressure begins at 90% of the selected maximum; at 100% the
kernel may terminate a game process to enforce the hard limit. Leaving either
field empty leaves that resource unlimited.

Resource limits require cgroup v2 and systemd 254 or newer. Packaged installs
configure the required delegation automatically; if an installation cannot
enforce a configured limit, Odin refuses to start that instance rather than
running it without the requested protection. Global settings define
automatic restart and backup defaults for every newly created game server.

### Instance API identity

The dashboard routes instances as `/instance/<uuid>` and the public API uses
`/api/instances/<uuid>/…`. Treat the UUID returned by the instance list as the
canonical identifier for lifecycle, configuration, logs, saves, backups, and
game-specific administration. The former game/name API paths have been
removed; integrations must resolve and retain the UUID from the instance list.
Historical dashboard URLs are redirected to their UUID route, so a rename
never changes a bookmark's target.

Palworld's administrative REST API is called only on its loopback port by
Odin; it is never proxied directly to the network.

For games with a declared server configuration document (ARK: Survival Ascended,
Rust, 7 Days to Die and Palworld), the Configuration tab also has an
**Advanced configuration** section. It reads only Odin-declared game files,
separates settings Odin manages from settings supplied by the game, and lets an
operator edit unmanaged values while the instance is stopped. Each declared
file also offers a syntax-coloured source editor; Odin validates the document,
keeps managed keys unchanged, limits source files to 1 MiB, and writes
atomically. It never discovers arbitrary paths or mod configuration. Supported
formats can also add and delete unmanaged keys.

Rust access lists manage owners, moderators, and bans while the server is
stopped. Valheim adds Thunderstore mod management, BepInEx update checks,
live players, and recent player-session history. Every instance's Backups tab
can use local storage, AWS S3, or Cloudflare R2. Remote backups remain listed
and can be restored or deleted from Odin; restores download a temporary
archive and remove it again when the operation finishes.

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
already ships a root-run system `odin.service`, running `odin serve --bind
127.0.0.1 --port 7331` and enabled (but not auto-started) by the package's
postinst:

```sh
sudo systemctl start odin.service    # start now
sudo systemctl status odin.service   # check it's running
sudo journalctl -u odin.service -f   # follow logs
sudo systemctl edit odin.service     # override --bind/--port via a drop-in
```

When a newer release is available, this install can update itself from the
dashboard's release banner. Confirming the update starts a separate system
service, so the dashboard can restart while game instances keep running.
Restart running instances afterward to use the new Odin supervisor. Container
deployments still update by pulling the new image and recreating the container.

A per-user install (`make install-user`, or a plain `cargo build`) has no
system service; run `odin serve` directly, or manage it yourself under
your own `systemctl --user`/init setup.

## Configuration

### Podman runtime

Every instance runs in an `odin-srv` Podman container. Each one owns its own
persistent SteamCMD and game payload under
`<data>/runtimes/<game>/<uuid>/`, downloaded inside the container on its first
    start (Rust's, ARK's, 7 Days to Die's and Palworld's are several gigabytes, so that
first start takes a while), and deleting an instance removes its runtime
volume. With **auto restart** enabled,
Odin restarts a container that stopped without an operator asking for it, including
after a host reboot; an operator **Stop** is respected. World data, configuration and
backups stay in the instance directory.

Rust is stopped through its RCON `quit`, 7 Days to Die through its console's
`shutdown` and Palworld through its REST API's `shutdown`, so all of them save
the world before exiting; if that interface is disabled or unreachable Odin
signals the server instead. Counter-Strike 2 has no world to save; it is
stopped through its Source RCON `quit`, with the same fallback.

Palworld keeps its saves and `PalWorldSettings.ini` in the instance directory
under `runtime/Pal/Saved/`; inside the container the game's own `Pal/Saved`
is a link to it. Odin creates `PalWorldSettings.ini` from the server's shipped
`DefaultPalWorldSettings.ini` on the first start and never replaces an existing
file. Files that earlier versions left in `runtime/` besides `Pal/Saved/` are
unused and can be deleted.

Counter-Strike 2 is a native Linux server (Steam app 730, tens of gigabytes, so
the shared installation pays off quickly). Odin passes only the game port
(UDP and TCP; Source RCON shares the TCP port), the start map, the game mode,
the player limit, an optional Game Server Login Token and a generated RCON
password; everything else is in the instance's `cfg/server.cfg`, which starts
from the file the game ships and is never replaced. SourceTV's port is derived
from the game port (+5) and reserved with it, and an instance's `server.cfg`
is linked (own files) or mounted (shared files) over the game's own. The
server binds to all addresses (`-ip 0.0.0.0`): without it Counter-Strike 2
listens for RCON only on the address its hostname resolves to. Without a login
token the server runs but is not listed publicly.

7 Days to Die, ARK: Survival Ascended and Counter-Strike 2 instances can have private game
files or reuse a shared installation; the game's page in the dashboard installs
and updates the shared payload, and all shared instances must be stopped while
it does. A stopped 7 Days to Die instance with its own game files can be moved
onto the shared installation from its Configuration tab: its saves, settings
and mods stay where they are, its game files become the shared installation if
there is none yet, and they are deleted only if you ask. ARK: Survival Ascended runs through Proton and exposes its common
native INI files under the game's Configuration tab. ARK clusters are local to one Odin host: create a cluster in
that page, select stopped ARK instances as members, and Odin mounts one
transfer directory for the cluster while passing the shared cluster ID to each
member. Odin does not configure cross-host filesystems; use a separately
managed shared filesystem only after testing it with ARK.

Odin itself starts without Podman configured. To use these games, set the
socket in **Settings** or in Odin's configuration file:

```toml
[podman]
socket = "/run/podman/podman.sock"
# base_image = "ghcr.io/blad3mak3r/odin-srv:base"
# proton_image = "ghcr.io/blad3mak3r/odin-srv:proton"
# runtime_image = "ghcr.io/blad3mak3r/odin-srv:base" # deprecated alias
# odin_binary = "/usr/bin/odin"
```

The Settings page can pull either runtime image. Pulling never interrupts a
running server; stopped containers are recreated with the pulled image, and a
stopped container whose local image changes is recreated on its next start.

Enable the rootful Podman socket with `sudo systemctl enable --now
podman.socket`. The dashboard checks connectivity and requires Podman 5.0.0
or newer before container operations are available. Do not grant unrelated users
access to this socket: it grants authority over its containers. Odin uses the
Unix API directly; it does not require the `podman` CLI in the service path.

Each instance shows its container and runtime-socket health on its detail
page. **Repair** is available only while the instance is stopped: it
regenerates Odin's private runtime specification, removes stale sockets, and
deletes the stopped UUID-named container so the next **Start** recreates it. It
never downloads the game, starts the instance, or changes saves, backups, or
mods.

> **Upgrading from a version that ran games as host processes?** Stop those
> instances before upgrading. Odin now only manages containers: a server still
> running outside one is no longer tracked, and its ports stay taken until you
> stop it. Its data is untouched; start it again from the dashboard and its
> container downloads a private copy of the game. Shared installs under
> `<data>/install/` and `<data>/games/*/install/` are no longer used and can
> be deleted.

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
  mods/<mod-id>/<version>/       # immutable shared payload — one download per exact version
  servers/<name>/
    state.json                   # instance metadata: port, world, password, visibility, mods, timestamps
    saves/                       # the world's save files
    backups/<id>.zip              # local snapshots; remote uploads are removed after success
    logs/console.log              # captured console output, tailed by `odin logs`
    BepInEx/plugins/<mod-id> -> ../../../../mods/<mod-id>/<version>  # exact enabled version
  games/rust/
    instances/<name>/            # Rust configuration, identity, logs, and backups
  runtimes/<game>/<uuid>/        # a Podman instance's private SteamCMD and game payload
  games/palworld/
    instances/<name>/
      runtime/Pal/Saved/         # saves and Config/LinuxServer/PalWorldSettings.ini
  games/7d2d/
    instances/<name>/
      config/serverconfig.xml    # instance-local server configuration
      Mods/<mod-name>/ModInfo.xml # instance-local uploaded 7D2D mods
      Saves/                      # isolated 7D2D saves
  games/cs2/
    instances/<name>/
      cfg/server.cfg             # the instance's own server.cfg
      logs/console.log
  cache/thunderstore-index.json  # cached Thunderstore package index (1 hour TTL)
```

Each instance has its own copy of its game under `runtimes/`, updated from
the instance page while it runs. Rust instances use distinct
`server.identity` values. Before every Rust start, inside its container, Odin
maintains `$HOME/.steam/sdk64/steamclient.so` as a hardlink to SteamCMD's
64-bit client library (or a symlink when the paths cross filesystems) so
Steamworks can initialize. Odin stops Rust through its console `quit` command,
which saves the world; a plain signal does not.
Valheim mods are versioned: `odin mods add` downloads each `(mod, version)`
once into the shared `mods/` store, and every enabled instance symlinks to
its exact version. Updating one instance creates or reuses the newer payload
and only repoints that instance; pinned instances remain unchanged. The
dashboard can switch back to any cached version and prune versions no longer
used by an instance.

7 Days to Die uses the internal game identifier `7d2d`. Its instances reserve
UDP ports `26900–26902` by default and load user-uploaded ZIP mods from each
instance's own `Mods/` directory. Odin requires a stopped 7D2D server before
creating a backup or changing mods.

Each 7D2D instance starts from the `serverconfig.xml` template shipped with its
own copy of the game: the container copies it into the instance's `config/`
directory on the first start and never replaces an existing file. When that
template changes after a game update, the dashboard offers a review
of added, removed, and changed defaults. Applying a review is explicit: Odin
never overwrites an instance value that differs from its original template.
Existing instances can adopt their current configuration as the baseline for
future reviews.

On 7 Days to Die V3 and later, gameplay rules are represented by the game's
single `SandboxCode` value rather than individual XML properties. Odin exposes
the complete value and a small visual editor for common V3 rules; it preserves
unrecognised encoded rules. Copy the complete code from the game's Sandbox
Options screen when using a newer or otherwise different game schema.

Odin can also enable 7D2D's password-protected console for an instance. It
uses a separate instance port and Odin connects to it through `127.0.0.1`; the
password is never returned by the API. Keep that port blocked from external
networks because the game does not provide Odin with a console bind-address
setting. Once enabled, the dashboard Console and Players tabs can run server
commands, show version/build and memory information, list connected players,
record recent sessions, and manage bans, administrators, and the whitelist.
An instance is reported as *ready* once its game process has started.

On the first start after upgrading, Odin automatically moves the legacy
single-version mod store into this layout and repoints every existing instance;
no migration command or dashboard action is required.

An instance's **running/stopped status is never trusted from a stored
flag** — it's always derived on demand by asking Podman whether its container
is running. That way `odin status` can never be wrong after a crash or a host
reboot.

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
without Node.js — it embeds whatever is currently in `web/dist/`. If the
frontend has not been built, dashboard requests return an explicit error;
run `make build` to build and embed the dashboard.

To iterate on the frontend without rebuilding the Rust binary on every
change, run `odin serve` in one terminal and `make web-dev` in another; the
Vite dev server proxies `/api` requests (including WebSocket upgrades) to
`odin serve`.

## License

[MIT](LICENSE)
