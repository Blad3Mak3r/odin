# Odin improvement roadmap

A prioritized plan for making the web dashboard and its backing API
genuinely more complete and useful for the people who actually run Odin:
admins keeping several Valheim servers alive for a community or guild.

This document assumes the dashboard (`odin serve`) stays the primary and
increasingly only supported interface — new capability belongs in the web
API (`src/web/routes/`) plus dashboard UI (`web/src/`), not the CLI. See
[AGENTS.md](AGENTS.md) for the repo's conventions and workflow.

## Ground rules

Two constraints shape every item below and should not be relitigated per
feature:

- **No accounts, no login.** `PRODUCT.md` documents this as a deliberate
  product decision: `odin serve` has zero auth middleware, and reaching it
  remotely is the admin's own job (SSH tunnel / reverse proxy). Nothing
  here proposes a dashboard login or RBAC/session UI. The one auth-adjacent
  idea that fits is an optional API token for *outbound* integrations
  (webhooks) — never inbound dashboard authentication.
- **Valheim-specific, not multi-game.** SteamCMD, BepInEx, and Thunderstore
  are all Valheim-specific integrations. Nothing here proposes generalizing
  the instance model to other dedicated servers — the goal is to go deeper
  on what already exists, not broader.

Every item is scoped to be its own PR, consistent with this repo's
"work on a branch, commit incrementally" workflow.

## Shared plumbing — reuse, don't duplicate

Three pieces of existing infrastructure should absorb almost all new
backend work below, instead of each feature inventing its own:

- **`src/activity.rs` (`ActivityLog`)** — persisted (`activity_events`
  table), a typed, additive `ActivityKind` enum (`#[serde(tag = "kind")]`),
  already broadcasts to the global `/api/events/sse` feed. Extending it
  means adding new `ActivityKind` variants, not a new events system.
- **`src/web/jobs.rs` (`JobRegistry`)** — anything long-running should
  spawn through `JobRegistry::spawn`, not a bespoke async task.
  `JobKindDescr` is a small, additive, closed enum. Today the registry is
  in-memory only, capped at 2000 log lines per job — see 2.1 below.
- **`src/web/runtime.rs` (`RuntimeRegistry`)**, ticked every 3s from
  `run_telemetry_tick` in `src/web/mod.rs` — the one place that already
  walks every instance on a fixed interval and already distinguishes
  running/stopped transitions. New per-tick derived behavior (e.g. crash
  auto-restart) belongs there, not in a second poller.

Items below call out explicitly whenever they touch one of these three
files, since that's the signal for "extends shared plumbing" versus "new
isolated route."

---

## Phase 1 — Quick wins

Frontend-only. The backend already supports these; nothing in the UI wires
them up. Cheapest, most immediately shippable items in this roadmap.

### 1.1 Wire up instance rename in the UI

`POST /instances/{name}/rename` and the `useRenameInstance` hook
(`web/src/lib/queries.ts:145`) both already exist and work — no component
calls the hook. An admin who typo'd a server name, or wants to rename their
"test" instance to something player-facing, currently has to delete and
recreate it, losing state in the process.

- **Backend**: none — route exists.
- **Frontend**: add a rename affordance to `InstanceHeader.tsx` (e.g. an
  edit icon next to the instance name), reusing the `Dialog`/`Input`
  pattern already used by `CreateInstanceDialog`. Must navigate to
  `/instances/{new_name}` after success, since the route param is the name.
- **Effort**: S, frontend-only.

### 1.2 Granular access-list editing

`POST /instances/{name}/lists/{kind}` (add one) and
`DELETE /instances/{name}/lists/{kind}/{id}` (remove one) exist
(`src/web/routes/lists.rs`), but `SteamIdListEditor.tsx` only ever calls
`useSetList` with the entire array. Today, adding one SteamID to a
40-person guild's admin list PUTs the whole list back — a race hazard if
two admins edit concurrently, and it leaves working backend routes unused.

