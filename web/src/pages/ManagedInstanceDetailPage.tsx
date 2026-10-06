import { RustAccessListsTab } from '@/components/instance/RustAccessListsTab'
import { VRisingAccessListsTab } from '@/components/instance/VRisingAccessListsTab'
import { VRisingRconTab } from '@/components/instance/VRisingRconTab'
import { PalworldAdminTab } from '@/components/instance/PalworldAdminTab'
import { RustRconTab } from '@/components/instance/RustRconTab'
import { WipeMapCard } from '@/components/instance/WipeMapCard'
import { UptimeScheduleCard } from '@/components/instance/UptimeScheduleCard'
import { AccessListsTab } from '@/components/instance/AccessListsTab'
import { ConfigTab } from '@/components/instance/ConfigTab'
import { PlayersTab } from '@/components/instance/PlayersTab'
import { ModsTab } from '@/components/instance/ModsTab'
import { SevenDaysModsTab } from '@/components/instance/SevenDaysModsTab'
import { SevenDaysConsoleTab } from '@/components/instance/SevenDaysConsoleTab'
import { SevenDaysPlayersTab } from '@/components/instance/SevenDaysPlayersTab'
import { SevenDaysSandboxCodeEditor } from '@/components/instance/SevenDaysSandboxCodeEditor'
import { useEffect, useState } from 'react'
import { BackupsTab } from '@/components/instance/BackupsTab'
import { SaveFilesTab } from '@/components/instance/SaveFilesTab'
import { Navigate, useNavigate, useParams } from 'react-router-dom'
import { toast } from 'sonner'
import { ManagedInstanceHeader } from '@/components/instance/ManagedInstanceHeader'
import { ManagedLogsTab } from '@/components/instance/ManagedLogsTab'
import { ManagedResourcesTab } from '@/components/instance/ManagedResourcesTab'
import { ResourceLimitsTab } from '@/components/instance/ResourceLimitsTab'
import { LiveLogOutput } from '@/components/instance/LiveLogOutput'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { QueryError } from '@/components/QueryError'
import {
  useManagedInstanceById,
  useManagedInstances,
  useManagedInstanceLogs,
  useUpdateGenericConfig,
  useUpdateRustConfig,
  useAdvancedConfig,
  useUpdateAdvancedConfig,
} from '@/lib/queries'
import type { AdvancedConfigChange, GameId, GenericConfigUpdateRequest } from '@/lib/types'
import { useLogSocket } from '@/hooks/useLogSocket'

function isConsoleError(line: string) {
  return /(?:^|\W)(?:error|exception|fatal|assertion failed|stack trace)(?:\W|$)/i.test(line)
}

function isGameId(value: string | undefined): value is GameId {
  return value === 'valheim' || value === 'rust' || value === 'vrising' || value === 'palworld' || value === 'runescape-dragonwilds' || value === '7d2d'
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

type GenericConfig = GenericConfigUpdateRequest & { configuredPasswords: Record<string, boolean> }

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
  const configuredPasswords = config.passwords_configured
  const settings = { ...config }
  delete settings.port
  delete settings.query_port
  delete settings.admin_port
  delete settings.auto_restart
  delete settings.passwords_configured
  if (typeof port !== 'number' || (typeof queryPort !== 'number' && queryPort !== null) || (typeof adminPort !== 'number' && adminPort !== null) || typeof config.auto_restart !== 'boolean') return null
  return {
    port,
    query_port: queryPort,
    admin_port: adminPort,
    settings,
    auto_restart: config.auto_restart,
    configuredPasswords: configuredPasswords !== null && typeof configuredPasswords === 'object' && !Array.isArray(configuredPasswords)
      ? Object.fromEntries(Object.entries(configuredPasswords).filter(([, value]) => typeof value === 'boolean')) as Record<string, boolean>
      : {},
  }
}

