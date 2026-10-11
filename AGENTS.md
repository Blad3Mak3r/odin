# AGENTS.md

Guidance for AI coding agents working in this repository. For user-facing
docs (install instructions, full CLI reference, data directory layout),
see [README.md](README.md) — this file is deliberately not a duplicate of
it.

## What this project is

Odin is a single Rust binary that manages the full lifecycle of Valheim
dedicated game servers on Linux: installing/updating the server via
SteamCMD, running multiple independent named "instances" as directly
supervised background processes (see `src/instance/process.rs`), managing
per-instance config and access lists, taking backups, and
installing/toggling BepInEx mods from Thunderstore through a shared,
deduplicated global mod store. It also ships an optional embedded web
dashboard (`odin serve`) built as a separate frontend package and compiled
into the binary at build time.

## Project direction

The web dashboard (`odin serve`) is where this project is headed: it's the
primary, and increasingly the *only*, supported way to operate Odin. New
user-facing capabilities should be built as web API routes
(`src/web/routes/`) plus dashboard UI (`web/src/`) — not as new CLI
subcommands.

The CLI (`src/cli.rs`, `src/commands/`) is being deprecated. Existing
commands keep working for now, but treat the CLI as legacy: don't add new
CLI-only functionality, and don't hold new web-only features back for lack
of CLI parity. When in doubt about where a capability belongs, it belongs
in the web API and dashboard.

## Game server configuration policy

Every new managed game must follow Odin's native-configuration model. Verify
the server's launch and configuration contract from primary sources before
designing or changing its driver: the publisher's dedicated-server
documentation, the official server image or launch script when one exists,
and the templates shipped by the installed server. Match the documented
server version. Community reports may help diagnose a compatibility gap, but
must not replace an official source for the normal contract.

- Keep in SQLite only Odin metadata and settings that Odin must pass directly
  to the process (for example, documented listener ports and RCON transport
  arguments). Keep game settings in the game's native configuration files.
  Do not create a JSON mirror, an Odin-specific settings schema, or duplicate
  game-owned values in the database.
- Derive per-instance data paths, log paths, identities, headless flags, and
  other isolation-critical arguments in the driver. They are not operator
  settings and must not become freely configurable database fields.
- Use the game-provided template to initialize a missing native document and
  never overwrite an existing operator file. The advanced editor may update
  only keys that already exist in the file, must write atomically, and must
  require the server to be stopped. Do not invent missing keys during normal
  editing.
- If a game has no native server configuration document, represent its
  documented launch arguments as typed, validated fields. Omit newly added
  optional arguments by default so existing instances retain their launch
  behavior.
- Preserve existing instances during a migration: native file values take
  precedence when the game documents that precedence. Add only missing
  migrated values, back up retired data without importing it over newer file
  edits, and make any filesystem/database migration safe to resume after a
  failed startup.
- Add tests that assert the complete process command, native document paths,
  initialization behavior, and migration precedence. Cite the official
  sources used to establish the contract in the pull request description.

## Agent workflow and engineering principles

- Prefer the smallest clear change that solves the stated problem. Do not
  introduce speculative extensibility, new layers, indirection, or dependencies
  without a concrete current need.
- Inspect the relevant code before editing it. Reuse and extend existing
  domain types, helpers, API clients, and UI components where they fit; do not
  duplicate logic or create parallel implementations of an existing concept.
  Extract an abstraction only when it removes genuine repeated complexity and
  leaves the call sites easier to understand.
- Use the available specialist skills when they match the task:
  - `ponytail` for repository-aware implementation workflows and task
    execution.
  - `rust-best-practices` for Rust design, error handling, async, testing,
    and idiomatic code decisions.
  - `vercel-react-best-practices` for dashboard React architecture,
    rendering, state, and performance decisions.
  These skills complement this file; repository-specific rules here take
  precedence.
- When the Context7 MCP is available, consult it before implementing against
  third-party libraries or frameworks. Use its version-appropriate
  documentation rather than relying on memory. If it is unavailable, use the
  project's installed versions and the upstream primary documentation.
- Keep changes narrowly scoped. Do not refactor unrelated code merely because
  it is nearby, and do not change generated files or dependency lockfiles
  unless the task requires it.

## Repository layout

- `src/` — the Rust CLI (edition 2024, no workspace, single binary crate).
  - `commands/` — one file per CLI subcommand.
  - `instance/` — instance lifecycle, state, and access lists.
  - `mods/` — BepInEx bootstrap and Thunderstore integration.
  - `web/` — the axum HTTP/WebSocket API backing `odin serve`, under
    `web/routes/`.
- `web/` — a separate npm package (`odin-dashboard`): Vite + React +
  TypeScript + Tailwind + shadcn/ui. Builds to `web/dist/`, which
  `rust-embed` embeds into the compiled binary. Not a Cargo workspace
  member — it's a wholly separate toolchain glued together only through
  the `Makefile` and the build output directory.

## Build, test, and lint

Everything is driven through the root `Makefile`:

- `make build` — builds the frontend, then the debug binary (`target/debug/odin`).
- `make release` — same, but an optimized release binary.
- `make run ARGS="..."` — `cargo run` with forwarded CLI args.
- `make test` — `cargo test`.
- `make lint` — `cargo clippy --all-targets -- -D warnings` (all warnings are errors).
- `make fmt` / `make fmt-check` — `cargo fmt` / `cargo fmt --check`.
- `make check` — fmt-check + lint + test; use for cross-cutting or
  high-risk changes, release validation, or when explicitly requested.
