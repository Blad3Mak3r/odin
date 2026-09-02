import { Link, useParams } from 'react-router-dom'
import { toast } from 'sonner'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { PageHeader } from '@/components/PageHeader'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { QueryError } from '@/components/QueryError'
import {
  useCreateManagedBackup,
  useManagedBackups,
  useManagedInstance,
  useManagedInstanceAction,
  useManagedInstanceLogs,
  useRestoreManagedBackup,
} from '@/lib/queries'
import type { GameId } from '@/lib/types'
import { formatBytes, formatRelativeTime } from '@/lib/utils'

function isGameId(value: string | undefined): value is GameId {
  return value === 'valheim' || value === 'rust'
}

export function ManagedInstanceDetailPage() {
  const { game, name } = useParams<{ game: string; name: string }>()
  const gameId = isGameId(game) ? game : 'valheim'
  const instance = useManagedInstance(gameId, name ?? '')
  const logs = useManagedInstanceLogs(gameId, name ?? '')
  const backups = useManagedBackups(gameId, name ?? '')
  const start = useManagedInstanceAction('start')
  const stop = useManagedInstanceAction('stop')
  const restart = useManagedInstanceAction('restart')
  const createBackup = useCreateManagedBackup()
  const restoreBackup = useRestoreManagedBackup()
  const { confirm, dialog } = useConfirmDialog()
  if (!isGameId(game) || !name) return null
  if (instance.isError) return <QueryError error={instance.error} />
  if (!instance.data) return null
  const detail = instance.data
  const target = { game: detail.game, name: detail.name }
  const busy = start.isPending || stop.isPending || restart.isPending
  const backupsRequireStop = detail.game === 'rust' && detail.running

  const restore = async (backupId: string) => {
    const confirmed = await confirm({
      title: `Restore backup '${backupId}'?`,
      description: `Restore '${detail.name}' from this backup? Odin snapshots the current data first.`,
      confirmLabel: 'Restore',
    })
    if (!confirmed) return
    restoreBackup.mutate(
      { ...target, backupId },
      { onSuccess: () => toast.success('Backup restored'), onError: (error) => toast.error(error.message) },
    )
  }

  return (
    <div className="flex flex-col gap-6">
      {dialog}
      <PageHeader
        title={detail.name}
        description={`${detail.game} server`}
        action={
          <div className="flex gap-2">
            {detail.running ? (
              <>
                <Button variant="outline" disabled={busy} onClick={() => restart.mutate(target, { onError: (error) => toast.error(error.message) })}>Restart</Button>
                <Button variant="outline" disabled={busy} onClick={() => stop.mutate(target, { onError: (error) => toast.error(error.message) })}>Stop</Button>
              </>
            ) : <Button disabled={busy} onClick={() => start.mutate(target, { onError: (error) => toast.error(error.message) })}>Start</Button>}
          </div>
        }
      />
      <Card>
        <CardHeader><CardTitle>Configuration</CardTitle><CardDescription>Game-specific settings managed by Odin.</CardDescription></CardHeader>
        <CardContent className="flex flex-col gap-2">
          {Object.entries(detail.config).map(([key, value]) => <div key={key} className="flex justify-between gap-4 text-sm"><span className="text-muted-foreground">{key}</span><span>{String(value ?? '—')}</span></div>)}
        </CardContent>
      </Card>
      {detail.capabilities.backups && (
        <Card>
          <CardHeader>
            <CardTitle>Backups</CardTitle>
            <CardDescription>{backupsRequireStop ? 'Stop this Rust server before creating or restoring a backup.' : 'Create or restore local server data backups.'}</CardDescription>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <div>
              <Button
                variant="outline"
                disabled={backupsRequireStop || createBackup.isPending}
                onClick={() => createBackup.mutate(target, { onSuccess: () => toast.success('Backup created'), onError: (error) => toast.error(error.message) })}
              >
                Create backup
              </Button>
            </div>
            {backups.isError && <QueryError error={backups.error} />}
            {backups.data?.length === 0 && <p className="text-sm text-muted-foreground">No backups yet.</p>}
            {backups.data?.map((backup) => (
              <div key={backup.id} className="flex flex-wrap items-center justify-between gap-2 border-t pt-3 text-sm">
                <span>{formatRelativeTime(backup.created_at)} · {formatBytes(backup.size_bytes)}</span>
                <Button size="sm" variant="outline" disabled={backupsRequireStop || restoreBackup.isPending} onClick={() => restore(backup.id)}>Restore</Button>
              </div>
            ))}
          </CardContent>
        </Card>
      )}
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
