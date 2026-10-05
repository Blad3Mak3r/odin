import { useState } from 'react'
import { toast } from 'sonner'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { usePalworldAction, usePalworldMetrics, usePalworldPlayers } from '@/lib/queries'

type Player = Record<string, unknown>

function playerList(response: unknown): Player[] {
  if (Array.isArray(response)) return response.filter((player): player is Player => typeof player === 'object' && player !== null)
  if (typeof response !== 'object' || response === null) return []
  const players = (response as Record<string, unknown>).players
  return Array.isArray(players) ? players.filter((player): player is Player => typeof player === 'object' && player !== null) : []
}

function playerValue(player: Player, ...keys: string[]) {
  for (const key of keys) {
    const value = player[key]
    if (typeof value === 'string' || typeof value === 'number') return String(value)
  }
  return null
}

export function PalworldAdminTab({ id, running }: { id: string; running: boolean }) {
  const players = usePalworldPlayers(id, running)
  const metrics = usePalworldMetrics(id, running)
  const announce = usePalworldAction('announce')
  const save = usePalworldAction('save')
  const kick = usePalworldAction('kick')
  const ban = usePalworldAction('ban')
  const unban = usePalworldAction('unban')
  const shutdown = usePalworldAction('shutdown')
  const [message, setMessage] = useState('')
  const [unbanId, setUnbanId] = useState('')
  const { confirm, dialog } = useConfirmDialog()

  if (!running) return <p className="text-sm text-muted-foreground">Instance is stopped — start it before using Palworld administration.</p>
  if (players.isError) return <QueryError error={players.error} />
  if (metrics.isError) return <QueryError error={metrics.error} />

  const list = playerList(players.data)
  const notify = (label: string) => ({ onSuccess: () => toast.success(label), onError: (error: Error) => toast.error(error.message) })
  const announceMessage = () => {
    const trimmed = message.trim()
    if (!trimmed) return
    announce.mutate({ id, request: { message: trimmed } }, { ...notify('Announcement sent'), onSuccess: () => { setMessage(''); toast.success('Announcement sent') } })
  }
  const shutdownServer = async () => {
    if (await confirm({ title: 'Shut down Palworld?', description: 'The game server will request a clean shutdown.', confirmLabel: 'Shut down' })) {
      shutdown.mutate({ id, request: {} }, notify('Shutdown requested'))
    }
  }

  return <div className="flex flex-col gap-4">
    {dialog}
    <Card>
      <CardHeader><CardTitle>Server controls</CardTitle><CardDescription>Commands are authenticated by Odin and sent only to this instance’s loopback REST API.</CardDescription></CardHeader>
      <CardContent className="flex flex-wrap gap-2">
        <Input className="min-w-56 flex-1" placeholder="Announcement message" value={message} onChange={(event) => setMessage(event.target.value)} onKeyDown={(event) => event.key === 'Enter' && announceMessage()} />
        <Button onClick={announceMessage} disabled={!message.trim() || announce.isPending}>Announce</Button>
        <Button variant="outline" onClick={() => save.mutate({ id }, notify('World save requested'))} disabled={save.isPending}>Save world</Button>
        <Button variant="destructive" onClick={shutdownServer} disabled={shutdown.isPending}>Shut down</Button>
      </CardContent>
    </Card>
    <Card>
      <CardHeader><CardTitle>Players</CardTitle><CardDescription>Connected players reported by Palworld.</CardDescription></CardHeader>
      <CardContent className="flex flex-col gap-2">
        {players.isLoading && <><Skeleton className="h-12 w-full" /><Skeleton className="h-12 w-full" /></>}
        {!players.isLoading && list.length === 0 && <p className="text-sm text-muted-foreground">No players are currently connected.</p>}
        {list.map((player, index) => {
          const userId = playerValue(player, 'userid', 'user_id', 'playeruid', 'steamid', 'id')
          const name = playerValue(player, 'name', 'playername', 'nickname') ?? 'Unknown player'
          return <div key={userId ?? index} className="flex flex-wrap items-center justify-between gap-2 rounded-xl border px-3 py-2">
            <div><p className="font-medium">{name}</p>{userId && <p className="font-mono text-xs text-muted-foreground">{userId}</p>}</div>
            {userId && <div className="flex gap-2"><Button size="sm" variant="outline" disabled={kick.isPending} onClick={() => kick.mutate({ id, request: { user_id: userId } }, notify(`Kicked ${name}`))}>Kick</Button><Button size="sm" variant="destructive" disabled={ban.isPending} onClick={() => ban.mutate({ id, request: { user_id: userId } }, notify(`Banned ${name}`))}>Ban</Button></div>}
          </div>
        })}
      </CardContent>
    </Card>
    <Card>
      <CardHeader><CardTitle>Unban player</CardTitle></CardHeader>
      <CardContent className="flex gap-2"><Input placeholder="Palworld user ID" value={unbanId} onChange={(event) => setUnbanId(event.target.value)} /><Button disabled={!unbanId.trim() || unban.isPending} onClick={() => unban.mutate({ id, request: { user_id: unbanId.trim() } }, { ...notify('Player unbanned'), onSuccess: () => { setUnbanId(''); toast.success('Player unbanned') } })}>Unban</Button></CardContent>
    </Card>
    <Card>
      <CardHeader><CardTitle>Metrics</CardTitle></CardHeader>
      <CardContent>{metrics.isLoading ? <Skeleton className="h-16 w-full" /> : <pre className="max-h-80 overflow-auto rounded-lg bg-muted p-3 text-xs">{JSON.stringify(metrics.data, null, 2)}</pre>}</CardContent>
    </Card>
  </div>
}