- `make web-install` — `npm --prefix web ci`.
- `make web-build` — installs and builds the frontend only.
- `make web-dev` — Vite dev server (proxies `/api` to `127.0.0.1:7331`, i.e. a locally running `odin serve`).

Frontend-only commands (from `web/`): `npm run build` (`tsc -b && vite
build` — this also typechecks), `npm run lint` (`oxlint`, not
ESLint/Prettier), `npm run dev`.

There is no CI configured in this repo. Validate changes proportionally
instead of running the entire suite by habit:

- Test and lint only the affected Rust module(s), test target(s), or frontend
  package first. For Rust, prefer targeted `cargo test <filter>` invocations
  when an applicable test filter exists; for frontend-only work, run the
  relevant `npm run lint` and/or `npm run build` command from `web/`.
- Run formatting checks for the files you changed. Do not run tests for
  unrelated modules solely as a routine step.
- Broaden validation to `make check` and a frontend build/typecheck only
  when the change crosses subsystem boundaries, changes shared infrastructure
  or public interfaces, is release-sensitive, or the user explicitly asks for
  full verification.
- State exactly which checks were run and any checks intentionally not run,
  with the reason.

## Manually running `odin serve` locally

If the `.deb`/`.rpm` package is also installed on the machine you're
developing on, there is a real, live `odin.service` already bound to
`127.0.0.1:7331` — a real systemd service managing real instances with
real world saves and mods. A locally-built `target/debug/odin serve` will
happily bind to the same port if it's free, but if it isn't, `curl`/a
browser hitting `127.0.0.1:7331` silently talks to the *production*
service instead of your build, with no error indicating the mismatch. Any
instance you create, delete, or mutate while "testing" lands in the real
data directory. Always check `systemctl status odin.service` (or `ss
-ltnp | grep 7331`) before assuming a request landed on your build, and
never point manual/agent testing at port 7331.

Separately, if `/etc/odin/config.toml` exists on the machine (i.e. the
package is installed at all, whether or not the service is running), a
locally-built binary always resolves to system mode (`Paths::resolve` in
`src/paths.rs` detects this purely from that path existing) and tries to
read that file — which is owned `root:odin`, mode `640`, so a normal user
account gets `Permission denied` and the process exits immediately.
`ODIN_DATA_DIR` does not help here: it only overrides the *data* dir, not
this config-file read. There is no per-invocation flag to force per-user
(XDG) mode.

The safe recipe for running a local build on such a machine:

```sh
sudo env ODIN_DATA_DIR=/path/to/a/scratch/dir \
  ./target/debug/odin serve --bind 127.0.0.1 --port 7332
```

- `sudo` (root, not `sudo -u odin`) so the group-restricted config file
  can be read *and* so the process can write to an arbitrary
  `ODIN_DATA_DIR` regardless of its ownership.
- A `--port` other than `7331`, so it can never be mistaken for — or
  silently fall back to — the real service.
- `ODIN_DATA_DIR` pointed at a throwaway directory, so nothing touches
  `/var/lib/odin` or any real instance's saves/config/mods.

## Code conventions

- Rust: formatted with `cargo fmt`, and `cargo clippy --all-targets -- -D
  warnings` must be clean — treat any new clippy warning as a build
  failure, not something to `#[allow]` away without reason.
- Frontend: linted with `oxlint` per `web/.oxlintrc.json` (React,
  TypeScript, and oxc rule sets); TypeScript is strict-ish
  (`noUnusedLocals`, `noUnusedParameters`, `verbatimModuleSyntax`); the
  `@/*` path alias maps to `web/src/*`; UI components follow shadcn/ui's
  `base-nova` style (see `web/components.json`).
- Dashboard tabs that select navigable content must be controlled by React
  Router, not component state. Give every tab a canonical, hierarchical URL;
  redirect an incomplete or invalid tab path to the default valid tab at the
  closest level.
- New capabilities target the web API (`src/web/routes/`) and dashboard
  (`web/src/`) only — see [Project direction](#project-direction). The CLI
  (`src/commands/`) is legacy: it doesn't need to grow alongside the web
  API anymore, and existing CLI/web duplication is being carried, not
  extended.

## Git workflow

- Work on a feature/fix/chore branch, not directly on `main`.
- Name branches with `<type>/<short-description>` using lowercase kebab-case:
  - `feature/...` — new functionality.
  - `fix/...` — bug fixes.
  - `docs/...` — documentation-only changes.
  - `chore/...` — maintenance, dependencies, or tooling changes.
  - `refactor/...` — code changes without behavior changes.
  - `test/...` — adding or updating tests.
  - `perf/...` — performance improvements.
  - `build/...` — build-system or packaging changes.
  - `ci/...` — CI/CD configuration changes.
  - `hotfix/...` — urgent production fixes.
  Examples: `feature/server-backups`, `docs/api-reference`,
  `chore/update-dependencies`.
- Commit incrementally as changes are made rather than batching everything
  into one commit at the end.
- Open a PR when the work is ready, rather than leaving commits sitting on
  the branch.
- Write everything — code, comments, commit messages, PR titles and
  descriptions — in English, regardless of the language used to request
  the work.
- Do not mention AI/Claude/Codex authorship anywhere in commits or PR text.
