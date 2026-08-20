# valheim-manager

Manages the lifecycle of one or more Valheim dedicated game server instances on
a Linux host: install/update via SteamCMD, start/stop instances detached in
tmux, attach to a running console, and manage mods via BepInEx + Thunderstore.

Linux only. Requires `tmux` on `PATH`.

## Commands

```
valheim install                       # install/update SteamCMD + the Valheim dedicated server
                                       # (refuses while any instance is running)
valheim start <server-name>           # create (if new) and start an instance, always detached
valheim stop <server-name>            # gracefully stop a running instance
valheim console <server-name>         # attach to a running instance's console (tmux attach)
valheim status                        # list all known instances and their state
valheim mods add <server-name> <mod-id>      # install a mod from Thunderstore (namespace-name[-version])
valheim mods update <server-name>            # update that instance's mods to their latest versions
```

Server names must be DNS-friendly: lowercase letters, digits, and hyphens
only, and cannot start or end with a hyphen (e.g. `my-server`).

## Data layout

All state lives under a data directory (XDG default:
`~/.local/share/valheim-manager`, overridable via `VALHEIM_MANAGER_DATA_DIR`
or `data_dir` in `~/.config/valheim-manager/config.toml`):

```
<data_dir>/
  steamcmd/                 # SteamCMD install
  install/valheim/          # shared Valheim dedicated server binaries (all instances symlink to this)
  servers/<name>/
    state.json               # instance metadata (port, world, mods, timestamps)
    server -> ../../install/valheim
    saves/
    logs/console.log
    BepInEx/plugins/<mod-id>/
  cache/thunderstore-index.json
```

## Development

```
cargo build
cargo test
cargo clippy --all-targets
cargo fmt
```
