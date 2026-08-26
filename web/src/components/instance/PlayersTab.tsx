import { Users } from 'lucide-react'
import { QueryError } from '@/components/QueryError'
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
    return <p className="text-sm text-muted-foreground">Loading…</p>
  }

  if (players.data.length === 0) {
    return <p className="text-sm text-muted-foreground">No players connected right now.</p>
  }

  return (
    <div className="flex max-w-sm flex-col gap-2">
      {players.data.map((player) => (
        <div
          key={player.name}
          className="flex items-center justify-between rounded-md border p-3 text-sm"
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
