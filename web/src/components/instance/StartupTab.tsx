import { CheckCircle2, Loader2, XCircle } from 'lucide-react'
import { LiveLogOutput } from '@/components/instance/LiveLogOutput'
import { QueryError } from '@/components/QueryError'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { useStartupReport } from '@/lib/queries'
import type { StartupAttempt, StartupEvent, StartupLastRun, StartupPhase } from '@/lib/types'

const PHASE_LABELS: Record<StartupPhase, string> = {
  preflight: 'Preflight checks',
  image: 'Runtime image',
  container: 'Container',
  runtime: 'Odin runtime',
  game_ready: 'Game readiness',
}

function formatTime(value: string) {
  return new Date(value).toLocaleTimeString()
}

function duration(attempt: StartupAttempt) {
  if (!attempt.finished_at) return null
  const seconds = (Date.parse(attempt.finished_at) - Date.parse(attempt.started_at)) / 1000
  return seconds < 10 ? `${seconds.toFixed(1)}s` : `${Math.round(seconds)}s`
}

function OutcomeBadge({ outcome }: { outcome: StartupAttempt['outcome'] }) {
  if (outcome === 'running') return <Badge variant="secondary"><Loader2 className="size-3 animate-spin" />Starting</Badge>
  return <Badge variant={outcome === 'failed' ? 'destructive' : 'default'}>{outcome === 'failed' ? 'Failed' : 'Started'}</Badge>
}

function EventIcon({ status }: { status: StartupEvent['status'] }) {
  if (status === 'running') return <Loader2 className="mt-0.5 size-4 shrink-0 animate-spin text-muted-foreground" aria-label="In progress" />
  if (status === 'failed') return <XCircle className="mt-0.5 size-4 shrink-0 text-destructive" aria-label="Failed" />
  return <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-label="Done" />
}

function Timeline({ events }: { events: StartupEvent[] }) {
  if (events.length === 0) return <p className="text-sm text-muted-foreground">No steps were recorded.</p>
  return (
    <ol className="flex flex-col gap-2">
      {events.map((event, index) => (
        <li key={index} className="flex gap-2 text-sm">
          <EventIcon status={event.status} />
          <div className="min-w-0">
            <span className="font-medium">{PHASE_LABELS[event.phase]}</span>
            <span className="ml-2 text-xs text-muted-foreground tabular-nums">{formatTime(event.at)}</span>
            {event.detail && <p className="break-words text-muted-foreground">{event.detail}</p>}
          </div>
        </li>
      ))}
    </ol>
  )
}

function FailureAlert({ attempt }: { attempt: StartupAttempt }) {
  const failure = attempt.failure
  if (!failure) return null
  return (
    <Alert variant="destructive">
      <XCircle />
      <AlertTitle>Failed during: {PHASE_LABELS[failure.phase]}</AlertTitle>
      <AlertDescription className="flex flex-col gap-2">
        <span className="break-words">{failure.reason}</span>
        {(failure.exit_code !== null || failure.oom_killed) && (
          <span className="flex flex-wrap gap-2">
            {failure.exit_code !== null && <Badge variant="outline">Exit code {failure.exit_code}</Badge>}
            {failure.oom_killed && <Badge variant="outline">Out of memory</Badge>}
          </span>
        )}
        {failure.hint && <span className="text-foreground">{failure.hint}</span>}
        {failure.log_tail.length > 0 && (
          <LiveLogOutput lines={failure.log_tail} emptyMessage="" />
        )}
      </AlertDescription>
    </Alert>
  )
}

function LogSection({ title, lines }: { title: string; lines: string[] }) {
  return (
    <details className="rounded-md border p-3">
      <summary className="cursor-pointer text-sm font-medium">{title} ({lines.length} lines)</summary>
      <div className="mt-3">
        <LiveLogOutput lines={lines} emptyMessage="Nothing was logged." />
      </div>
    </details>
  )
}

function LastRun({ run }: { run: StartupLastRun }) {
  return (
    <section className="flex flex-col gap-3">
      <h3 className="text-sm font-medium">Last container run</h3>
      <div className="flex flex-wrap gap-2">
        <Badge variant={run.exit_code === 0 ? 'secondary' : 'destructive'}>
          {run.exit_code === null ? 'No exit code' : `Exit code ${run.exit_code}`}
        </Badge>
        {run.oom_killed && <Badge variant="destructive">Out of memory</Badge>}
        {run.finished_at && <Badge variant="outline">Ended {new Date(run.finished_at).toLocaleString()}</Badge>}
      </div>
      {run.error && <p className="break-words text-sm text-destructive">{run.error}</p>}
      <LogSection title="Container output" lines={run.container_log_tail} />
      <LogSection title="Install log (SteamCMD)" lines={run.install_log_tail} />
      <LogSection title="Game console" lines={run.console_log_tail} />
    </section>
  )
}

function EarlierAttempts({ attempts }: { attempts: StartupAttempt[] }) {
  if (attempts.length === 0) return null
  return (
    <details className="rounded-md border p-3">
      <summary className="cursor-pointer text-sm font-medium">Earlier attempts ({attempts.length})</summary>
      <ul className="mt-3 flex flex-col gap-3">
        {attempts.map((attempt) => (
          <li key={attempt.id} className="flex flex-col gap-1 text-sm">
            <div className="flex flex-wrap items-center gap-2">
              <OutcomeBadge outcome={attempt.outcome} />
              <span className="text-xs text-muted-foreground tabular-nums">{new Date(attempt.started_at).toLocaleString()}</span>
            </div>
            {attempt.failure && <p className="break-words text-muted-foreground">{PHASE_LABELS[attempt.failure.phase]}: {attempt.failure.reason}</p>}
          </li>
        ))}
      </ul>
    </details>
  )
}

export function StartupTab({ id }: { id: string }) {
  const report = useStartupReport(id)
  const [latest, ...earlier] = report.data?.attempts ?? []

  return (
    <Card>
      <CardHeader>
        <CardTitle>Startup</CardTitle>
        <CardDescription>
          Every step of the latest start and, when it fails, why. Odin keeps the last 10 attempts.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {report.isError && <QueryError error={report.error} />}
        {report.isPending && <Skeleton className="h-24" />}
        {report.data && !latest && !report.data.last_run && (
          <p className="text-sm text-muted-foreground">No start has been recorded for this instance yet.</p>
        )}
        {latest && (
          <section className="flex flex-col gap-3">
            <div className="flex flex-wrap items-center gap-2">
              <OutcomeBadge outcome={latest.outcome} />
              <span className="text-sm text-muted-foreground tabular-nums">
                {new Date(latest.started_at).toLocaleString()}
                {duration(latest) && ` · took ${duration(latest)}`}
              </span>
            </div>
            <FailureAlert attempt={latest} />
            <Timeline events={latest.events} />
          </section>
        )}
        {report.data?.last_run && <LastRun run={report.data.last_run} />}
        <EarlierAttempts attempts={earlier} />
      </CardContent>
    </Card>
  )
}
