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
  )
}
