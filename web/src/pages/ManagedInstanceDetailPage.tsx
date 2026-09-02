import { Link, useParams } from 'react-router-dom'
import { PageHeader } from '@/components/PageHeader'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { QueryError } from '@/components/QueryError'
import { useManagedInstance, useManagedInstanceLogs } from '@/lib/queries'
import type { GameId } from '@/lib/types'

function isGameId(value: string | undefined): value is GameId {
  return value === 'valheim' || value === 'rust'
}

export function ManagedInstanceDetailPage() {
  const { game, name } = useParams<{ game: string; name: string }>()
  const gameId = isGameId(game) ? game : 'valheim'
  const instance = useManagedInstance(gameId, name ?? '')
  const logs = useManagedInstanceLogs(gameId, name ?? '')
  if (!isGameId(game) || !name) return null
  if (instance.isError) return <QueryError error={instance.error} />
  if (!instance.data) return null
  const detail = instance.data
  return (
    <div className="flex flex-col gap-6">
      <PageHeader title={detail.name} description={`${detail.game} server`} />
      <Card>
        <CardHeader><CardTitle>Configuration</CardTitle><CardDescription>Game-specific settings managed by Odin.</CardDescription></CardHeader>
        <CardContent className="flex flex-col gap-2">
          {Object.entries(detail.config).map(([key, value]) => <div key={key} className="flex justify-between gap-4 text-sm"><span className="text-muted-foreground">{key}</span><span>{String(value ?? '—')}</span></div>)}
        </CardContent>
      </Card>
      <Card>
        <CardHeader><CardTitle>Logs</CardTitle><CardDescription>Last 200 server log lines.</CardDescription></CardHeader>
        <CardContent>
          {logs.isError ? <QueryError error={logs.error} /> : <pre className="max-h-96 overflow-auto whitespace-pre-wrap rounded-md bg-muted p-3 text-xs">{logs.data?.lines.join('\n') || 'No logs yet.'}</pre>}
        </CardContent>
      </Card>
      {detail.game === 'valheim' && <Link className="text-sm underline" to={`/instances/valheim/${detail.name}/logs`}>Open the full Valheim dashboard</Link>}
      <Badge className="w-fit" variant={detail.running ? 'default' : 'secondary'}>{detail.running ? 'running' : 'stopped'}</Badge>
    </div>
  )
}
