# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Community and game-server admins running Valheim dedicated servers — often
several at once, sometimes alongside other game servers — for a player base
larger than just themselves (a guild, a community, a group of friends they're
responsible for keeping online). They are comfortable with a terminal and a
Linux host, but are operating this more like light ops than a personal
hobby project: uptime, mod curation, and being able to hand out connect info
matter because other people depend on it.

## Product Purpose

Odin manages the full lifecycle of Valheim dedicated servers on Linux from
one dependency-light binary: installing/updating the game via SteamCMD,
running multiple independent named instances as supervised background
processes, per-instance config and access lists, backups, and BepInEx mod
installs from Thunderstore through a shared, deduplicated global mod store.
Success is not babysitting SteamCMD invocations, terminal multiplexers, and
shell scripts by hand to keep one or many servers alive, updated, and
moddable.

## Positioning

A single, dependency-light binary (no Python, no Docker, no terminal
multiplexer, no database) that runs many independent named Valheim
instances side by side, sharing one SteamCMD-installed copy of the game
binaries and one deduplicated mod store, with an embedded web dashboard
compiled directly into the binary rather than a separate hosted service.

## Operating Context

- Runs on a Linux host (x86_64) the admin controls directly — a home box,
  a VPS, or a dedicated system-wide install with its own `odin` service
  user.
- Two install modes: system-wide (`.deb`/`.rpm`, `/etc/odin` +
  `/var/lib/odin`, a systemd `odin.service`) and per-user (`~/.local/bin`,
  XDG paths) — the same product, different deployment footprint.
- `odin serve` ships with **no authentication** and binds to `127.0.0.1` by
  default; reaching it remotely is expected to go through an SSH tunnel or
  the admin's own reverse proxy, not a built-in login. UI/UX should not
  imply accounts, sessions, or login exist.
- Instances are managed side by side: creating/starting/stopping/renaming,
  per-instance config (world, port, password, public/private visibility),
  access lists (admin/banned/permitted), backups/restore, and mod
  install/enable/disable — all per named instance, against shared global
  game binaries and a shared global mod store.
- A live console/log tail and host + per-instance CPU/RAM usage are part of
  day-to-day operation, not just setup.

## Capabilities and Constraints

- Linux-only; no Windows/macOS support.
- Single compiled binary; the dashboard frontend is embedded static assets
  (`rust-embed`), not a separate process or database.
- The CLI (`src/commands/`) is being deprecated in favor of the web
  dashboard: per this repo's AGENTS.md, "the web dashboard is where this
  project is headed: it's the primary, and increasingly the *only*,
  supported way to operate Odin." New user-facing capabilities land as web
  API + dashboard UI, not new CLI subcommands.
- Instance names are DNS-friendly (lowercase, digits, hyphens; no leading/
  trailing hyphen) — a real constraint surfaced anywhere a name is entered.
- Mods are versioned once per shared store entry, not per instance — two
  instances can't currently pin two different versions of the same mod
  simultaneously; `odin mods update` affects every instance linking that
  mod.
- An instance's running/stopped state is always derived live from the OS
  process (pid + start time), never trusted from a stored flag.

## Brand Commitments

- Name: **Odin** — the Norse All-Father who watches over the nine realms;
  the product "watches over your Valheim realms." The naming/mythology
  framing (realms, instances as servers under one all-father) is an
  existing, intentional identity element, not incidental.
- An existing logo, favicon, and a shadcn/ui `base-nova`-styled dashboard
  (Vite + React + TypeScript + Tailwind) are the confirmed, current visual
  baseline (`web/public/logo.png`, `web/public/favicon.ico`,
  `web/components.json`) — treated as the incumbent world for any future
  design work, not something this init redoes or opens for revision.

## Evidence on Hand

- `README.md` is the authoritative, current feature/command reference —
  installation, full CLI surface, data directory layout, dashboard
  description, and systemd service setup.
- No testimonials, case studies, press, pricing, or usage benchmarks exist;
  future work must not fabricate them. Odin is open-source (MIT) and free.

## Product Principles

1. One binary, minimal runtime dependencies — never design toward a
   posture (hosted service, required cloud dependency, always-on account
   system) the product doesn't actually have.
2. Multi-instance is core, not an edge case — flows and UI should assume an
   admin managing several servers side by side, not just one.
3. The dashboard is the front door going forward — new capability and
   design investment target `odin serve`, not the legacy CLI.
4. State must always be trustworthy over convenient — status, uptime, and
   resource numbers are derived live, never cached optimistically; design
   should preserve that honesty rather than paper over it with assumed
   state.
5. No accounts, no login — respect the no-auth-by-design posture; don't
   design UI that implies identity/session management the product
   deliberately doesn't have.
