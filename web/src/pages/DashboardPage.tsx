import { useQueryClient } from '@tanstack/react-query'
import { AlertTriangle, CheckCircle2, Loader2, XCircle } from 'lucide-react'
import { useEffect, useRef } from 'react'
import { Link } from 'react-router-dom'
import { toast } from 'sonner'
import { PageHeader } from '@/components/PageHeader'
import { QueryError } from '@/components/QueryError'
import { ResourceMetric, ResourceMetricSkeleton } from '@/components/ResourceMetric'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { ACTIVITY_ICONS, describeActivity } from '@/lib/activity'
import { describeJobKind, jobStatusVariant } from '@/lib/jobs'
import {
  useActivityFeed,
  useDoctor,
  useHostResourceHistory,
  useHostResources,
  useInstallServer,
  useInstallStatus,
  useJobs,
} from '@/lib/queries'
import type { CheckResult } from '@/lib/types'
import { formatBytes, formatRelativeTime } from '@/lib/utils'

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
  const history = useHostResourceHistory()
  const installServer = useInstallServer()
  const installStatus = useInstallStatus()
  const jobs = useJobs()
  const activity = useActivityFeed()
  const queryClient = useQueryClient()

  const runningInstallJob = jobs.data?.find(
    (j) =>
      j.kind.kind === 'steamcmd_install' &&
      (j.status.status === 'queued' || j.status.status === 'running'),
  )

  // Force one refetch as soon as an install/update job finishes, so the
  // "update available" badge doesn't wait for its own poll interval.
  const wasRunningInstallJob = useRef(false)
  useEffect(() => {
    if (wasRunningInstallJob.current && !runningInstallJob) {
      queryClient.invalidateQueries({ queryKey: ['install-status'] })
      queryClient.invalidateQueries({ queryKey: ['doctor'] })
    }
    wasRunningInstallJob.current = Boolean(runningInstallJob)
  }, [runningInstallJob, queryClient])

  const showInstallButton = !installStatus.data || !installStatus.data.installed || installStatus.data.update_available

  return (
    <div className="flex flex-col gap-6">
      <PageHeader title="Dashboard" description="Environment status and host resources." />

      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <Card>
          <CardHeader className="flex-row items-center justify-between space-y-0">
            <CardTitle className="text-base">Dependency status</CardTitle>
            {showInstallButton && (
              <Button
                size="sm"
                disabled={installServer.isPending || Boolean(runningInstallJob)}
                onClick={() =>
                  installServer.mutate(undefined, {
                    onError: (e) => toast.error(e.message),
                  })
                }
              >
                {installServer.isPending || runningInstallJob ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : null}
                Install / update server
              </Button>
            )}
          </CardHeader>
          <CardContent>
            {doctor.isLoading && (
              <div className="flex flex-col gap-2 py-2">
                <Skeleton className="h-4 w-full" />
                <Skeleton className="h-4 w-full" />
                <Skeleton className="h-4 w-2/3" />
              </div>
            )}
            {doctor.isError && <QueryError error={doctor.error} />}
            {doctor.data?.map((check) => <CheckRow key={check.label} check={check} />)}
            {installStatus.isError && <QueryError error={installStatus.error} />}
            {installStatus.data && (
              <div className="flex items-center justify-between py-2">
                <span className="text-sm">Valheim server version</span>
                {!installStatus.data.installed ? (
                  <Badge variant="outline">Not installed</Badge>
                ) : installStatus.data.update_available ? (
                  <Badge variant="secondary">
                    Update available: {installStatus.data.installed_build_id} →{' '}
                    {installStatus.data.latest_build_id}
                  </Badge>
                ) : (
                  <Badge variant="outline">Up to date (build {installStatus.data.installed_build_id})</Badge>
                )}
              </div>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">Host resources</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-4 text-sm">
            {resources.isError && <QueryError error={resources.error} />}
            {resources.data ? (
              <>
                <ResourceMetric
                  label="CPU"
                  value={`${resources.data.cpu_percent.toFixed(1)}%`}
                  history={history.data ?? []}
                  dataKey="cpu_percent"
                  formatValue={(v) => `${v.toFixed(1)}%`}
                />
                <ResourceMetric
                  label="Memory"
                  value={`${formatBytes(resources.data.memory_used_bytes)} / ${formatBytes(resources.data.memory_total_bytes)}`}
                  history={history.data ?? []}
                  dataKey="memory_bytes"
                  formatValue={formatBytes}
                />
                <div className="flex items-center justify-between">
                  <span className="text-muted-foreground">Disk free</span>
                  <span>
                    {formatBytes(resources.data.disk_available_bytes)} /{' '}
                    {formatBytes(resources.data.disk_total_bytes)}
                  </span>
                </div>
              </>
            ) : (
              <>
                <ResourceMetricSkeleton />
                <ResourceMetricSkeleton />
              </>
            )}
          </CardContent>
        </Card>

        {jobs.data && jobs.data.length > 0 && (
          <Card>
            <CardHeader className="flex flex-row items-center justify-between">
              <CardTitle className="text-base">Recent jobs</CardTitle>
              <Link to="/jobs" className="text-xs text-muted-foreground hover:underline">
                View all
              </Link>
            </CardHeader>
            <CardContent className="flex flex-col gap-2">
              {jobs.data.slice(0, 5).map((job) => (
                <div key={job.id} className="flex items-center justify-between text-sm">
                  <span>{describeJobKind(job.kind)}</span>
                  <Badge variant={jobStatusVariant(job.status)}>{job.status.status}</Badge>
                </div>
              ))}
            </CardContent>
          </Card>
        )}

        {activity.data.length > 0 && (
          <Card>
            <CardHeader>
              <CardTitle className="text-base">Live activity</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-2">
              {activity.data.slice(0, 6).map((event) => {
                const Icon = ACTIVITY_ICONS[event.kind.kind]
                return (
                  <div key={event.id} className="flex items-center gap-2 text-sm">
                    <Icon className="size-4 shrink-0 text-muted-foreground" />
                    <span className="min-w-0 flex-1 truncate">{describeActivity(event.kind, event.game)}</span>
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {formatRelativeTime(event.at)}
                    </span>
                  </div>
                )
              })}
            </CardContent>
          </Card>
        )}
      </div>
    </div>
  )
}
