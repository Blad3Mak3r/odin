import { useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import { QueryError } from '@/components/QueryError'
import { useSevenDaysPlayerAction, useSevenDaysPlayers } from '@/lib/queries'

export function SevenDaysPlayersTab({ id, running }: { id: string; running: boolean }) {
  const players = useSevenDaysPlayers(id, running)
  const action = useSevenDaysPlayerAction(id)
  const [reason, setReason] = useState('')
  if (!running) return <Card><CardHeader><CardTitle>Players</CardTitle><CardDescription>Start the server to query its player list.</CardDescription></CardHeader></Card>
  if (players.isError) return <QueryError error={players.error} />
  return <Card><CardHeader><CardTitle>Players</CardTitle><CardDescription>Reported by the 7 Days to Die local console. A modded server may format player identifiers differently.</CardDescription></CardHeader><CardContent className="flex flex-col gap-3"><Input value={reason} placeholder="Optional moderation reason" onChange={(event) => setReason(event.target.value)} />{players.isLoading ? <><Skeleton className="h-12 w-full" /><Skeleton className="h-12 w-full" /></> : players.data?.length === 0 ? <p className="text-sm text-muted-foreground">No players were reported by the server.</p> : players.data?.map((player) => <div key={player.entity_id} className="flex flex-wrap items-center justify-between gap-3 rounded-lg border p-3"><div><p className="font-medium">{player.name}</p><p className="text-xs text-muted-foreground">Entity {player.entity_id}{player.platform_id ? ` · ${player.platform_id}` : ''}</p></div><div className="flex gap-2"><Button size="sm" variant="outline" disabled={action.isPending} onClick={() => action.mutate({ player: player.platform_id ?? player.entity_id, action: 'kick', reason }, { onSuccess: () => toast.success('Player kicked'), onError: (error) => toast.error(error.message) })}>Kick</Button><Button size="sm" variant="destructive" disabled={action.isPending} onClick={() => action.mutate({ player: player.platform_id ?? player.entity_id, action: 'ban', reason }, { onSuccess: () => toast.success('Player banned'), onError: (error) => toast.error(error.message) })}>Ban</Button></div></div>)}</CardContent></Card>
}
