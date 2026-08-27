import { QueryError } from '@/components/QueryError'
import { ResourceMetric, ResourceMetricSkeleton } from '@/components/ResourceMetric'
import { Card, CardContent } from '@/components/ui/card'
import { useInstanceResourceHistory, useInstanceResources } from '@/lib/queries'
import { formatBytes } from '@/lib/utils'

export function ResourcesTab({ name, running }: { name: string; running: boolean }) {
  const resources = useInstanceResources(name, running)
  const history = useInstanceResourceHistory(name, running)

  if (!running) {
    return <p className="text-sm text-muted-foreground">Instance is stopped — nothing to measure.</p>
  }

  if (resources.isError) {
    return <QueryError error={resources.error} />
  }

  return (
    <Card className="max-w-sm">
      <CardContent className="flex flex-col gap-4 text-sm">
        {resources.isLoading || !resources.data ? (
          <>
            <ResourceMetricSkeleton />
            <ResourceMetricSkeleton />
          </>
        ) : (
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
              value={formatBytes(resources.data.memory_bytes)}
              history={history.data ?? []}
              dataKey="memory_bytes"
              formatValue={formatBytes}
            />
          </>
        )}
      </CardContent>
    </Card>
  )
}
