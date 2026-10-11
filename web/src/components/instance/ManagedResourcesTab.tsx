import { useState } from 'react'
import { QueryError } from '@/components/QueryError'
import { ResourceMetric, ResourceMetricSkeleton } from '@/components/ResourceMetric'
import { Button } from '@/components/ui/button'
import { buttonVariants } from '@/components/ui/button-variants'
import { Card, CardContent } from '@/components/ui/card'
import { useManagedResourceHistory, useManagedResources } from '@/lib/queries'
import { cn, formatBytes } from '@/lib/utils'

const RANGES = [
  { label: 'Live', hours: undefined },
  { label: '1h', hours: 1 },
  { label: '24h', hours: 24 },
  { label: '7d', hours: 24 * 7 },
] as const

export function ManagedResourcesTab({ id, running }: { id: string; running: boolean }) {
  const [hours, setHours] = useState<number | undefined>()
  const resources = useManagedResources(id, running)
  const history = useManagedResourceHistory(id, hours, running)

  if (!running) return <p className="text-sm text-muted-foreground">Instance is stopped — nothing to measure.</p>
  if (resources.isError) return <QueryError error={resources.error} />
  if (resources.isLoading || !resources.data) {
    return (
      <div className="grid gap-4 sm:grid-cols-2">
        {[0, 1].map((index) => <Card key={index}><CardContent className="text-sm"><ResourceMetricSkeleton /></CardContent></Card>)}
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex gap-1">
          {RANGES.map((range) => (
            <Button key={range.label} size="sm" variant={hours === range.hours ? 'default' : 'outline'} onClick={() => setHours(range.hours)}>
              {range.label}
            </Button>
          ))}
        </div>
        <a
          href={`/api/instances/${id}/resources/history/export${hours ? `?hours=${hours}` : ''}`}
          download
          className={cn(buttonVariants({ variant: 'outline', size: 'sm' }))}
        >
          Export CSV
        </a>
      </div>
      {history.isError && <QueryError error={history.error} />}
      <div className="grid gap-4 sm:grid-cols-2">
        <Card><CardContent className="text-sm"><ResourceMetric label="CPU" value={`${resources.data.cpu_percent.toFixed(1)}%`} history={history.data ?? []} dataKey="cpu_percent" formatValue={(value) => `${value.toFixed(1)}%`} /></CardContent></Card>
        <Card><CardContent className="text-sm"><ResourceMetric label="Memory" value={formatBytes(resources.data.memory_bytes)} history={history.data ?? []} dataKey="memory_bytes" formatValue={formatBytes} /></CardContent></Card>
      </div>
    </div>
  )
}