- **Backend**: none — routes exist.
- **Frontend**: add `useAddListEntry`/`useRemoveListEntry` hooks to
  `queries.ts` (mirroring `useSetList`'s shape); swap
  `SteamIdListEditor.tsx`'s `addId`/`removeId` over to them.
- **Effort**: S, frontend-only.

### 1.3 Delete instance from the instances list

Delete only lives on the instance-detail page today
(`InstanceHeader.tsx`'s trash icon). An admin cleaning up a batch of
test/abandoned instances has to open each one individually.

- **Backend**: none — `DELETE /instances/{name}` exists.
- **Frontend**: add a delete action to `InstanceActions` in
  `InstancesPage.tsx`, reusing the `useConfirmDialog` + `useDeleteInstance`
  pattern already used on the detail page.
- **Effort**: S, frontend-only.

### 1.4 Surface `keep_backups` on delete

`DELETE /instances/{name}?keep_backups=true` already exists
(`DeleteQuery` in `src/web/routes/instances.rs:116`), but no UI control
sets it. An admin deleting a seasonal/event server can't currently choose
to keep its backups from the dashboard.

- **Backend**: none.
- **Frontend**: add a checkbox to the delete confirm dialog; thread the
  query param through `useDeleteInstance`.
- **Effort**: S, frontend-only.

### 1.5 Inline CPU/mem columns on the instances table

`InstancesPage.tsx` shows World/Port/Mods but not resource usage — an
admin scanning the fleet for "which instance is eating CPU" has to click
into each one's Resources tab. The data already streams live via the
global events SSE (`ResourcesTick` in `src/web/runtime.rs`) that
`useLiveSocket` already subscribes to elsewhere in the app.

- **Backend**: none — the tick already carries per-instance
  `cpu_percent`/`memory_bytes`/`players`.
- **Frontend**: add CPU/mem columns to `InstancesPage.tsx`'s table, sourced
  from the existing live-socket-fed query cache.
- **Effort**: S, frontend-only.

---

## Phase 2 — Operational depth

Full-stack. These are the "light ops" gaps that matter most for someone
keeping a community's server alive day to day — not cosmetic, but real
downtime/data-loss/visibility risk if left unaddressed.

### 2.1 Persist job history to SQLite

`JobRegistry` (`src/web/jobs.rs`) is in-memory only. Restarting
`odin serve` — a deploy, a crash, a host reboot — loses every job's
history, including whether last night's backup actually succeeded. An
admin diagnosing "did the backup job actually complete" after a routine
`odin` upgrade currently has no way to find out.

- **Backend**: add a `jobs` table via a new migration in
  `src/db/migrations/` (id, kind as JSON, status as JSON, started_at,
  finished_at, log lines — either inline JSON or a separate
  `job_log_lines` table). `JobRegistry::spawn`/`push_line`/`set_status`
  write to the DB alongside the existing in-memory update, mirroring
  `ActivityLog::record`'s buffer+persist+broadcast shape. Load recent job
  summaries from the DB on startup, the way `ActivityLog::load` already
  does. **Touches `src/web/jobs.rs` directly** — keep the in-memory
  broadcast/buffer behavior for live subscribers unchanged; this only adds
  durability underneath it.
- **Frontend**: none required for parity; worth extending `JobsPage.tsx`
  with longer history / pagination once the backend isn't capped by
  process lifetime.
- **Effort**: M, mostly backend.

### 2.2 Scheduled/automatic backups with retention

Backups are entirely manual today (`POST /instances/{name}/backups`). A
community admin running a persistent world wants "back up every night at
3am, keep the last 14" without hand-rolling a cron entry against a
deprecated CLI command.

- **Backend**: new `backup_schedules` table (instance_name, interval,
  retain_count, enabled) via migration. A background loop analogous to
  `spawn_telemetry` in `src/web/mod.rs` checks due schedules (e.g. every
  minute) and spawns backups through the *existing* job system
  (`JobRegistry::spawn` with `JobKindDescr::BackupCreate`) — not a new
  execution path. After a backup succeeds, prune beyond `retain_count` via
  the existing `backup::delete`, and record a new
  `ActivityKind::BackupPruned { backup_id }`. New
  `GET/PUT /instances/{name}/backup-schedule` routes. **Touches
  `src/activity.rs`** (new event kind) **and reuses `src/web/jobs.rs`**
  (spawns through the existing job kind, no new one needed).
- **Frontend**: a "Retention" section in `BackupsTab.tsx` (interval +
  keep-count fields, on/off toggle), following `ConfigTab.tsx`'s existing
  form-field patterns.
- **Effort**: L, full-stack.

### 2.3 Crash auto-restart (opt-in per instance)

The telemetry tick already *detects* unwitnessed process death
(`run_telemetry_tick` in `src/web/mod.rs:120-127`, which clears the stale
pid and emits `ActivityKind::InstanceStopped`) but never restarts it. For
an always-on community world, a crash (OOM, a bad mod) currently means the
server stays down until an admin in a different timezone happens to notice.

- **Backend**: add an `auto_restart` boolean to the `instances` table
  (migration) and to `InstanceView`/the config view. In the existing
  "pid persisted but not alive" branch of `run_telemetry_tick`, if
  `auto_restart` is set, call the same start path
  `routes/instances.rs::start_instance` uses, instead of only clearing the
  pid. Guard against restart-loops with a last-attempt timestamp (kept as
  operational state, not persisted config — e.g. in `RuntimeRegistry`, not
  the DB). Record a distinct `ActivityKind::InstanceAutoRestarted`, so the
  activity feed doesn't conflate "an admin started it" with "it crashed
  and came back on its own." **Directly touches `run_telemetry_tick`** —
  this is the correct single integration point, not a new poller.
- **Frontend**: a toggle in `ConfigTab.tsx` ("Restart automatically if the
  server crashes"); optionally a badge if an instance is currently in an
  auto-restart backoff state.
- **Effort**: M, full-stack — the backend hook point is narrow and
  well-defined.

### 2.4 Discord webhook alerting

The single highest-value item in this roadmap for a Valheim-guild
audience: a ping when a server crashes, a backup fails, or an update is
available — without having to SSH in and check the dashboard. Needs zero
new infrastructure: `crate::http::CLIENT` (`src/http.rs`, an existing
shared `reqwest` client) can already POST a Discord webhook payload
directly.

- **Backend**: new `webhooks` table (id, url, enabled, event-kind filter)
  via migration. New routes `GET/POST /webhooks`, `DELETE /webhooks/{id}`,
  and a `POST /webhooks/{id}/test`. A background task subscribes to
  `ActivityLog::subscribe()` at server startup — the *same* broadcast
  channel `/api/events/sse` already consumes — filters by each webhook's
  configured kinds, and POSTs a formatted payload. **Reuses
  `src/activity.rs`'s existing broadcast subscriber**, no second event bus.
  (This is also the natural home for the roadmap's one allowed
  auth-adjacent concept — an API token for *inbound* integrations, if a
  future bot/command integration needs one — flagged here for awareness,
  not scoped as part of this item.)
- **Frontend**: a new Settings-page section (see 3.2) or a dedicated
  `WebhooksPage.tsx` — list of configured webhooks, add/test/delete,
  checkboxes for which event kinds to forward.
- **Effort**: M, full-stack.

### 2.5 Bulk fleet operations

Multi-instance is core to the product, but every route today is
single-`{name}`. An admin restarting all instances before a shared
game-binary update, or stopping everything before a host reboot, currently
clicks through every instance page one at a time.

- **Backend**: no new concept — thin fan-out routes over the *existing*
  per-instance operations, each dispatched through `JobRegistry`.
  Candidates: `POST /instances/bulk/start`, `/stop`, `/restart`,
  `/mods/update-all` (body: an array of instance names, or `"all"`). May
  need an additive `BulkOperation { op, count }` `JobKindDescr` variant.
- **Frontend**: row-select checkboxes + a bulk action bar on
  `InstancesPage.tsx`'s table (Start selected / Stop selected / Update
  mods for selected).
- **Effort**: M, full-stack.

### 2.6 Metrics retention/export beyond the in-memory window

`RuntimeRegistry` caps at 120 samples (~6 minutes) per series, in memory
only. An admin investigating "was there a memory leak overnight," or
correlating a crash with a resource spike from hours earlier, has nothing
once it's scrolled past that window.

- **Backend**: periodically (e.g. every ~60th telemetry tick, ~3 min)
  downsample and persist into a `resource_samples` table (instance_name
  nullable for host-level, at, cpu_percent, memory_bytes) via a new
  migration, pruning rows older than a configurable retention window (e.g.
  7 days) on write. Keep the existing in-memory `RuntimeRegistry` as the
  fast path for the live chart — additive, not a replacement. Extend the
  existing `/history` routes to optionally read from SQLite for ranges
  beyond the in-memory window; add a CSV export route. **Touches
  `src/web/runtime.rs`** only to add a periodic persistence hook called
  from `run_telemetry_tick`, not a rewrite.
- **Frontend**: a time-range selector (1h/24h/7d) and an "Export CSV"
  button on `ResourcesTab.tsx`'s `ResourceChart`.
- **Effort**: L, full-stack — the retention-policy design is the fiddly
  part.

---

## Phase 3 — Admin quality-of-life

Frontend-forward, no urgent operational risk if skipped, but meaningfully
reduces day-to-day friction for someone running several servers for other
people.

### 3.1 Instances list search/filter/tags

`InstancesPage.tsx`'s table is flat with no search or sort — fine at 3
instances, painful at 15+ (a busy host running PvP/creative/modded/vanilla
worlds side by side).

- Ship client-side search/sort first (S, frontend-only). If tags/grouping
  are wanted beyond that, add a nullable `tags`/`group` column to
  `instances` (migration) plus a small `PATCH /instances/{name}/tags`
  route as a separate follow-up PR (M, full-stack).

### 3.2 Settings/preferences page

No settings surface exists at all today — `/doctor` and `/version` are
only ever shown on the Dashboard page, and there's nowhere for 2.4's
webhook config, default backup retention, or default instance settings to
live.

- **Backend**: mostly none new — this is a frontend aggregation point for
  existing and new routes. If global creation defaults are wanted, a small
  key/value `settings` table or an extension to `src/config.rs`.
- **Frontend**: new `web/src/pages/SettingsPage.tsx`, routed in
  `App.tsx`/navigation.
- **Effort**: M, mostly frontend.

### 3.3 Richer player info (SteamID, playtime)

`PlayerInfo` (`src/web/players.rs:26`) only has `name`/`connected_at`,
parsed from a `console.log` line that does **not** contain a SteamID
(only an in-game character name and internal peer id). An admin can't
currently cross-reference "which SteamID is this character" for moderation
without leaving the dashboard.

- **Research flag**: whether Valheim's console output ever surfaces a
  SteamID alongside a connection is genuinely uncertain — the module's own
  doc comment already describes the parsing as best-effort, not verified
  against a real console.log. Worth a short spike before committing to
  that half of this item.
- **Backend**: regardless of the SteamID outcome, add a `player_sessions`
  table (instance_name, name, steam_id nullable, joined_at, left_at)
  populated from the existing join/leave transitions
  (`ActivityKind::PlayerJoined`/`PlayerLeft`), giving a queryable playtime
  history via a new `GET /instances/{name}/players/history` route.
- **Frontend**: extend `PlayersTab.tsx` with a history view (past
  sessions, rough playtime) beyond the current live-only list.
- **Effort**: M–L, depending on the SteamID research outcome.

### 3.4 Mod version rollback (global-version scope only)

Real architectural limit, not just missing UI: mods are versioned once per
shared store entry (`PRODUCT.md`), so two instances cannot currently pin
two different versions of the same mod — `mods::update` moves every
linked instance together. True per-instance pinning would need dedup by
content-hash instead of mod-id, which is out of scope here.

- **Backend**: a `POST /instances/{name}/mods/{mod_id}/rollback` (target
  version) reusing the version-resolution path `mods::add` already uses,
  reverting the *shared/global* version — which affects every instance
  linking that mod, consistent with how updates already behave.
- **Frontend**: a version picker in `ModsTab.tsx`/`GlobalModsPage.tsx`,
  sourced from Thunderstore's already-available per-package version list.
- **Effort**: M. Explicitly **not** recommending true per-instance pinning
  without a separate, larger data-model design pass.

### 3.5 Mod update diff in the job log

`mods::update` (`src/mods/mod.rs:114`) already computes from/to versions
per mod but only logs them via `tracing`, not into the job's user-visible
log. An admin updating 20 mods at once currently can't see what changed
before players log in and something breaks.

- **Backend**: thread the existing from/to computation into the
  `JobLogger` output alongside the `tracing::info!` call — a small,
  low-risk change.
- **Frontend**: none needed — `JobProgress.tsx` already renders job log
  lines live.
- **Effort**: S, backend-lean.

### 3.6 World/save file browser

Backups are opaque zips of `saves/` today. An admin can't currently see
save file sizes/dates, or verify which world a backup corresponds to
before restoring over the live one, without downloading and unzipping it
manually.

- **Backend**: read-only routes only — `GET /instances/{name}/saves`
  (list files with size/mtime) and a single-file download route.
  Deliberately no upload/delete of individual save files, since that
  bypasses the backup/restore safety net (`backup::restore` snapshots
  before overwriting; ad hoc file edits wouldn't).
- **Frontend**: a new tab or section, list + download button, similar in
  shape to `ModConfigFiles.tsx`'s list/detail pattern.
- **Effort**: M, full-stack — backend is a small read-only fs listing.

---

## Phase 4 — Bigger bets (discretionary, not committed)

Flagged explicitly as optional/exploratory. Don't sequence these into
near-term planning without a separate scoping pass.

### 4.1 Generalized scheduled/scripted task system

Beyond scheduled backups (2.2) — scheduled restarts, scheduled mod
updates, scheduled announcements. Real demand beyond backups specifically
is unclear, and a generalized scheduler (cron parsing, conflict handling,
a scheduling UI) is meaningfully more design surface than the narrow,
well-motivated backup-retention case. Recommend treating 2.2 as the
concrete instance and only generalizing if a second concrete use case
(e.g. scheduled restarts) is explicitly requested later. If pursued: a
`scheduled_tasks` table + a generic dispatcher keyed by a small closed
`TaskKind` enum (mirroring `JobKindDescr`'s shape), reusing `JobRegistry`
for execution.

### 4.2 Cross-instance structured log search

Full-text/structured search over `console.log` across all instances
(e.g. "every crash-looking line across the fleet in the last 24h"), versus
today's per-instance tail. SQLite FTS5 could index log lines without a new
dependency, but ingesting every log line for every instance is a real
write-volume/storage-growth commitment — unlike the periodic downsampling
in 2.6 — that should be sized against actual log volume from a real
deployment before committing. If pursued: an FTS5 virtual table fed by the
existing log-tailer infrastructure, a `GET /logs/search` route, a new
dashboard page.

### 4.3 Console-command bridge to the running server (RCON-equivalent)

The ability to send admin commands (kick, save, broadcast) to a running
instance from the dashboard. **Flagged as research-only, not
implementation-ready.** `src/instance/process.rs`'s own doc comment states
plainly: *"the dedicated server has no admin/RCON protocol, so there's no
console input to wire up: stdin is `/dev/null`."* Valheim's dedicated
server binary does not expose a documented command/RCON interface the way
Minecraft or ARK do. Any viable path is not "wire up stdin" — there's
nothing listening on it — but one of:

- A BepInEx-mod-based bridge (a mod opening a local socket/HTTP port Odin
  could talk to) — feasible in principle since Odin already manages
  BepInEx installs, but makes a mod a *hard dependency* for a core
  dashboard feature, which cuts against mods being optional.
- Piping stdin and testing whether the vanilla binary accepts any commands
  at all — needs empirical verification; the existing code comment
  suggests this was already investigated and rejected, not merely
  unimplemented.

**Recommendation**: a spike/research task only, confirming or ruling out
both paths against a real running instance, before any implementation work
is scoped.

---

## Summary table

| # | Item | Phase | Effort | Stack | Touches shared plumbing |
|---|---|---|---|---|---|
| 1.1 | Wire up rename UI | Quick win | S | Frontend | — |
| 1.2 | Granular access-list edit | Quick win | S | Frontend | — |
| 1.3 | Delete from instances list | Quick win | S | Frontend | — |
| 1.4 | `keep_backups` in delete UI | Quick win | S | Frontend | — |
| 1.5 | Inline CPU/mem in instances table | Quick win | S | Frontend | — |
| 2.1 | Persist job history to SQLite | Op depth | M | Backend | `jobs.rs` |
| 2.2 | Scheduled backups + retention | Op depth | L | Full-stack | `activity.rs`, `jobs.rs` |
| 2.3 | Crash auto-restart | Op depth | M | Full-stack | `activity.rs`, `web/mod.rs` tick |
| 2.4 | Discord webhook alerting | Op depth | M | Full-stack | `activity.rs` (subscriber) |
| 2.5 | Bulk fleet operations | Op depth | M | Full-stack | `jobs.rs` |
| 2.6 | Metrics retention/export | Op depth | L | Full-stack | `runtime.rs` |
| 3.1 | Instances search/tags | QoL | S then M | Frontend then full-stack | — |
| 3.2 | Settings page | QoL | M | Mostly frontend | — |
| 3.3 | Richer player info | QoL | M–L | Full-stack | (research flag) |
| 3.4 | Mod rollback (global-version scope) | QoL | M | Full-stack | `activity.rs` |
| 3.5 | Mod update diff in job log | QoL | S | Backend-lean | `jobs.rs` (log content only) |
| 3.6 | World/save file browser | QoL | M | Full-stack | — |
| 4.1 | Generalized scheduler | Bigger bet | — | — | discretionary |
| 4.2 | Cross-instance log search | Bigger bet | — | — | discretionary |
| 4.3 | Console/RCON bridge | Bigger bet | — | — | research only |

## Suggested sequencing

Phase 1 in full first — five small, low-risk, immediately visible PRs that
also serve as a warm-up on the exact patterns Phase 2 reuses (confirm
dialogs, hooks, live-socket-fed table columns). Then from Phase 2, **2.1
(job persistence)** before **2.2 (scheduled backups)**, since scheduled
backups are far less trustworthy without durable job history to confirm
they actually ran. **2.4 (Discord webhooks)** can land independently of
the rest of Phase 2 at any point once 2.1 or 2.3 give it something worth
alerting on. Phase 3 and 4 are not time-sensitive and can be picked up
opportunistically or reprioritized based on real admin feedback once
Phase 1–2 ship.
