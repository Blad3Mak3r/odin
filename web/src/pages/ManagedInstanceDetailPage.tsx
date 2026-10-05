import { RustAccessListsTab } from '@/components/instance/RustAccessListsTab'
import { RustRconTab } from '@/components/instance/RustRconTab'
import { WipeMapCard } from '@/components/instance/WipeMapCard'
import { UptimeScheduleCard } from '@/components/instance/UptimeScheduleCard'
import { useState } from 'react'
import { BackupsTab } from '@/components/instance/BackupsTab'
import { SaveFilesTab } from '@/components/instance/SaveFilesTab'
import { Navigate, useNavigate, useParams } from 'react-router-dom'
import { toast } from 'sonner'
import { ManagedInstanceHeader } from '@/components/instance/ManagedInstanceHeader'
import { ManagedLogsTab } from '@/components/instance/ManagedLogsTab'
import { ManagedResourcesTab } from '@/components/instance/ManagedResourcesTab'
import { LiveLogOutput } from '@/components/instance/LiveLogOutput'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { QueryError } from '@/components/QueryError'
import {
  useManagedInstance,
  useManagedInstanceById,
  useManagedInstanceLogs,
  useUpdateRustConfig,
} from '@/lib/queries'
import type { GameId } from '@/lib/types'
import { useLogSocket } from '@/hooks/useLogSocket'

function isConsoleError(line: string) {
  return /(?:^|\W)(?:error|exception|fatal|assertion failed|stack trace)(?:\W|$)/i.test(line)
}

function isGameId(value: string | undefined): value is GameId {
  return value === 'valheim' || value === 'rust' || value === 'vrising' || value === 'palworld' || value === 'runescape-dragonwilds'
}

type RustConfig = {
  port: number
  queryPort: number
  rconPort: number
  rconPassword: string
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
  const rconPort = config.rcon_port
  const rconPassword = config.rcon_password
  const hostname = config.hostname
  const level = config.level
  const seed = config.seed
  const worldSize = config.world_size
  const maxPlayers = config.max_players
  const autoRestart = config.auto_restart
  if (
    typeof port !== 'number' || typeof queryPort !== 'number' || typeof rconPort !== 'number' || typeof rconPassword !== 'string' || typeof hostname !== 'string' ||
    typeof level !== 'string' || typeof seed !== 'number' || typeof worldSize !== 'number' ||
    typeof maxPlayers !== 'number' || typeof autoRestart !== 'boolean'
  ) return null
  return { port, queryPort, rconPort, rconPassword, hostname, level, seed, worldSize, maxPlayers, autoRestart }
}

function RustConfigForm({ name, config, running }: { name: string; config: RustConfig; running: boolean }) {
  const update = useUpdateRustConfig()
  const [port, setPort] = useState(String(config.port))
  const [queryPort, setQueryPort] = useState(String(config.queryPort))
  const [rconPort, setRconPort] = useState(String(config.rconPort))
  const [rconPassword, setRconPassword] = useState(config.rconPassword)
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
        port: Number(port),
        query_port: Number(queryPort),
        rcon_port: Number(rconPort),
        rcon_password: rconPassword,
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
        if (new Set([Number(port), Number(queryPort), Number(rconPort)]).size !== 3) {
          toast.error('Game, query, and RCON ports must be different')
          return
        }
        save()
      }}
    >
      {running && <p className="text-sm text-muted-foreground">Stop this Rust server before changing its configuration.</p>}
      <div className="grid gap-4 sm:grid-cols-2">
        <ConfigInput id="rust-port" label="Game port" type="number" min={1} max={65535} value={port} disabled={running} onChange={setPort} />
        <ConfigInput id="rust-query-port" label="Query port" type="number" min={1} max={65535} value={queryPort} disabled={running} onChange={setQueryPort} />
        <ConfigInput id="rust-rcon-port" label="RCON port" type="number" min={1} max={65535} value={rconPort} disabled={running} onChange={setRconPort} />
        <ConfigInput id="rust-rcon-password" label="RCON password" type="password" value={rconPassword} disabled={running} onChange={setRconPassword} />
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
      <p className="text-sm text-muted-foreground">Choose different game, query, and RCON ports between 1 and 65535. Keep the RCON password secret, and only allow its TCP port through your firewall when remote administration is needed.</p>
      <Button className="w-fit" type="submit" disabled={running || update.isPending}>Save configuration</Button>
    </form>
  )
}

function ConfigInput({ id, label, type = 'text', value, disabled, onChange, min, max }: {
  id: string
  label: string
  type?: 'text' | 'number' | 'password'
  value: string | number
  disabled: boolean
  onChange: (value: string) => void
  min?: number
  max?: number
}) {
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>{label}</Label>
      <Input id={id} type={type} min={min} max={max} required value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} />
    </div>
  )
}

