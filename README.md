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
  managed) SteamCMD — no Python, no Docker, no systemd required.
- **Multiple named instances.** Run several independent Valheim servers on
  one host, each with its own world, port, password, visibility, and mod
  set, sharing a single downloaded copy of the game binaries.
- **Detached by default.** Every instance runs inside its own `tmux`
  session, so it keeps running after you disconnect. Reattach any time with
  `odin console`, or just tail its logs with `odin logs`.
- **Mod support out of the box.** `odin mods add` bootstraps
  [BepInEx](https://github.com/BepInEx/BepInEx) automatically and installs
  mods straight from the [Thunderstore](https://thunderstore.io/c/valheim)
  API by name — no manual unzipping into the right folder.
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

## Installation

Build from source with [Rust](https://www.rust-lang.org/tools/install)
(2024 edition) and the included `Makefile`:

```sh
git clone git@github.com:Blad3Mak3r/odin.git
cd odin
make release          # optimized build at target/release/odin
make install           # installs to ~/.local/bin/odin (override with PREFIX=/usr/local)
```

Make sure the install directory (`~/.local/bin` by default) is on your
`PATH`. See [`make help`](#development) for every available target.

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
odin mods search valheim-plus
odin mods add my-server nathanhwood-ValheimPlus
odin restart my-server

# Snapshot the world, then stop the server
odin backup my-server
odin stop my-server
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
| `odin start <server-name>` | Create the instance if it doesn't exist yet (auto-assigning a free port and a random password) and start it, always detached in its own `tmux` session. |
| `odin stop <server-name>` | Gracefully stop a running instance (sends `Ctrl-C` for a clean world save, then force-kills the session if it doesn't exit in time). |
| `odin restart <server-name>` | Stop the instance if it's running, then start it again. Useful after installing mods or changing config. |
| `odin status` | List every known instance with its live status, port, world, uptime, and mod count. |
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
| `odin mods search <query>` | Search the Thunderstore package index by name or author. |
| `odin mods add <server-name> <mod-id>` | Install a mod by its Thunderstore id (`namespace-name` or `namespace-name-version`). Bootstraps BepInEx into the instance automatically on first use. |
| `odin mods update <server-name>` | Update all of an instance's installed mods to their latest available versions. |
| `odin mods list <server-name>` | List installed mods and their versions (reads local state only, no network call). |
| `odin mods remove <server-name> <mod-id>` | Uninstall a mod. |

### Diagnostics

| Command | Description |
|---|---|
| `odin doctor` | Check that `tmux` and SteamCMD are available, the game is installed, the data directory is writable, and Thunderstore/Steam are reachable. Exits non-zero only on a critical failure (e.g. missing `tmux`). |

## Configuration

Odin stores everything under a single **data directory**, resolved in this
order:

1. `data_dir` in `~/.config/odin/config.toml`
2. the `ODIN_DATA_DIR` environment variable
3. the XDG default, `~/.local/share/odin`

### Data layout

```
<data_dir>/
  steamcmd/                     # SteamCMD installation
  install/valheim/               # shared Valheim dedicated server binaries — every instance
                                  # symlinks to this, so `odin install` updates them all at once
  servers/<name>/
    state.json                   # instance metadata: port, world, password, visibility, mods, timestamps
    server -> ../../install/valheim
    saves/                       # the world's save files
    backups/<id>.zip              # snapshots created by `odin backup`
    logs/console.log              # captured console output, tailed by `odin logs`
    BepInEx/plugins/<mod-id>/     # per-instance mods — each instance can run a different mod set
  cache/thunderstore-index.json  # cached Thunderstore package index (1 hour TTL)
```

Each instance's game binaries are a symlink into one shared, SteamCMD-managed
install — so every instance always runs the same game version, and updating
is a single `odin install` rather than one download per server. Mods,
however, are entirely per-instance, so different servers can run completely
different mod sets against that same shared binary.

An instance's **running/stopped status is never stored** — it's always
derived on demand by checking whether its `tmux` session exists. That way
`odin status` can never lie to you after a crash, a manual `tmux
kill-session`, or a host reboot.

## Development

```sh
make build       # debug build
make release      # optimized release build
make test         # run the test suite
make lint          # clippy, denying warnings
make fmt           # format the codebase
make fmt-check     # check formatting without modifying files
make check         # fmt-check + lint + test — run before committing
make install        # install the release binary (PREFIX=... to override, default ~/.local)
make clean          # remove build artifacts
make help            # list all targets
```

The release profile (`[profile.release]` in `Cargo.toml`) is tuned for a
small, fast binary: full LTO, a single codegen unit, symbol stripping, and
`panic = "abort"`.

## License

[MIT](LICENSE)
