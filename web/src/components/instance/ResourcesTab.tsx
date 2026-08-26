import { QueryError } from '@/components/QueryError'
import { ResourceChart } from '@/components/ResourceChart'
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
    return <p className="text-sm text-muted-foreground">Loading…</p>
  }

  return (
    <div className="flex max-w-sm flex-col gap-4 text-sm">
      <div className="flex flex-col gap-1 rounded-md border p-3">
        <div className="flex items-center justify-between">
          <span className="text-muted-foreground">CPU</span>
          <span>{resources.data.cpu_percent.toFixed(1)}%</span>
        </div>
        <ResourceChart
          data={history.data ?? []}
          dataKey="cpu_percent"
          formatValue={(v) => `${v.toFixed(1)}%`}
        />
      </div>
      <div className="flex flex-col gap-1 rounded-md border p-3">
        <div className="flex items-center justify-between">
          <span className="text-muted-foreground">Memory</span>
          <span>{formatBytes(resources.data.memory_bytes)}</span>
        </div>
        <ResourceChart data={history.data ?? []} dataKey="memory_bytes" formatValue={formatBytes} />
      </div>
    </div>
  )
}
