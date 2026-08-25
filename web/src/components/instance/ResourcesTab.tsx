import { QueryError } from '@/components/QueryError'
import { useInstanceResources } from '@/lib/queries'
import { formatBytes } from '@/lib/utils'

export function ResourcesTab({ name, running }: { name: string; running: boolean }) {
  const resources = useInstanceResources(name, running)

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
    <div className="flex max-w-sm flex-col gap-3 text-sm">
      <div className="flex items-center justify-between rounded-md border p-3">
        <span className="text-muted-foreground">CPU</span>
        <span>{resources.data.cpu_percent.toFixed(1)}%</span>
      </div>
      <div className="flex items-center justify-between rounded-md border p-3">
        <span className="text-muted-foreground">Memory</span>
        <span>{formatBytes(resources.data.memory_bytes)}</span>
      </div>
    </div>
  )
}
