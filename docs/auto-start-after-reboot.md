# Auto-starting instances after a host reboot

## Problem

`auto_restart` currently covers an unexpected Valheim or supervisor exit. The
dashboard telemetry loop notices a dead process only when the database still
contains its old PID. That can happen after a VPS reboot, but it is an
implementation side effect rather than a durable boot policy. A clean
shutdown may clear the PID, and an intentionally stopped instance must not be
started merely because `auto_restart` remains enabled.

The boot policy therefore needs to persist operator intent separately from
observed process state.

## Proposed state model

Add one non-user-editable column to `instances`:

```sql
desired_running INTEGER NOT NULL DEFAULT 0
```

The relevant states become:

| `auto_restart` | `desired_running` | Observed process | Result |
|---|---|---|---|
| false | any | stopped | Remain stopped |
| true | false | stopped | Remain stopped; the operator stopped it |
| true | true | running | No action |
| true | true | stopped | Start or retry after the cooldown |

`pid` and `pid_started_at` remain observations, never desired state.

## State transitions

- A successful operator start sets `desired_running = true`.
- An operator stop sets `desired_running = false` before signalling the
  supervisor. This prevents a crash-restart race while the stop is underway.
- Restart preserves `desired_running = true`; its internal stop phase must not
  look like an operator stop.
- Enabling `auto_restart` on an already-running instance sets
  `desired_running = true`.
- Disabling `auto_restart` leaves the process alone and makes
  `desired_running` irrelevant. It can be cleared for simpler inspection.
- A host/container shutdown may stop Valheim cleanly, but must preserve
  `desired_running`. It therefore needs a shutdown-specific lifecycle path,
  distinct from the operator-facing stop action.
- Deleting an instance removes the state through the existing cascade.

The lifecycle API should express the difference with an internal enum such as
`StopReason::Operator` and `StopReason::HostShutdown`, not an unexplained
boolean parameter.

## Startup reconciliation

When `odin serve` starts:

1. Open and migrate the database.
2. Build `AppState` and start the normal event infrastructure.
3. Run a background startup reconciler.
4. List instances where `auto_restart && desired_running`.
5. Check their live PID fingerprints.
6. For each stopped instance, remove stale supervisor sockets/pidfiles and
   call the existing locked `instance::lifecycle::start` path.
7. Start instances sequentially so several Valheim processes do not all
   contend for CPU, memory, and disk during boot.
8. Record a distinct boot-start activity event and log failures.

The HTTP listener should not wait for Valheim startup. The dashboard must be
available while reconciliation is progressing.

After this one-time pass, the telemetry loop should use the same predicate:

```text
auto_restart && desired_running && !observed_running
```

The existing per-instance cooldown then provides retry behavior if an
instance cannot start. This removes the current dependency on `pid.is_some()`
and unifies crash recovery with reboot recovery.

The detached supervisor must also check both `auto_restart` and
`desired_running` before respawning its child.

## Migration

For an upgrade from the current schema:

```sql
desired_running = auto_restart AND pid IS NOT NULL
```

An instance that was running immediately before the upgrade keeps its intent.
An intentionally stopped instance has no PID and remains stopped. The existing
PID/start-time validation still protects against PID reuse after reboot.

## Packaging implications

The packaged `odin.service` is already enabled for `multi-user.target`, waits
for `network-online.target`, and uses `KillMode=process` so a dashboard restart
does not kill detached supervisors. No new systemd unit per instance is
needed.

The container path currently uses `ODIN_STOP_INSTANCES_ON_SHUTDOWN=1`. Its
shutdown handler must call the host-shutdown stop path so worlds are saved
without clearing `desired_running`; recreating the container will then restore
the previously desired instances.

## Verification

- Migration marks only previously running, auto-restart instances as desired.
- Operator stop prevents a later telemetry or boot restart.
- Operator restart preserves desired state.
- Graceful host/container shutdown preserves desired state.
- Startup reconciliation ignores running, disabled, and intentionally stopped
  instances.
- Startup failures obey the existing cooldown and do not form a tight loop.
- A full reboot integration test confirms that the dashboard returns first and
  desired instances subsequently become ready.