function RustConfigForm({ id, config, running }: { id: string; config: RustConfig; running: boolean }) {
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

function ConfigInput({ id, label, type = 'text', value, disabled, onChange, min, max, placeholder, required = true }: {
  id: string
  label: string
  type?: 'text' | 'number' | 'password'
  value: string | number
  disabled: boolean
  onChange: (value: string) => void
  min?: number
  max?: number
  placeholder?: string
  required?: boolean
}) {
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>{label}</Label>
      <Input id={id} type={type} min={min} max={max} placeholder={placeholder} required={required} value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} />
    </div>
  )
}

function AdvancedConfigSection({ id, running }: { id: string; running: boolean }) {
  const config = useAdvancedConfig(id, true)
  const update = useUpdateAdvancedConfig(id)
  const [values, setValues] = useState<Record<string, string>>({})
  useEffect(() => {
    const initial: Record<string, string> = {}
    for (const file of config.data?.files ?? []) for (const entry of file.entries) initial[`${file.id}:${entry.key}`] = entry.value
    setValues(initial)
  }, [config.data])
  if (config.isLoading || config.isError) return null
  const files = config.data?.files ?? []
  if (files.every((file) => file.entries.length === 0)) return null
  const save = () => {
    const changes: AdvancedConfigChange[] = []
    for (const file of files) for (const entry of file.entries) {
      const value = values[`${file.id}:${entry.key}`] ?? entry.value
      if (value !== entry.value) changes.push({ file: file.id, key: entry.key, value })
    }
    if (changes.length > 0) update.mutate(changes, { onSuccess: () => toast.success('Advanced configuration saved'), onError: (error) => toast.error(error.message) })
  }
  return <details className="rounded-xl border p-4"><summary className="cursor-pointer text-sm font-medium">Advanced configuration</summary><p className="mt-2 text-xs text-muted-foreground">Settings found in declared server files. Stop the server before saving.</p><div className="mt-4 flex flex-col gap-5">{files.filter((file) => file.entries.length > 0).map((file) => <div key={file.id} className="flex flex-col gap-3"><div><p className="text-sm font-medium">{file.path}</p><p className="text-xs text-muted-foreground">{file.format}</p></div><div className="grid gap-3 sm:grid-cols-2">{file.entries.map((entry) => <div key={entry.key} className="flex flex-col gap-1"><Label htmlFor={`advanced-${file.id}-${entry.key}`}>{entry.key}</Label><Input id={`advanced-${file.id}-${entry.key}`} type={entry.sensitive ? 'password' : 'text'} placeholder={entry.sensitive && entry.configured ? '••••••••' : undefined} value={values[`${file.id}:${entry.key}`] ?? entry.value} disabled={running} onChange={(event) => setValues((current) => ({ ...current, [`${file.id}:${entry.key}`]: event.target.value }))} /></div>)}</div></div>)}<Button className="w-fit" type="button" disabled={running || update.isPending} onClick={save}>Save advanced configuration</Button></div></details>
}

