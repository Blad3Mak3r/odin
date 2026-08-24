import { useInstanceResources } from '@/lib/queries'

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  return `${(bytes / 1024 ** exponent).toFixed(1)} ${units[exponent]}`
}

export function ResourcesTab({ name, running }: { name: string; running: boolean }) {
  const resources = useInstanceResources(name, running)

  if (!running) {
    return <p className="text-sm text-muted-foreground">Instance is stopped — nothing to measure.</p>
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