export function ManagedInstanceDetailPage() {
  const { id, game, name, '*': tabPath } = useParams<{ id: string; game: string; name: string; '*': string }>()
  const navigate = useNavigate()
  const gameId = isGameId(game) ? game : 'valheim'
  const instanceById = useManagedInstanceById(id ?? '')
  const instanceByName = useManagedInstance(gameId, name ?? '')
  const instance = id ? instanceById : instanceByName
  const detailForRequests = instance.data
  const logs = useManagedInstanceLogs(detailForRequests?.game ?? gameId, detailForRequests?.name ?? name ?? '')
  const liveLogs = useLogSocket(detailForRequests?.name ?? name ?? '', detailForRequests?.game ?? gameId)
  const consoleLines = liveLogs.lines.length > 0 ? liveLogs.lines : (logs.data?.lines ?? [])
  const errorLines = consoleLines.filter(isConsoleError)
  if (!id && (!isGameId(game) || !name)) return null
  if (instance.isError) return <QueryError error={instance.error} />
  if (!instance.data) return null
  const detail = instance.data
  const rustConfig = detail.game === 'rust' ? asRustConfig(detail.config) : null
  const tabs = [
    { id: 'logs', label: 'Logs' },
    ...(detail.game === 'rust' ? [{ id: 'rcon', label: 'RCON' }] : []),
    { id: 'errors', label: 'Errors' },
    { id: 'config', label: 'Config' },
    ...(detail.capabilities.access_lists ? [{ id: 'lists', label: 'Access lists' }] : []),
    ...(detail.capabilities.backups ? [{ id: 'backups', label: 'Backups' }] : []),
    { id: 'saves', label: 'Save files' },
    { id: 'resources', label: 'Resources' },
  ]
  const [tab, ...nestedPath] = tabPath?.split('/').filter(Boolean) ?? []
  if (!tab || (tab !== 'lists' && nestedPath.length > 0) || !tabs.some((candidate) => candidate.id === tab)) {
    return <Navigate replace to={id ? `/instance/${id}/logs` : `/instances/${detail.game}/${detail.name}/logs`} />
  }

  return (
    <div className="flex flex-col gap-6">
      <ManagedInstanceHeader instance={detail} />
      <Tabs value={tab} onValueChange={(value) => navigate(id ? (value === 'lists' ? `/instance/${id}/lists/owner` : `/instance/${id}/${value}`) : (value === 'lists' ? `/instances/${detail.game}/${detail.name}/lists/owner` : `/instances/${detail.game}/${detail.name}/${value}`))}>
        <div className="overflow-x-auto">
          <TabsList className="w-max">
          {tabs.map((item) => <TabsTrigger key={item.id} value={item.id}>{item.label}</TabsTrigger>)}
          </TabsList>
        </div>
        <TabsContent value="logs"><ManagedLogsTab game={detail.game} name={detail.name} /></TabsContent>
        {detail.game === 'rust' && <TabsContent value="rcon"><RustRconTab name={detail.name} running={detail.running} /></TabsContent>}
        <TabsContent value="errors">
          <Card>
            <CardHeader>
              <CardTitle>Errors</CardTitle>
              <CardDescription>Console lines containing errors, exceptions, fatal failures, assertions, or stack traces.</CardDescription>
            </CardHeader>
            <CardContent>
              {logs.isError
                ? <QueryError error={logs.error} />
                : <LiveLogOutput lines={errorLines} emptyMessage="No errors found in the available console output." />}
            </CardContent>
          </Card>
        </TabsContent>
        <TabsContent value="config" className="flex flex-col gap-6">
          {rustConfig
            ? <RustConfigForm key={`${detail.id}-${JSON.stringify(detail.config)}`} name={detail.name} config={rustConfig} running={detail.running} />
            : <p className="text-sm text-muted-foreground">No editable configuration is available for this game.</p>}
          <UptimeScheduleCard game={detail.game} name={detail.name} />
          {detail.game === 'rust' && <WipeMapCard name={detail.name} running={detail.running} />}
        </TabsContent>
        {detail.capabilities.backups && (
          <TabsContent value="backups"><BackupsTab name={detail.name} game={detail.game} running={detail.running} /></TabsContent>
        )}
        {detail.game === 'rust' && tab === 'lists' && (
          <TabsContent value="lists"><RustAccessListsTab name={detail.name} path={nestedPath} running={detail.running} /></TabsContent>
        )}
        <TabsContent value="saves"><SaveFilesTab game={detail.game} name={detail.name} /></TabsContent>
        <TabsContent value="resources"><ManagedResourcesTab name={detail.name} running={detail.running} /></TabsContent>
      </Tabs>
    </div>
  )
}
