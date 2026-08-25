# AGENTS.md

Guidance for AI coding agents working in this repository. For user-facing
docs (install instructions, full CLI reference, data directory layout),
see [README.md](README.md) — this file is deliberately not a duplicate of
it.

## What this project is

Odin is a single Rust binary that manages the full lifecycle of Valheim
dedicated game servers on Linux: installing/updating the server via
SteamCMD, running multiple independent named "instances" detached in
`tmux`, managing per-instance config and access lists, taking backups, and
installing/toggling BepInEx mods from Thunderstore through a shared,
deduplicated global mod store. It also ships an optional embedded web
dashboard (`odin serve`) built as a separate frontend package and compiled
into the binary at build time.

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
- `make check` — fmt-check + lint + test; run this before opening a PR.
- `make web-install` — `npm --prefix web ci`.
- `make web-build` — installs and builds the frontend only.
- `make web-dev` — Vite dev server (proxies `/api` to `127.0.0.1:7331`, i.e. a locally running `odin serve`).

Frontend-only commands (from `web/`): `npm run build` (`tsc -b && vite
build` — this also typechecks), `npm run lint` (`oxlint`, not
ESLint/Prettier), `npm run dev`.

There is no CI configured in this repo, so `make check` plus a frontend
build/typecheck/lint is the only signal an agent gets before a PR is
reviewed by a human — run all of it.

## Code conventions

- Rust: formatted with `cargo fmt`, and `cargo clippy --all-targets -- -D
  warnings` must be clean — treat any new clippy warning as a build
  failure, not something to `#[allow]` away without reason.
- Frontend: linted with `oxlint` per `web/.oxlintrc.json` (React,
  TypeScript, and oxc rule sets); TypeScript is strict-ish
  (`noUnusedLocals`, `noUnusedParameters`, `verbatimModuleSyntax`); the
  `@/*` path alias maps to `web/src/*`; UI components follow shadcn/ui's
  `base-nova` style (see `web/components.json`).
- Keep the CLI (`src/commands/`) and the web API (`src/web/routes/`) in
  sync when adding a capability that should be reachable from both — most
  instance/mod mutations are meant to be usable from either surface.

## Git workflow

- Work on a feature/fix/chore branch, not directly on `main`.
- Commit incrementally as changes are made rather than batching everything
  into one commit at the end.
- Open a PR when the work is ready, rather than leaving commits sitting on
  the branch.
- Write everything — code, comments, commit messages, PR titles and
  descriptions — in English, regardless of the language used to request
  the work.
- Do not mention AI/Claude/Codex authorship anywhere in commits or PR text.
