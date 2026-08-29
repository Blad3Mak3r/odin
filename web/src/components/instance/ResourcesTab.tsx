import { useState } from 'react'
import { QueryError } from '@/components/QueryError'
import { ResourceMetric, ResourceMetricSkeleton } from '@/components/ResourceMetric'
import { Button, buttonVariants } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { useInstanceResourceHistory, useInstanceResources, useLastSaved } from '@/lib/queries'
import { cn, formatBytes, formatRelativeTime } from '@/lib/utils'

const RANGES = [
  { label: 'Live', hours: undefined },
  { label: '1h', hours: 1 },
  { label: '24h', hours: 24 },
  { label: '7d', hours: 24 * 7 },
] as const

export function ResourcesTab({ name, running }: { name: string; running: boolean }) {
  const [hours, setHours] = useState<number | undefined>(undefined)
  const resources = useInstanceResources(name, running)
  const history = useInstanceResourceHistory(name, hours, running)
  const lastSaved = useLastSaved(name)

  if (!running) {
    return <p className="text-sm text-muted-foreground">Instance is stopped — nothing to measure.</p>
  }

  if (resources.isError) {
    return <QueryError error={resources.error} />
  }

  if (resources.isLoading || !resources.data) {
    return (
      <div className="grid gap-4 sm:grid-cols-2">
        <Card>
          <CardContent className="text-sm">
            <ResourceMetricSkeleton />
          </CardContent>
        </Card>
        <Card>
          <CardContent className="text-sm">
            <ResourceMetricSkeleton />
          </CardContent>
        </Card>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex gap-1">
          {RANGES.map((r) => (
            <Button
              key={r.label}
              size="sm"
              variant={hours === r.hours ? 'default' : 'outline'}
              onClick={() => setHours(r.hours)}
            >
              {r.label}
            </Button>
          ))}
        </div>
        <a
          href={`/api/instances/${name}/resources/history/export${hours ? `?hours=${hours}` : ''}`}
          download
          className={cn(buttonVariants({ variant: 'outline', size: 'sm' }))}
        >
          Export CSV
        </a>
      </div>
      {lastSaved.data && (
        <p className="text-sm text-muted-foreground">
          World last saved {formatRelativeTime(lastSaved.data)}
        </p>
      )}
      <div className="grid gap-4 sm:grid-cols-2">
        <Card>
          <CardContent className="text-sm">
            <ResourceMetric
              label="CPU"
              value={`${resources.data.cpu_percent.toFixed(1)}%`}
              history={history.data ?? []}
              dataKey="cpu_percent"
              formatValue={(v) => `${v.toFixed(1)}%`}
            />
          </CardContent>
        </Card>
        <Card>
          <CardContent className="text-sm">
            <ResourceMetric
              label="Memory"
              value={formatBytes(resources.data.memory_bytes)}
              history={history.data ?? []}
              dataKey="memory_bytes"
              formatValue={formatBytes}
            />
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
