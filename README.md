# Odin

**Odin** is a command-line tool for running and maintaining [Valheim](https://www.valheimgame.com/)
dedicated servers on Linux. In Norse mythology, Odin is the All-Father who
watches over the nine realms; this Odin watches over your Valheim realms —
installing and updating the game server, starting and stopping instances,
keeping them alive in the background, and managing their mods — so you don't
have to babysit `screen`/`tmux` sessions and SteamCMD invocations by hand.

## Why Odin exists

Running a Valheim dedicated server on Linux usually means stitching together
several separate tools yourself: SteamCMD to install and update the server,
some terminal multiplexer to keep it running after you log out, manual
`BepInEx`/Thunderstore downloads if you want mods, and a pile of shell
scripts to remember port numbers, world names, and passwords across
restarts. Odin folds all of that into one small, dependency-light binary
with a consistent command surface, so managing a server — or a dozen of
them, side by side — is a single, predictable command away.

## Features

- **One binary, no runtime dependencies** beyond `tmux` and (transparently
  managed) SteamCMD — no Python, no Docker required. `systemd` is entirely
  optional, only used if you choose to install the dashboard as a service
  (`odin serve install`).
- **Multiple named instances.** Run several independent Valheim servers on
  one host, each with its own world, port, password, visibility, and mod
  set, sharing a single downloaded copy of the game binaries.
- **Detached by default.** Every instance runs inside its own `tmux`
  session, so it keeps running after you disconnect. Reattach any time with
  `odin console`, or just tail its logs with `odin logs`.
- **Mod support out of the box.** `odin mods add` bootstraps
  [BepInEx](https://github.com/BepInEx/BepInEx) automatically and installs
  mods straight from the [Thunderstore](https://thunderstore.io/c/valheim)
  API by name — no manual unzipping into the right folder. Mods download
  once into a shared global store and get symlinked into every instance
  that wants them, so running the same mod on several servers doesn't mean
  downloading it several times; `odin mods enable`/`disable` toggles a mod
  per instance without reinstalling it.
- **Ready-to-share connect info.** `odin status` prints each instance's
  public address and password alongside its live state, so getting a
  friend into your world doesn't mean a round-trip through `config get`
  and "what's my IP".
- **Backups with a safety net.** `odin backup`/`odin restore` snapshot a
  world's save files to a zip archive; restoring always takes a fresh
  snapshot of the current state first, so a restore is never a one-way,
  unrecoverable action.
- **State you can trust.** An instance's "running" status is always derived
  live from the actual `tmux` session, never from a flag that can silently
  go stale after a crash or a reboot.
- **A `doctor` command** that checks your environment (tmux, SteamCMD, the
  game install, disk permissions, network reachability) so a broken setup
  fails with a clear diagnosis instead of a cryptic error three commands
  later.
- **An optional web dashboard.** `odin serve` starts a single self-contained
  HTTP server — the built frontend is embedded in the binary, no separate
  process or database — for managing everything above from a browser: create
  and control instances, edit config and access lists, search and install
  mods, watch a live console and logs, and see host/instance resource usage.
  See [Web dashboard](#web-dashboard) below.
- **Optional system-wide install.** The `.deb`/`.rpm` packages (`make
  install`) set up a dedicated `odin` system account, `/etc/odin` +
  `/var/lib/odin`, and a system `odin.service` running the dashboard — run
  ad-hoc instance commands as that account with `sudo -u odin odin
  <command>` (`nologin` only blocks interactive login, not `sudo -u`
  exec), or manage everything from the dashboard day to day.

## Requirements

- **Linux only** (x86_64). Odin shells out to `tmux` and to SteamCMD's
  Linux distribution; there is no Windows or macOS support.
- **[`tmux`](https://github.com/tmux/tmux)** must be installed and on
  `PATH`. Odin never tries to install it for you — it's a lightweight,
  universally-packaged system tool (`apt install tmux`, `dnf install tmux`,
  `pacman -S tmux`, ...).
- **SteamCMD is handled for you.** Odin downloads and installs it
  automatically on first `odin install`; you don't need to have it set up
  beforehand.
- **Outbound network access** to Steam's content servers (to install/update
  the game) and to `thunderstore.io` (to install/update mods).
- **[Node.js](https://nodejs.org/) is only needed to build the web
  dashboard's frontend** (`make web-build`, run automatically by `make
  build`/`release`). The compiled `odin` binary itself has no Node.js or
  npm runtime dependency — the dashboard is embedded static assets, not a
  separate process.
- The `.deb`/`.rpm` packages declare `tmux` as a real dependency, resolved
  automatically by `apt`/`dnf`; building from source still requires
  installing it yourself as noted above.

## Installation

Build from source with [Rust](https://www.rust-lang.org/tools/install)
(2024 edition) and the included `Makefile`. Two install modes are
available:

**System-wide (recommended)** — builds a `.deb` or `.rpm` (whichever fits
the host distro) and installs it via the system package manager. This
creates a dedicated `odin` system user, `/etc/odin` and `/var/lib/odin`,
and a system `odin.service` systemd unit running the dashboard as that
user (see [Running as a systemd service](#running-as-a-systemd-service)).
Needs `sudo`, and requires `cargo install cargo-deb` or `cargo install
cargo-generate-rpm` beforehand, matching the host distro:

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
# Install SteamCMD and the Valheim dedicated server (safe to re-run to update later)
odin install

# Create and start a server named "my-server" (always runs detached in tmux)
odin start my-server

# Check on it
odin status
odin logs my-server --follow

# Attach to its live console (Ctrl-b d to detach without stopping the server)
odin console my-server

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
| `odin start <server-name>` | Create the instance if it doesn't exist yet (auto-assigning a free port and a random password) and start it, always detached in its own `tmux` session. |
| `odin stop <server-name>` | Gracefully stop a running instance (sends `Ctrl-C` for a clean world save, then force-kills the session if it doesn't exit in time). |
| `odin restart <server-name>` | Stop the instance if it's running, then start it again. Useful after installing mods or changing config. |
| `odin rename <old-name> <new-name>` | Rename an instance. Must be stopped first. Only its identity changes — the world name and save files are left untouched. |
| `odin delete <server-name> [-y\|--yes] [--keep-backups]` | Permanently delete an instance. Must be stopped first, and asks for confirmation unless `-y` is given. `--keep-backups` deletes everything except the `backups/` directory. |
| `odin status` | List every known instance with its live status, address (public IP:port), world, uptime, mod count, and password — everything needed to hand a friend a join string. |
| `odin console <server-name>` | Attach interactively to a running instance's console via `tmux attach`. |
| `odin logs <server-name> [-f\|--follow] [-n\|--lines N]` | Print (and optionally follow) the instance's captured console output, without attaching to `tmux`. Default 50 lines. |
| `odin exec <server-name> <command>` | Send a line of input to a running instance's console without attaching — handy for scripting. |

### Configuration

| Command | Description |
|---|---|
| `odin config <server-name> get` | Print the instance's world name, port, password, and public/private visibility. |
| `odin config <server-name> set [--world W] [--port P] [--password PW] [--public true\|false]` | Update one or more of those fields. Changes apply on the instance's next `odin restart`. |

### Backups

| Command | Description |
|---|---|
| `odin backup <server-name>` | Zip the instance's save files into a timestamped archive. |
| `odin restore <server-name> [backup-id]` | With no id, lists available backups. With an id, restores it — the instance must be stopped first, and Odin always takes a fresh backup of the current state before overwriting it. |

### Mods

| Command | Description |
|---|---|
| `odin mods search <server-name> <query> [-l\|--list]` | Search the Thunderstore package index by name or author, ranked by relevance (name match beats owner match, ties broken by downloads), showing whether each result is already installed on `<server-name>`. Interactive by default — prompts for a result number to install afterward; pass `-l`/`--list` to just list. |
| `odin mods add <server-name> <mod-id>` | Install a mod by its Thunderstore id (`namespace-name` or `namespace-name-version`). Bootstraps BepInEx into the instance automatically on first use. Downloads into the shared global mod store only if it isn't already there. |
| `odin mods update <server-name>` | Update all of an instance's installed mods to their latest available versions. Replaces the one shared copy in the global store, so this affects every other instance currently linking that mod too. |
| `odin mods list <server-name>` | List installed mods, their versions, and whether each is currently enabled (reads local state only, no network call). |
| `odin mods manage <server-name>` | Interactively toggle which installed mods are enabled via a checkbox list (space to toggle, enter to confirm, esc to cancel). Doesn't install new mods — use `odin mods search` for that. |
| `odin mods enable <server-name> <mod-id>` | Re-enable a previously disabled mod — relinks it from the global store, no reinstall needed. |
| `odin mods disable <server-name> <mod-id>` | Disable a mod without uninstalling it, so BepInEx stops loading it for this instance. |
| `odin mods remove <server-name> <mod-id>` | Uninstall a mod from this instance (the shared download stays in the global store for other instances still using it). |

### Diagnostics

| Command | Description |
|---|---|
| `odin doctor` | Check that `tmux` and SteamCMD are available, the game is installed, the data directory is writable, and Thunderstore/Steam are reachable. Exits non-zero only on a critical failure (e.g. missing `tmux`). |

### Shell completions

| Command | Description |
|---|---|
| `odin completions <shell>` | Print a completion script for `bash`, `zsh`, `fish`, `elvish`, or `powershell` to stdout — e.g. `odin completions zsh > ~/.zfunc/_odin`. |

### Web dashboard

| Command | Description |
|---|---|
| `odin serve [--bind ADDR] [--port PORT]` | Start the web dashboard (JSON API + embedded frontend). Defaults to `127.0.0.1:7331`. No authentication — see [Web dashboard](#web-dashboard). |
| `odin serve install [--bind ADDR] [--port PORT] [--force]` | Install `odin serve` as a per-user systemd service (`systemctl --user`, no root needed). See [Running as a systemd service](#running-as-a-systemd-service). |
| `odin serve uninstall [-y]` | Stop, disable, and remove the systemd service installed by `odin serve install`. |

## Web dashboard

`odin serve` starts a JSON API plus the built frontend from one binary:

```sh
odin serve                            # binds 127.0.0.1:7331 by default
odin serve --bind 0.0.0.0 --port 8080  # or pick your own address/port
```

It covers the same ground as the CLI — dependency status, instance
create/start/stop/restart/rename/delete, per-instance config, mod
search/install/enable via Thunderstore, a live console and log tail, and
editing `adminlist.txt`/`bannedlist.txt`/`permittedlist.txt` — plus live
host and per-instance CPU/RAM usage that isn't exposed by the CLI at all.

**There is no authentication.** `odin serve` binds to `127.0.0.1` by
default for exactly this reason; if you want to reach it from another
machine, put it behind an SSH tunnel (`ssh -L 8080:localhost:7331
your-host`) or your own authenticating reverse proxy rather than binding it
directly to a public address.

The frontend (`web/`, a Vite + React + TypeScript + Tailwind + shadcn/ui
project) is embedded into the binary at compile time, so building it is a
separate step from `cargo build` — see [Development](#development).

### Running as a systemd service

**System-wide install** (via `make install`/the `.deb`/`.rpm` package)
already ships a system `odin.service`, running `odin serve --bind
127.0.0.1 --port 7331` as the dedicated `odin` user and enabled (but not
auto-started) by the package's postinst:

```sh
sudo systemctl start odin.service    # start now
sudo systemctl status odin.service   # check it's running
sudo journalctl -u odin.service -f   # follow logs
sudo systemctl edit odin.service     # override --bind/--port via a drop-in
```

**Per-user install** (`make install-user`, or a plain `cargo build`) has no
system service — install one for your own account instead, no root
required:

```sh
odin serve install                            # binds 127.0.0.1:7331 by default
odin serve install --bind 0.0.0.0 --port 8080  # or pick your own address/port
```

This writes a unit to `~/.config/systemd/user/odin-serve.service` (pass
`--force` to overwrite an existing one) and tries to enable *lingering*
for your user (`loginctl enable-linger`), so the service keeps running
after you log out — on some systems this needs elevated privileges, in
which case `odin serve install` tells you to run `sudo loginctl
enable-linger $(whoami)` yourself. Once installed, manage it with the
usual `systemctl --user`/`journalctl --user` commands, which `odin serve
install` also prints for you:

```sh
systemctl --user enable --now odin-serve.service   # start now and on boot
systemctl --user status odin-serve.service         # check it's running
journalctl --user -u odin-serve.service -f         # follow logs
systemctl --user disable --now odin-serve.service  # stop and disable
```

Remove the service entirely with `odin serve uninstall`.

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
  mods/<mod-id>/                 # shared, global mod store — one download per mod, no matter
                                  # how many instances have it enabled
  servers/<name>/
    state.json                   # instance metadata: port, world, password, visibility, mods, timestamps
    server -> ../../install/valheim
    saves/                       # the world's save files
    backups/<id>.zip              # snapshots created by `odin backup`
    logs/console.log              # captured console output, tailed by `odin logs`
    BepInEx/plugins/<mod-id> -> ../../../../mods/<mod-id>  # present only while enabled
  cache/thunderstore-index.json  # cached Thunderstore package index (1 hour TTL)
```

Each instance's game binaries are a symlink into one shared, SteamCMD-managed
install — so every instance always runs the same game version, and updating
is a single `odin install` rather than one download per server. Mods work
the same way: `odin mods add` downloads a mod once into the shared `mods/`
store, and every instance that has it enabled just symlinks to that same
copy — `odin mods enable`/`disable` add or remove that symlink instead of
copying or moving files around. Because the store isn't versioned per mod,
`odin mods update` replaces the one shared copy wherever it's linked; if you
need two servers pinned to two different versions of the same mod at the
same time, that isn't currently supported.

An instance's **running/stopped status is never stored** — it's always
derived on demand by checking whether its `tmux` session exists. That way
`odin status` can never lie to you after a crash, a manual `tmux
kill-session`, or a host reboot.

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
