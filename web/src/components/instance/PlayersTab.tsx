import { Users } from 'lucide-react'
import { QueryError } from '@/components/QueryError'
import { Skeleton } from '@/components/ui/skeleton'
import { usePlayers } from '@/lib/queries'
import { formatRelativeTime } from '@/lib/utils'

export function PlayersTab({ name, running }: { name: string; running: boolean }) {
  const players = usePlayers(name, running)

  if (!running) {
    return <p className="text-sm text-muted-foreground">Instance is stopped — no players connected.</p>
  }

  if (players.isError) {
    return <QueryError error={players.error} />
  }

  if (players.isLoading || !players.data) {
    return (
      <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
        <Skeleton className="h-11 w-full" />
        <Skeleton className="h-11 w-full" />
      </div>
    )
  }

  if (players.data.length === 0) {
    return <p className="text-sm text-muted-foreground">No players connected right now.</p>
  }

  return (
    <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
      {players.data.map((player) => (
        <div
          key={player.name}
          className="flex items-center justify-between rounded-xl border p-3 text-sm"
        >
          <span className="flex items-center gap-2">
            <Users className="size-4 text-muted-foreground" />
            {player.name}
          </span>
          <span className="text-xs text-muted-foreground">
            since {formatRelativeTime(player.connected_at)}
          </span>
        </div>
      ))}
    </div>
  )
}
