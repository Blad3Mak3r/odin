import { RustAccessListsTab } from '@/components/instance/RustAccessListsTab'
import { VRisingAccessListsTab } from '@/components/instance/VRisingAccessListsTab'
import { VRisingRconTab } from '@/components/instance/VRisingRconTab'
import { PalworldAdminTab } from '@/components/instance/PalworldAdminTab'
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
  useUpdateGenericConfig,
  useUpdateRustConfig,
} from '@/lib/queries'
import type { GameId, GenericConfigUpdateRequest } from '@/lib/types'
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

type GenericConfig = GenericConfigUpdateRequest

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

function asGenericConfig(config: Record<string, unknown>): GenericConfig | null {
  const port = config.port
  const queryPort = config.query_port
  const adminPort = config.admin_port
  const settings = { ...config }
  delete settings.port
  delete settings.query_port
  delete settings.admin_port
  delete settings.auto_restart
  if (typeof port !== 'number' || (typeof queryPort !== 'number' && queryPort !== null) || (typeof adminPort !== 'number' && adminPort !== null) || typeof config.auto_restart !== 'boolean') return null
  return { port, query_port: queryPort, admin_port: adminPort, settings, auto_restart: config.auto_restart }
}

function RustConfigForm({ id, name, config, running }: { id: string; name: string; config: RustConfig; running: boolean }) {
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
      id,
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

function ConfigInput({ id, label, type = 'text', value, disabled, onChange, min, max, required = true }: {
  id: string
  label: string
  type?: 'text' | 'number' | 'password'
  value: string | number
  disabled: boolean
  onChange: (value: string) => void
  min?: number
  max?: number
  required?: boolean
}) {
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>{label}</Label>
      <Input id={id} type={type} min={min} max={max} required={required} value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} />
    </div>
  )
}

