import { Users } from 'lucide-react'
import { QueryError } from '@/components/QueryError'
import { Skeleton } from '@/components/ui/skeleton'
import { usePlayerHistory, usePlayers } from '@/lib/queries'
import { formatRelativeTime } from '@/lib/utils'

export function PlayersTab({ id, running }: { id: string; running: boolean }) {
  const players = usePlayers(id, running)
  const history = usePlayerHistory(id)

  if (players.isError || history.isError) {
    return <QueryError error={players.error ?? history.error} />
  }

  if ((running && (players.isLoading || !players.data)) || history.isLoading) {
    return (
      <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
        <Skeleton className="h-11 w-full" />
        <Skeleton className="h-11 w-full" />
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <section className="space-y-3">
        <h3 className="text-sm font-medium">Connected now</h3>
        {!running ? (
          <p className="text-sm text-muted-foreground">Instance is stopped.</p>
        ) : players.data?.length === 0 ? (
          <p className="text-sm text-muted-foreground">No players connected right now.</p>
        ) : (
          <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
            {players.data?.map((player) => (
              <div
                key={player.steam_id ?? player.name}
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
        )}
      </section>

      <section className="space-y-3">
        <h3 className="text-sm font-medium">Recent sessions</h3>
        {!history.data?.length ? (
          <p className="text-sm text-muted-foreground">No player sessions recorded yet.</p>
        ) : (
          <div className="overflow-x-auto rounded-xl border">
            <table className="w-full text-left text-sm">
              <thead className="border-b bg-muted/40 text-xs text-muted-foreground">
                <tr>
                  <th className="px-3 py-2 font-medium">Player</th>
                  <th className="px-3 py-2 font-medium">Steam ID</th>
                  <th className="px-3 py-2 font-medium">Joined</th>
                  <th className="px-3 py-2 font-medium">Left</th>
                </tr>
              </thead>
              <tbody>
                {history.data.map((session) => (
                  <tr key={session.id} className="border-b last:border-0">
                    <td className="px-3 py-2 font-medium">{session.name}</td>
                    <td className="px-3 py-2 font-mono text-xs text-muted-foreground">
                      {session.steam_id ?? 'Unknown'}
                    </td>
                    <td className="px-3 py-2">{formatRelativeTime(session.joined_at)}</td>
                    <td className="px-3 py-2">
                      {session.left_at ? formatRelativeTime(session.left_at) : 'Connected'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  )
}
