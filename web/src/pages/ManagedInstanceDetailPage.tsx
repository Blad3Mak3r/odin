import { useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { toast } from 'sonner'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { PageHeader } from '@/components/PageHeader'
import { ResourceMetric } from '@/components/ResourceMetric'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { QueryError } from '@/components/QueryError'
import {
  useCreateManagedBackup,
  useManagedBackups,
  useManagedInstance,
  useManagedInstanceAction,
  useManagedInstanceLogs,
  useManagedInstanceTransition,
  useManagedRustResources,
  useManagedRustResourceHistory,
  useRestoreManagedBackup,
  useUpdateRustConfig,
} from '@/lib/queries'
import type { GameId } from '@/lib/types'
import { formatBytes, formatRelativeTime } from '@/lib/utils'

function isGameId(value: string | undefined): value is GameId {
  return value === 'valheim' || value === 'rust'
}

type RustConfig = {
  port: number
  queryPort: number
  hostname: string
  level: string
  seed: number
  worldSize: number
  maxPlayers: number
  autoRestart: boolean
}

function asRustConfig(config: Record<string, unknown>): RustConfig | null {
  const port = config.port
  const queryPort = config.query_port
  const hostname = config.hostname
  const level = config.level
  const seed = config.seed
  const worldSize = config.world_size
  const maxPlayers = config.max_players
  const autoRestart = config.auto_restart
  if (
    typeof port !== 'number' || typeof queryPort !== 'number' || typeof hostname !== 'string' ||
    typeof level !== 'string' || typeof seed !== 'number' || typeof worldSize !== 'number' ||
    typeof maxPlayers !== 'number' || typeof autoRestart !== 'boolean'
  ) return null
  return { port, queryPort, hostname, level, seed, worldSize, maxPlayers, autoRestart }
}

function RustConfigForm({ name, config, running }: { name: string; config: RustConfig; running: boolean }) {
  const update = useUpdateRustConfig()
  const [hostname, setHostname] = useState(config.hostname)
  const [level, setLevel] = useState(config.level)
  const [seed, setSeed] = useState(config.seed)
  const [worldSize, setWorldSize] = useState(config.worldSize)
  const [maxPlayers, setMaxPlayers] = useState(config.maxPlayers)
  const [autoRestart, setAutoRestart] = useState(config.autoRestart)

  const save = () => update.mutate(
    {
      name,
      request: {
        hostname,
        level,
        seed,
        world_size: worldSize,
        max_players: maxPlayers,
        auto_restart: autoRestart,
      },
    },
    {
      onSuccess: () => toast.success('Rust configuration saved'),
      onError: (error) => toast.error(error.message),
    },
  )

  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(event) => {
        event.preventDefault()
        save()
      }}
    >
      {running && <p className="text-sm text-muted-foreground">Stop this Rust server before changing its configuration.</p>}
      <div className="grid gap-4 sm:grid-cols-2">
        <ConfigInput id="rust-hostname" label="Hostname" value={hostname} disabled={running} onChange={setHostname} />
        <ConfigInput id="rust-level" label="Map" value={level} disabled={running} onChange={setLevel} />
        <ConfigInput id="rust-seed" label="Seed" type="number" value={seed} disabled={running} onChange={(value) => setSeed(Number(value))} />
        <ConfigInput id="rust-world-size" label="World size" type="number" min={1} value={worldSize} disabled={running} onChange={(value) => setWorldSize(Number(value))} />
        <ConfigInput id="rust-max-players" label="Max players" type="number" min={1} value={maxPlayers} disabled={running} onChange={(value) => setMaxPlayers(Number(value))} />
      </div>
      <div className="flex items-center justify-between rounded-xl border p-3">
        <div>
          <Label htmlFor="rust-auto-restart">Restart automatically</Label>
          <p className="text-xs text-muted-foreground">Restart this server after an unexpected exit.</p>
        </div>
        <Switch id="rust-auto-restart" checked={autoRestart} disabled={running} onCheckedChange={setAutoRestart} />
      </div>
      <p className="text-sm text-muted-foreground">Ports are allocated by Odin: game {config.port}, query {config.queryPort}.</p>
      <Button className="w-fit" type="submit" disabled={running || update.isPending}>Save configuration</Button>
    </form>
  )
}

function ConfigInput({ id, label, type = 'text', value, disabled, onChange, min }: {
  id: string
  label: string
  type?: 'text' | 'number'
  value: string | number
  disabled: boolean
  onChange: (value: string) => void
  min?: number
}) {
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>{label}</Label>
      <Input id={id} type={type} min={min} value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} />
    </div>
  )
}

export function ManagedInstanceDetailPage() {
  const { game, name } = useParams<{ game: string; name: string }>()
  const gameId = isGameId(game) ? game : 'valheim'
  const instance = useManagedInstance(gameId, name ?? '')
  const logs = useManagedInstanceLogs(gameId, name ?? '')
  const resources = useManagedRustResources(name ?? '', gameId === 'rust')
  const resourceHistory = useManagedRustResourceHistory(name ?? '', gameId === 'rust')
  const backups = useManagedBackups(gameId, name ?? '')
  const start = useManagedInstanceAction('start')
  const stop = useManagedInstanceAction('stop')
  const restart = useManagedInstanceAction('restart')
  const transition = useManagedInstanceTransition(gameId, name ?? '')
  const createBackup = useCreateManagedBackup()
  const restoreBackup = useRestoreManagedBackup()
  const { confirm, dialog } = useConfirmDialog()
  if (!isGameId(game) || !name) return null
  if (instance.isError) return <QueryError error={instance.error} />
  if (!instance.data) return null
  const detail = instance.data
  const rustConfig = detail.game === 'rust' ? asRustConfig(detail.config) : null
  const target = { game: detail.game, name: detail.name }
  const busy = start.isPending || stop.isPending || restart.isPending || transition.data !== null
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
          {rustConfig
            ? <RustConfigForm key={`${detail.id}-${JSON.stringify(detail.config)}`} name={detail.name} config={rustConfig} running={detail.running} />
            : Object.entries(detail.config).map(([key, value]) => <div key={key} className="flex justify-between gap-4 text-sm"><span className="text-muted-foreground">{key}</span><span>{String(value ?? '—')}</span></div>)}
        </CardContent>
      </Card>
      {detail.game === 'rust' && (
        <Card>
          <CardHeader><CardTitle>Server resources</CardTitle><CardDescription>Live CPU and memory use for this Rust server.</CardDescription></CardHeader>
          <CardContent className="flex flex-col gap-4">
            {resources.isError ? <QueryError error={resources.error} /> : (
              <>
                {resourceHistory.isError && <QueryError error={resourceHistory.error} />}
                <ResourceMetric
                  label="CPU"
                  value={`${resources.data?.cpu_percent.toFixed(1) ?? '0.0'}%`}
                  history={resourceHistory.data ?? []}
                  dataKey="cpu_percent"
                  formatValue={(value) => `${value.toFixed(1)}%`}
                />
                <ResourceMetric
                  label="Memory"
                  value={formatBytes(resources.data?.memory_bytes ?? 0)}
                  history={resourceHistory.data ?? []}
                  dataKey="memory_bytes"
                  formatValue={formatBytes}
                />
              </>
            )}
          </CardContent>
        </Card>
      )}
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
