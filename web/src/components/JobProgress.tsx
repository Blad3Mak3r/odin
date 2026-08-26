import { Loader2 } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import type { JobStatus } from '@/lib/types'

export function JobProgress({
  log,
  status,
  connected,
  logHeightClassName = 'max-h-32',
}: {
  log: string[]
  status: JobStatus | null
  connected: boolean
  logHeightClassName?: string
}) {
  const isActive = status?.status === 'running' || status?.status === 'queued'
  const connectionLost = !connected && isActive

  return (
    <div className="rounded-md border bg-muted/30 p-3">
      <div className="mb-2 flex items-center gap-2">
        {isActive && !connectionLost ? <Loader2 className="size-4 animate-spin" /> : null}
        <Badge
          variant={
            connectionLost ? 'destructive' : status?.status === 'failed' ? 'destructive' : 'secondary'
          }
        >
          {connectionLost ? 'connection lost' : (status?.status ?? 'starting')}
        </Badge>
        {status?.status === 'failed' && (
          <span className="text-xs text-destructive">{status.message}</span>
        )}
      </div>
      <div className={`${logHeightClassName} overflow-y-auto font-mono text-xs`}>
        {log.map((line, i) => (
          // Job log lines have no stable id and never reorder, only append.
          // eslint-disable-next-line react/no-array-index-key
          <div key={i}>{line}</div>
        ))}
      </div>
    </div>
  )
}