function GenericConfigForm({ id, game, name, config, running }: { id: string; game: Extract<GameId, 'vrising' | 'palworld' | 'runescape-dragonwilds'>; name: string; config: GenericConfig; running: boolean }) {
  const update = useUpdateGenericConfig()
  const [port, setPort] = useState(String(config.port))
  const [queryPort, setQueryPort] = useState(config.query_port === null ? '' : String(config.query_port))
  const [adminPort, setAdminPort] = useState(config.admin_port === null ? '' : String(config.admin_port))
  const [serverName, setServerName] = useState(typeof config.settings.server_name === 'string' ? config.settings.server_name : name)
  const [maxPlayers, setMaxPlayers] = useState(typeof config.settings.max_players === 'number' ? String(config.settings.max_players) : '32')
  const [ownerId, setOwnerId] = useState(typeof config.settings.owner_id === 'string' ? config.settings.owner_id : '')
  const [worldName, setWorldName] = useState(typeof config.settings.default_world_name === 'string' ? config.settings.default_world_name : name)
  const [adminPassword, setAdminPassword] = useState(typeof config.settings.admin_password === 'string' ? config.settings.admin_password : '')
  const [worldPassword, setWorldPassword] = useState(typeof config.settings.world_password === 'string' ? config.settings.world_password : '')
  const [rconPassword, setRconPassword] = useState(typeof config.settings.rcon_password === 'string' ? config.settings.rcon_password : '')
  const [autoRestart, setAutoRestart] = useState(config.auto_restart)
  const [rconEnabled, setRconEnabled] = useState(config.settings.rcon_enabled === true)
  const [restEnabled, setRestEnabled] = useState(config.settings.rest_api_enabled === true)
  const isDragonwilds = game === 'runescape-dragonwilds'

  const save = () => {
    const settings: Record<string, unknown> = { ...config.settings, server_name: serverName }
    if (game === 'vrising') Object.assign(settings, { max_players: Number(maxPlayers), rcon_enabled: rconEnabled, rcon_password: rconPassword })
    if (game === 'palworld') Object.assign(settings, { max_players: Number(maxPlayers), rest_api_enabled: restEnabled, admin_password: adminPassword })
    if (isDragonwilds) Object.assign(settings, { owner_id: ownerId, default_world_name: worldName, admin_password: adminPassword, world_password: worldPassword })
    update.mutate({
      id,
      game,
      name,
      request: {
        port: Number(port),
        query_port: queryPort ? Number(queryPort) : null,
        admin_port: adminPort ? Number(adminPort) : null,
        settings,
        auto_restart: autoRestart,
      },
    }, {
      onSuccess: () => toast.success('Server configuration saved'),
      onError: (error) => toast.error(error.message),
    })
  }

  return (
    <form className="flex flex-col gap-4" onSubmit={(event) => { event.preventDefault(); save() }}>
      <p className="text-sm text-muted-foreground">Stop this server before changing its configuration. Empty password fields keep their existing value.</p>
      <div className="grid gap-4 sm:grid-cols-2">
        <ConfigInput id="generic-port" label="Game port" type="number" min={1} max={65535} value={port} disabled={running} onChange={setPort} />
        {config.query_port !== null && <ConfigInput id="generic-query-port" label={isDragonwilds ? 'Beacon port' : 'Query port'} type="number" min={1} max={65535} value={queryPort} disabled={running} onChange={setQueryPort} />}
        {config.admin_port !== null && <ConfigInput id="generic-admin-port" label={game === 'palworld' ? 'REST API port' : 'RCON port'} type="number" min={1} max={65535} value={adminPort} disabled={running} onChange={setAdminPort} />}
        <ConfigInput id="generic-server-name" label="Server name" value={serverName} disabled={running} onChange={setServerName} />
        {!isDragonwilds && <ConfigInput id="generic-max-players" label="Max players" type="number" min={1} value={maxPlayers} disabled={running} onChange={setMaxPlayers} />}
        {game === 'vrising' && <ConfigInput id="vrising-rcon-password" label="RCON password" type="password" value={rconPassword} disabled={running} onChange={setRconPassword} required={rconEnabled} />}
        {game === 'palworld' && <ConfigInput id="palworld-admin-password" label="REST API password" type="password" value={adminPassword} disabled={running} onChange={setAdminPassword} required={restEnabled} />}
        {isDragonwilds && <>
          <ConfigInput id="dragonwilds-owner-id" label="Owner ID" value={ownerId} disabled={running} onChange={setOwnerId} />
          <ConfigInput id="dragonwilds-world" label="Default world" value={worldName} disabled={running} onChange={setWorldName} />
          <ConfigInput id="dragonwilds-admin-password" label="Administration password" type="password" value={adminPassword} disabled={running} onChange={setAdminPassword} required={false} />
          <ConfigInput id="dragonwilds-world-password" label="World password" type="password" value={worldPassword} disabled={running} onChange={setWorldPassword} required={false} />
        </>}
      </div>
      {game === 'vrising' && <div className="flex items-center justify-between rounded-xl border p-3"><Label htmlFor="vrising-rcon">Enable local RCON</Label><Switch id="vrising-rcon" checked={rconEnabled} disabled={running} onCheckedChange={setRconEnabled} /></div>}
      {game === 'palworld' && <div className="flex items-center justify-between rounded-xl border p-3"><Label htmlFor="palworld-rest">Enable local REST API</Label><Switch id="palworld-rest" checked={restEnabled} disabled={running} onCheckedChange={setRestEnabled} /></div>}
      <div className="flex items-center justify-between rounded-xl border p-3"><div><Label htmlFor="generic-auto-restart">Restart automatically</Label><p className="text-xs text-muted-foreground">Restart this server after an unexpected exit.</p></div><Switch id="generic-auto-restart" checked={autoRestart} disabled={running} onCheckedChange={setAutoRestart} /></div>
      <Button className="w-fit" type="submit" disabled={running || update.isPending}>Save configuration</Button>
    </form>
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
  const logs = useManagedInstanceLogs(detailForRequests?.game ?? gameId, detailForRequests?.name ?? name ?? '', 200, detailForRequests?.id)
  const liveLogs = useLogSocket(detailForRequests?.name ?? name ?? '', detailForRequests?.game ?? gameId, detailForRequests?.id)
  const consoleLines = liveLogs.lines.length > 0 ? liveLogs.lines : (logs.data?.lines ?? [])
  const errorLines = consoleLines.filter(isConsoleError)
  if (!id && (!isGameId(game) || !name)) return null
  if (instance.isError) return <QueryError error={instance.error} />
  if (!instance.data) return null
  const detail = instance.data
  const rustConfig = detail.game === 'rust' ? asRustConfig(detail.config) : null
  const genericConfig = detail.game === 'vrising' || detail.game === 'palworld' || detail.game === 'runescape-dragonwilds' ? asGenericConfig(detail.config) : null
  const tabs = [
    { id: 'logs', label: 'Logs' },
    ...(detail.game === 'rust' ? [{ id: 'rcon', label: 'RCON' }] : []),
    ...(detail.game === 'vrising' ? [{ id: 'rcon', label: 'RCON' }] : []),
    { id: 'errors', label: 'Errors' },
    { id: 'config', label: 'Config' },
    ...(detail.game === 'palworld' ? [{ id: 'admin', label: 'Admin' }] : []),
    ...(detail.capabilities.access_lists ? [{ id: 'lists', label: 'Access lists' }] : []),
    ...(detail.capabilities.backups ? [{ id: 'backups', label: 'Backups' }] : []),
    { id: 'saves', label: 'Save files' },
    { id: 'resources', label: 'Resources' },
  ]
  const [tab, ...nestedPath] = tabPath?.split('/').filter(Boolean) ?? []
  const listsBasePath = id ? `/instance/${id}/lists` : `/instances/${detail.game}/${detail.name}/lists`
  const defaultListTab = detail.game === 'vrising' ? 'admin' : 'owner'
  if (!tab || (tab !== 'lists' && nestedPath.length > 0) || !tabs.some((candidate) => candidate.id === tab)) {
    return <Navigate replace to={id ? `/instance/${id}/logs` : `/instances/${detail.game}/${detail.name}/logs`} />
  }

  return (
    <div className="flex flex-col gap-6">
      <ManagedInstanceHeader instance={detail} />
      <Tabs value={tab} onValueChange={(value) => navigate(id ? (value === 'lists' ? `/instance/${id}/lists/${defaultListTab}` : `/instance/${id}/${value}`) : (value === 'lists' ? `/instances/${detail.game}/${detail.name}/lists/${defaultListTab}` : `/instances/${detail.game}/${detail.name}/${value}`))}>
        <div className="overflow-x-auto">
          <TabsList className="w-max">
          {tabs.map((item) => <TabsTrigger key={item.id} value={item.id}>{item.label}</TabsTrigger>)}
          </TabsList>
        </div>
        <TabsContent value="logs"><ManagedLogsTab id={detail.id} game={detail.game} name={detail.name} /></TabsContent>
        {detail.game === 'rust' && <TabsContent value="rcon"><RustRconTab id={detail.id} running={detail.running} /></TabsContent>}
        {detail.game === 'vrising' && <TabsContent value="rcon"><VRisingRconTab id={detail.id} running={detail.running} /></TabsContent>}
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
            ? <RustConfigForm key={`${detail.id}-${JSON.stringify(detail.config)}`} id={detail.id} name={detail.name} config={rustConfig} running={detail.running} />
            : genericConfig && (detail.game === 'vrising' || detail.game === 'palworld' || detail.game === 'runescape-dragonwilds')
              ? <GenericConfigForm key={`${detail.id}-${JSON.stringify(detail.config)}`} id={detail.id} game={detail.game} name={detail.name} config={genericConfig} running={detail.running} />
              : <p className="text-sm text-muted-foreground">No editable configuration is available for this game.</p>}
          <UptimeScheduleCard id={detail.id} />
          {detail.game === 'rust' && <WipeMapCard id={detail.id} name={detail.name} running={detail.running} />}
        </TabsContent>
        {detail.game === 'palworld' && <TabsContent value="admin"><PalworldAdminTab id={detail.id} running={detail.running} /></TabsContent>}
        {detail.capabilities.backups && (
          <TabsContent value="backups"><BackupsTab id={detail.id} name={detail.name} game={detail.game} running={detail.running} /></TabsContent>
        )}
        {detail.game === 'rust' && tab === 'lists' && (
          <TabsContent value="lists"><RustAccessListsTab id={detail.id} path={nestedPath} running={detail.running} basePath={listsBasePath} /></TabsContent>
        )}
        {detail.game === 'vrising' && tab === 'lists' && (
          <TabsContent value="lists"><VRisingAccessListsTab id={detail.id} path={nestedPath} basePath={listsBasePath} /></TabsContent>
        )}
        <TabsContent value="saves"><SaveFilesTab id={detail.id} game={detail.game} name={detail.name} /></TabsContent>
        <TabsContent value="resources"><ManagedResourcesTab id={detail.id} running={detail.running} /></TabsContent>
      </Tabs>
    </div>
  )
}
