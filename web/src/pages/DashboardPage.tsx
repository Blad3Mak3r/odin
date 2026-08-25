import { AlertTriangle, CheckCircle2, Loader2, XCircle } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { useDoctor, useHostResources, useInstallServer, useJobs } from '@/lib/queries'
import type { CheckResult } from '@/lib/types'
import { formatBytes } from '@/lib/utils'

function CheckRow({ check }: { check: CheckResult }) {
  const Icon = check.ok ? CheckCircle2 : check.critical ? XCircle : AlertTriangle
  const color = check.ok ? 'text-emerald-500' : check.critical ? 'text-destructive' : 'text-amber-500'
  return (
    <div className="flex items-center justify-between border-b py-2 last:border-b-0">
      <div className="flex items-center gap-2">
        <Icon className={`size-4 ${color}`} />
        <span className="text-sm">{check.label}</span>
      </div>
      {check.detail && <span className="text-xs text-muted-foreground">{check.detail}</span>}
    </div>
  )
}

export function DashboardPage() {
  const doctor = useDoctor()
  const resources = useHostResources()
  const installServer = useInstallServer()
  const jobs = useJobs()

  const installNeeded = doctor.data?.some(
    (c) => c.label === 'Valheim dedicated server installed' && !c.ok,
  )
  const runningJob = jobs.data?.find(
    (j) => j.status.status === 'queued' || j.status.status === 'running',
  )

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Dashboard</h1>
        <p className="text-sm text-muted-foreground">Environment status and host resources.</p>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader className="flex-row items-center justify-between space-y-0">
            <CardTitle className="text-base">Dependency status</CardTitle>
            {installNeeded && (
              <Button
                size="sm"
                disabled={installServer.isPending || Boolean(runningJob)}
                onClick={() => installServer.mutate()}
              >
                {installServer.isPending || runningJob ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : null}
                Install / update server
              </Button>
            )}
          </CardHeader>
          <CardContent>
            {doctor.isLoading && <p className="text-sm text-muted-foreground">Loading…</p>}
            {doctor.data?.map((check) => <CheckRow key={check.label} check={check} />)}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">Host resources</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 text-sm">
            {resources.data ? (
              <>
                <div className="flex items-center justify-between">
                  <span className="text-muted-foreground">CPU</span>
                  <span>{resources.data.cpu_percent.toFixed(1)}%</span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-muted-foreground">Memory</span>
                  <span>
                    {formatBytes(resources.data.memory_used_bytes)} /{' '}
                    {formatBytes(resources.data.memory_total_bytes)}
                  </span>
                </div>
                <div className="flex items-center justify-between">
                  <span className="text-muted-foreground">Disk free</span>
                  <span>
                    {formatBytes(resources.data.disk_available_bytes)} /{' '}
                    {formatBytes(resources.data.disk_total_bytes)}
                  </span>
                </div>
              </>
            ) : (
              <p className="text-muted-foreground">Loading…</p>
            )}
          </CardContent>
        </Card>
      </div>

      {jobs.data && jobs.data.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">Recent jobs</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-2">
            {jobs.data
              .slice()
              .reverse()
              .slice(0, 5)
              .map((job) => (
                <div key={job.id} className="flex items-center justify-between text-sm">
                  <span>{describeJobKind(job.kind)}</span>
                  <Badge variant={job.status.status === 'failed' ? 'destructive' : 'secondary'}>
                    {job.status.status}
                  </Badge>
                </div>
              ))}
          </CardContent>
        </Card>
      )}
    </div>
  )
}

function describeJobKind(kind: { kind: string; instance?: string; mod_id?: string }): string {
  switch (kind.kind) {
    case 'steamcmd_install':
      return 'Install/update server files'
    case 'mod_add':
      return `Install mod ${kind.mod_id} on ${kind.instance}`
    case 'mod_update':
      return `Update mods on ${kind.instance}`
    default:
      return kind.kind
  }
}
