# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Community and game-server administrators running one or more dedicated game
servers on a Linux host they control: a home server, VPS, or dedicated
machine. They may manage a guild, community, or group of friends and need
servers to remain available, updated, and independently configured without
manually coordinating SteamCMD, shell scripts, ports, and background
processes.

## Product Purpose

Odin is a self-hosted Linux service and embedded web dashboard for managing
the lifecycle of dedicated game servers from one Rust binary. It installs and
updates supported servers through SteamCMD, runs isolated named instances,
supervises their processes, exposes each game's configuration, and provides
backups plus the game-specific capabilities Odin supports.

The currently compiled game drivers are Valheim, Rust, V Rising, Palworld,
RuneScape: Dragonwilds, and 7 Days to Die. Odin is not a general-purpose
plugin host: supported games ship as compiled drivers with explicit launch,
storage, and configuration contracts.

## Positioning

A dependency-light, self-hosted control plane for several dedicated games on
one Linux host. It combines a single compiled binary, an embedded dashboard,
SQLite for Odin's own metadata, shared game installs, and isolated instance
data. It does not require Python, Docker, a terminal multiplexer, a hosted
service, or an external database.

The dashboard is the primary product interface. The bundled CLI remains for
existing scripts and one-off operations, but is legacy: new user-facing
capabilities are built in the web API and dashboard.

## Operating Context

- Odin runs on a Linux x86_64 host controlled by the administrator. It
  supports system-wide package installs using `/etc/odin` and `/var/lib/odin`,
  and per-user installs using XDG paths.
- `odin serve` binds to `127.0.0.1` by default and has no built-in
  authentication. Remote access belongs behind an SSH tunnel or an
  administrator-provided authenticated reverse proxy.
- Servers are managed as independent instances, scoped by both game and
  name. Instances can share a display name across different games, but never
  share save data, logs, runtime paths, identity, or reserved ports.
- Odin shares immutable install files per game where the game's runtime
  allows it, while deriving instance-owned data, logs, runtime directories,
  and process identity. Each instance is directly supervised by Odin.
- Running state is derived from the operating system process and its start
  time, not from a stored boolean. The dashboard also provides live logs and
  host/per-instance resource information.
- Odin stores its metadata, lifecycle records, schedules, and process
  arguments in SQLite. Game-owned settings remain in each game's documented
  native configuration files; the dashboard edits those files without
  creating an Odin-specific mirror.

## Current Capabilities and Constraints

- Every supported driver can create, start, stop, restart, rename, delete,
  install/update, configure, and back up isolated instances through the
  dashboard.
- All currently supported games expose backups. Player views, mod management,
  access-list management, and readiness signals are intentionally
  game-specific rather than universal.
  - Valheim supports players, BepInEx/Thunderstore mods, access lists, and
    readiness.
  - Rust supports access lists and RCON-backed administration, but not Odin
    mod management, player views, or readiness detection.
  - V Rising supports access lists and RCON-backed administration.
  - Palworld supports player and access-list operations through its local,
    authenticated REST API, plus readiness detection.
  - RuneScape: Dragonwilds supports access-list operations.
  - 7 Days to Die supports player views, mod management, and readiness
    detection.
- Game configuration follows the native contract for each server. Odin keeps
  lifecycle-critical process arguments and metadata in SQLite; all other
  game settings belong to native files or, when a server has no equivalent
  file, documented typed launch arguments.
- Configuration documents are initialized from installed game templates when
  applicable. Existing files are preserved, advanced edits only change keys
  already present, and changes require a stopped server and use atomic file
  replacement.
- Valheim's shared BepInEx and Thunderstore store is deduplicated globally.
  This mod model is specific to Valheim; mod support is not presumed for the
  other games.
- Linux only; there is no Windows or macOS host support. V Rising runs its
  Windows dedicated server through an Odin-managed Proton-GE runtime.
- Odin's dashboard assets are embedded in the binary. Node.js is needed to
  build those assets, not to run the resulting service.

## Roadmap and Product Direction

1. The web dashboard is the front door going forward. New operational
   capability belongs in the web API and dashboard rather than new CLI-only
   commands.
2. Additional game support must use compiled drivers and the native
   configuration model: validate the launch contract against official
   documentation, official launch scripts/images, and installed templates;
   isolate all instance-owned data; and keep game settings in native files.
3. New game configuration work must preserve existing instances. Migrations
   must be resumable, must not overwrite newer native-file edits, and must
   leave retired configuration data recoverable.
4. Capability differences remain visible and honest. The UI must expose only
   the lifecycle, configuration, player, mod, backup, access-list, and
   readiness features a particular driver actually implements.

## Brand Commitments

- Name: **Odin** — the Norse All-Father who watches over many realms. The
  product watches over several independent game-server instances under one
  administrator.
- The existing logo, favicon, and shadcn/ui `base-nova` dashboard style are
  the current visual baseline (`web/public/logo.png`,
  `web/public/favicon.ico`, and `web/components.json`). They are incumbent
  product assets, not a request to redesign the brand.

## Evidence on Hand

- `README.md` is the user-facing reference for installation, supported games,
  CLI compatibility, data layout, dashboard operation, and systemd setup.
- `AGENTS.md` defines the implementation direction: dashboard-first work,
  native game configuration, official-source verification, and isolation
  requirements for future game drivers.
- No testimonials, case studies, pricing, or usage benchmarks are available.
  Odin is open source under the MIT license; product work must not fabricate
  external validation or commercial claims.

## Product Principles

1. One self-hosted binary, minimal runtime dependencies — do not design
   toward accounts, hosted control planes, or required cloud services that
   Odin does not provide.
2. Multi-game and multi-instance operation are core workflows. Isolation is
   part of correctness: one instance must not alter another instance's save,
   configuration, identity, logs, or ports.
3. The dashboard is the primary interface; the CLI is carried for
   compatibility, not expanded as the default surface for new product work.
4. Native game configuration is authoritative. Odin manages the process and
   isolation boundary while retaining the server's documented configuration
   format and precedence rules.
5. State must be trustworthy over convenient. Lifecycle state, resource
   usage, and server capability must reflect live process information and the
   selected driver's actual implementation.
6. No accounts, no login. Preserve the loopback-by-default, no-auth product
   posture and do not imply identity or session management in the UI.