function GenericConfigForm({ id, game, config, running }: { id: string; game: Extract<GameId, 'vrising' | 'palworld' | 'runescape-dragonwilds' | '7d2d'>; config: GenericConfig; running: boolean }) {
  const update = useUpdateGenericConfig()
  const [port, setPort] = useState(String(config.port))
  const [queryPort, setQueryPort] = useState(config.query_port === null ? '' : String(config.query_port))
  const [adminPort, setAdminPort] = useState(config.admin_port === null ? '' : String(config.admin_port))
  const [serverName, setServerName] = useState(typeof config.settings.server_name === 'string' ? config.settings.server_name : '')
  const [maxPlayers, setMaxPlayers] = useState(typeof config.settings.max_players === 'number' ? String(config.settings.max_players) : '32')
  const [ownerId, setOwnerId] = useState(typeof config.settings.owner_id === 'string' ? config.settings.owner_id : '')
  const [worldName, setWorldName] = useState(typeof config.settings.default_world_name === 'string' ? config.settings.default_world_name : '')
  const [adminPassword, setAdminPassword] = useState(typeof config.settings.admin_password === 'string' ? config.settings.admin_password : '')
  const [serverPassword, setServerPassword] = useState(typeof config.settings.server_password === 'string' ? config.settings.server_password : '')
  const [worldPassword, setWorldPassword] = useState(typeof config.settings.world_password === 'string' ? config.settings.world_password : '')
  const [rconPassword, setRconPassword] = useState(typeof config.settings.rcon_password === 'string' ? config.settings.rcon_password : '')
  const [autoRestart, setAutoRestart] = useState(config.auto_restart)
  const [rconEnabled, setRconEnabled] = useState(config.settings.rcon_enabled === true)
  const [restEnabled, setRestEnabled] = useState(config.settings.rest_api_enabled === true)
  const isDragonwilds = game === 'runescape-dragonwilds'
  const isSevenDays = game === '7d2d'
  const [description, setDescription] = useState(typeof config.settings.server_description === 'string' ? config.settings.server_description : '')
  const [visibility, setVisibility] = useState(typeof config.settings.visibility === 'number' ? String(config.settings.visibility) : '2')
  const [gameWorld, setGameWorld] = useState(typeof config.settings.game_world === 'string' ? config.settings.game_world : 'Navezgane')
  const [gameName, setGameName] = useState(typeof config.settings.game_name === 'string' ? config.settings.game_name : '')
  const [worldSeed, setWorldSeed] = useState(typeof config.settings.world_gen_seed === 'string' ? config.settings.world_gen_seed : '')
  const [worldSize, setWorldSize] = useState(typeof config.settings.world_gen_size === 'number' ? String(config.settings.world_gen_size) : '6144')
  const [sandboxCode, setSandboxCode] = useState(typeof config.settings.sandbox_code === 'string' ? config.settings.sandbox_code : '')
  const [telnetEnabled, setTelnetEnabled] = useState(config.settings.telnet_enabled === true)
  const [telnetPassword, setTelnetPassword] = useState(typeof config.settings.telnet_password === 'string' ? config.settings.telnet_password : '')

  const save = () => {
    const settings: Record<string, unknown> = { ...config.settings, server_name: serverName }
    if (game === 'vrising') Object.assign(settings, { max_players: Number(maxPlayers), rcon_enabled: rconEnabled, rcon_password: rconPassword })
    if (game === 'palworld') Object.assign(settings, { max_players: Number(maxPlayers), rest_api_enabled: restEnabled, admin_password: adminPassword, server_password: serverPassword })
    if (isDragonwilds) Object.assign(settings, { owner_id: ownerId, default_world_name: worldName, admin_password: adminPassword, world_password: worldPassword })
    if (isSevenDays) {
      Object.assign(settings, { server_description: description, server_password: serverPassword, visibility: Number(visibility), max_players: Number(maxPlayers), game_world: gameWorld, game_name: gameName, world_gen_seed: worldSeed, world_gen_size: Number(worldSize), telnet_enabled: telnetEnabled, telnet_password: telnetPassword })
      const code = sandboxCode.trim()
      if (code) settings.sandbox_code = code
      else delete settings.sandbox_code
    }
    update.mutate({
      id,
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
      <p className="text-sm text-muted-foreground">Stop this server before changing its configuration. Dots indicate an existing password; leave the field unchanged to keep it.</p>
      <div className="grid gap-4 sm:grid-cols-2">
        <ConfigInput id="generic-port" label="Game port" type="number" min={1} max={65535} value={port} disabled={running} onChange={setPort} />
        {config.query_port !== null && <ConfigInput id="generic-query-port" label={isDragonwilds ? 'Beacon port' : 'Query port'} type="number" min={1} max={65535} value={queryPort} disabled={running} onChange={setQueryPort} />}
        {config.admin_port !== null && <ConfigInput id="generic-admin-port" label={game === 'palworld' ? 'REST API port' : isSevenDays ? 'Local console port' : 'RCON port'} type="number" min={1} max={65535} value={adminPort} disabled={running} onChange={setAdminPort} />}
        <ConfigInput id="generic-server-name" label="Server name" value={serverName} disabled={running} onChange={setServerName} />
        {!isDragonwilds && <ConfigInput id="generic-max-players" label="Max players" type="number" min={1} value={maxPlayers} disabled={running} onChange={setMaxPlayers} />}
        {game === 'vrising' && <ConfigInput id="vrising-rcon-password" label="RCON password" type="password" value={rconPassword} disabled={running} onChange={setRconPassword} required={rconEnabled} />}
        {game === 'palworld' && <ConfigInput id="palworld-admin-password" label="REST API password" type="password" value={adminPassword} disabled={running} onChange={setAdminPassword} placeholder={config.configuredPasswords.admin_password ? '••••••••' : undefined} required={false} />}
        {game === 'palworld' && <ConfigInput id="palworld-server-password" label="Server password" type="password" value={serverPassword} disabled={running} onChange={setServerPassword} placeholder={config.configuredPasswords.server_password ? '••••••••' : undefined} />}
        {isDragonwilds && <>
          <ConfigInput id="dragonwilds-owner-id" label="Owner ID" value={ownerId} disabled={running} onChange={setOwnerId} />
          <ConfigInput id="dragonwilds-world" label="Default world" value={worldName} disabled={running} onChange={setWorldName} />
          <ConfigInput id="dragonwilds-admin-password" label="Administration password" type="password" value={adminPassword} disabled={running} onChange={setAdminPassword} required={false} />
          <ConfigInput id="dragonwilds-world-password" label="World password" type="password" value={worldPassword} disabled={running} onChange={setWorldPassword} required={false} />
        </>}
        {isSevenDays && <>
          <ConfigInput id="7d2d-description" label="Description" value={description} disabled={running} onChange={setDescription} />
          <ConfigInput id="7d2d-password" label="Server password" type="password" value={serverPassword} disabled={running} onChange={setServerPassword} placeholder={config.configuredPasswords.server_password ? '••••••••' : undefined} />
          <ConfigInput id="7d2d-visibility" label="Visibility (0 hidden, 2 public)" type="number" min={0} max={2} value={visibility} disabled={running} onChange={setVisibility} />
          <ConfigInput id="7d2d-world" label="World (Navezgane or RWG)" value={gameWorld} disabled={running} onChange={setGameWorld} />
          <ConfigInput id="7d2d-game-name" label="Save name" value={gameName} disabled={running} onChange={setGameName} />
          <ConfigInput id="7d2d-seed" label="RWG seed" value={worldSeed} disabled={running} onChange={setWorldSeed} />
          <ConfigInput id="7d2d-size" label="RWG size" type="number" value={worldSize} disabled={running} onChange={setWorldSize} />
          <SevenDaysSandboxCodeEditor value={sandboxCode} disabled={running} onChange={setSandboxCode} />
          <div className="flex flex-col gap-2 sm:col-span-2"><Label htmlFor="7d2d-console-password">Local console password</Label><Input id="7d2d-console-password" type="password" value={telnetPassword} disabled={running || !telnetEnabled} placeholder={config.configuredPasswords.telnet_password ? '••••••••••••' : 'At least 12 characters'} onChange={(event) => setTelnetPassword(event.target.value)} /><p className="text-xs text-muted-foreground">Odin connects to this password-protected console through loopback. Keep its port blocked from external networks.</p></div>
        </>}
      </div>
      {game === 'vrising' && <div className="flex items-center justify-between rounded-xl border p-3"><Label htmlFor="vrising-rcon">Enable local RCON</Label><Switch id="vrising-rcon" checked={rconEnabled} disabled={running} onCheckedChange={setRconEnabled} /></div>}
      {game === 'palworld' && <div className="flex items-center justify-between rounded-xl border p-3"><Label htmlFor="palworld-rest">Enable local REST API</Label><Switch id="palworld-rest" checked={restEnabled} disabled={running} onCheckedChange={setRestEnabled} /></div>}
      {isSevenDays && <div className="flex items-center justify-between rounded-xl border p-3"><div><Label htmlFor="7d2d-console">Enable local server console</Label><p className="text-xs text-muted-foreground">Required for dashboard commands and player actions.</p></div><Switch id="7d2d-console" checked={telnetEnabled} disabled={running} onCheckedChange={setTelnetEnabled} /></div>}
      <div className="flex items-center justify-between rounded-xl border p-3"><div><Label htmlFor="generic-auto-restart">Restart automatically</Label><p className="text-xs text-muted-foreground">Restart this server after an unexpected exit.</p></div><Switch id="generic-auto-restart" checked={autoRestart} disabled={running} onCheckedChange={setAutoRestart} /></div>
      <Button className="w-fit" type="submit" disabled={running || update.isPending}>Save configuration</Button>
      <AdvancedConfigSection id={id} running={running} />
    </form>
  )
}

export function ManagedInstanceDetailPage() {
  const { id, game, name, '*': tabPath } = useParams<{ id: string; game: string; name: string; '*': string }>()
  const navigate = useNavigate()
  const gameId = isGameId(game) ? game : 'valheim'
  const instanceById = useManagedInstanceById(id ?? '')
  const managedInstances = useManagedInstances()
  const legacyInstance = managedInstances.data?.find((candidate) => candidate.game === gameId && candidate.name === name)
  const instance = id
    ? instanceById
    : { data: legacyInstance, isError: managedInstances.isError, error: managedInstances.error }
  const detailForRequests = instance.data
  const logs = useManagedInstanceLogs(detailForRequests?.id ?? '', 200)
  const liveLogs = useLogSocket(detailForRequests?.id ?? '')
  const consoleLines = liveLogs.lines.length > 0 ? liveLogs.lines : (logs.data?.lines ?? [])
  const errorLines = consoleLines.filter(isConsoleError)
  if (!id && (!isGameId(game) || !name)) return null
  if (instance.isError) return <QueryError error={instance.error} />
  if (!instance.data) return null
  const detail = instance.data
  // Keep bookmarks to the historical game/name URL working, but immediately
  // replace them with the immutable UUID route so a later rename is safe.
  if (!id) {
    return <Navigate replace to={`/instance/${detail.id}${tabPath ? `/${tabPath}` : ''}`} />
  }
  const rustConfig = detail.game === 'rust' ? asRustConfig(detail.config) : null
  const genericConfig = detail.game === 'vrising' || detail.game === 'palworld' || detail.game === 'runescape-dragonwilds' || detail.game === '7d2d' ? asGenericConfig(detail.config) : null
  const tabs = [
    { id: 'logs', label: 'Logs' },
    ...(detail.game === 'rust' ? [{ id: 'rcon', label: 'RCON' }] : []),
    ...(detail.game === 'vrising' ? [{ id: 'rcon', label: 'RCON' }] : []),
    ...(detail.game === '7d2d' ? [{ id: 'console', label: 'Console' }, { id: 'players', label: 'Players' }] : []),
    { id: 'errors', label: 'Errors' },
    { id: 'config', label: 'Config' },
    ...((detail.game === 'valheim' || detail.game === '7d2d') && detail.capabilities.mods ? [{ id: 'mods', label: 'Mods' }] : []),
    ...(detail.game === 'palworld' ? [{ id: 'admin', label: 'Admin' }] : []),
    ...((detail.game === 'valheim' || detail.game === 'rust' || detail.game === 'vrising') && detail.capabilities.access_lists ? [{ id: 'lists', label: 'Access lists' }] : []),
    ...(detail.capabilities.backups ? [{ id: 'backups', label: 'Backups' }] : []),
    { id: 'saves', label: 'Save files' },
    { id: 'resources', label: 'Resources' },
    { id: 'limits', label: 'Limits' },
    ...(detail.game === 'valheim' && detail.capabilities.players ? [{ id: 'players', label: 'Players' }] : []),
  ]
  const [tab, ...nestedPath] = tabPath?.split('/').filter(Boolean) ?? []
  const listsBasePath = `/instance/${id}/lists`
  const defaultListTab = detail.game === 'vrising' || detail.game === 'valheim' ? 'admin' : 'owner'
  if (!tab || (tab !== 'lists' && tab !== 'mods' && nestedPath.length > 0) || !tabs.some((candidate) => candidate.id === tab)) {
    return <Navigate replace to={`/instance/${id}/logs`} />
  }

  return (
    <div className="flex flex-col gap-6">
      <ManagedInstanceHeader instance={detail} />
      <Tabs value={tab} onValueChange={(value) => navigate(value === 'lists' ? `/instance/${id}/lists/${defaultListTab}` : `/instance/${id}/${value}`)}>
        <div className="overflow-x-auto">
          <TabsList className="w-max">
          {tabs.map((item) => <TabsTrigger key={item.id} value={item.id}>{item.label}</TabsTrigger>)}
          </TabsList>
        </div>
        <TabsContent value="logs"><ManagedLogsTab id={detail.id} /></TabsContent>
        {detail.game === 'rust' && <TabsContent value="rcon"><RustRconTab id={detail.id} running={detail.running} /></TabsContent>}
        {detail.game === 'vrising' && <TabsContent value="rcon"><VRisingRconTab id={detail.id} running={detail.running} /></TabsContent>}
        {detail.game === '7d2d' && <TabsContent value="console"><SevenDaysConsoleTab id={detail.id} running={detail.running} /></TabsContent>}
        {detail.game === '7d2d' && <TabsContent value="players"><SevenDaysPlayersTab id={detail.id} running={detail.running} /></TabsContent>}
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
          {detail.game === 'valheim'
            ? <ConfigTab id={detail.id} />
            : rustConfig
            ? <RustConfigForm key={`${detail.id}-${JSON.stringify(detail.config)}`} id={detail.id} config={rustConfig} running={detail.running} />
            : genericConfig && (detail.game === 'vrising' || detail.game === 'palworld' || detail.game === 'runescape-dragonwilds' || detail.game === '7d2d')
              ? <GenericConfigForm key={`${detail.id}-${JSON.stringify(detail.config)}`} id={detail.id} game={detail.game} config={genericConfig} running={detail.running} />
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
        {detail.game === 'valheim' && tab === 'lists' && (
          <TabsContent value="lists"><AccessListsTab id={detail.id} path={nestedPath} /></TabsContent>
        )}
        {detail.game === 'valheim' && tab === 'mods' && (
          <TabsContent value="mods"><ModsTab id={detail.id} name={detail.name} running={detail.running} path={nestedPath} /></TabsContent>
        )}
        {detail.game === '7d2d' && tab === 'mods' && <TabsContent value="mods"><SevenDaysModsTab id={detail.id} running={detail.running} /></TabsContent>}
        <TabsContent value="saves"><SaveFilesTab id={detail.id} /></TabsContent>
        <TabsContent value="resources"><ManagedResourcesTab id={detail.id} running={detail.running} /></TabsContent>
        <TabsContent value="limits"><ResourceLimitsTab id={detail.id} /></TabsContent>
        {detail.game === 'valheim' && <TabsContent value="players"><PlayersTab id={detail.id} running={detail.running} /></TabsContent>}
      </Tabs>
    </div>
  )
}
