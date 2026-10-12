import { Loader2, RefreshCw, Wrench } from 'lucide-react'
import { toast } from 'sonner'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useRepairRuntime, useRuntimeHealth } from '@/lib/queries'
import { gameLabel } from '@/lib/games'
import type { GameId } from '@/lib/types'

function label(value: string) {
  return value.replaceAll('_', ' ')
}

function statusVariant(status: string) {
  return status === 'responding' || status === 'running' ? 'default' : status === 'missing' || status === 'created' || status === 'exited' || status === 'orphaned' ? 'secondary' : 'destructive'
}

export function RuntimeHealthCard({ id, game, running }: { id: string; game: GameId; running: boolean }) {
  const health = useRuntimeHealth(id)
  const repair = useRepairRuntime()
  const data = health.data
  const repairable = !running && data?.podman.compatible === true && ['missing', 'created', 'exited'].includes(data.container.status)

  return (
    <Card>
      <CardHeader>
        <CardTitle>Runtime health</CardTitle>
        <CardDescription>Podman container and Odin runtime status for this instance.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {health.isError && <QueryError error={health.error} />}
        {data && (
          <>
            <div className="flex flex-wrap gap-2">
              <Badge variant={data.podman.compatible ? 'default' : 'destructive'}>
                Podman {data.podman.compatible ? 'ready' : 'unavailable'}
              </Badge>
              <Badge variant={statusVariant(data.container.status)}>
                Container: {label(data.container.status)}
              </Badge>
              <Badge variant={statusVariant(data.socket.status)}>
                Runtime socket: {label(data.socket.status)}
              </Badge>
            </div>
            <div className="grid gap-2 text-sm text-muted-foreground sm:grid-cols-2">
              <span>Podman version: {data.podman.version ?? '—'}</span>
              <span>Runtime Odin: {data.socket.odin_version ?? '—'}</span>
              <span>Runtime protocol: {data.socket.protocol_version ?? '—'}</span>
            </div>
            {[data.podman.error, data.container.error, data.socket.error].filter((message): message is string => Boolean(message)).map((message) => (
              <p key={message} className="text-sm text-destructive">{message}</p>
            ))}
            <ul className="list-disc space-y-1 pl-5 text-sm text-muted-foreground">
              {data.guidance.map((message) => <li key={message}>{message}</li>)}
            </ul>
          </>
        )}
        <div className="flex flex-wrap gap-2">
          <Button type="button" variant="outline" size="sm" onClick={() => health.refetch()} disabled={health.isFetching}>
            {health.isFetching ? <Loader2 className="size-4 animate-spin" /> : <RefreshCw className="size-4" />}
            Refresh
          </Button>
          {!running && data && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={!repairable || repair.isPending}
              onClick={() => repair.mutate(id, {
                onSuccess: () => toast.success(`${gameLabel(game)} runtime repaired; the instance remains stopped`),
                onError: (error) => toast.error(error.message),
              })}
            >
              {repair.isPending ? <Loader2 className="size-4 animate-spin" /> : <Wrench className="size-4" />}
              Repair
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  )
}
