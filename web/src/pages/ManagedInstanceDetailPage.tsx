import { RustAccessListsTab } from '@/components/instance/RustAccessListsTab'
import { PalworldAdminTab } from '@/components/instance/PalworldAdminTab'
import { RustRconTab } from '@/components/instance/RustRconTab'
import { WipeMapCard } from '@/components/instance/WipeMapCard'
import { SharedInstallMigrationCard } from '@/components/instance/SharedInstallMigrationCard'
import { UptimeScheduleCard } from '@/components/instance/UptimeScheduleCard'
import { AccessListsTab } from '@/components/instance/AccessListsTab'
import { ConfigTab } from '@/components/instance/ConfigTab'
import { PlayersTab } from '@/components/instance/PlayersTab'
import { ModsTab } from '@/components/instance/ModsTab'
import { SevenDaysModsTab } from '@/components/instance/SevenDaysModsTab'
import { SevenDaysConsoleTab } from '@/components/instance/SevenDaysConsoleTab'
import { SevenDaysPlayersTab } from '@/components/instance/SevenDaysPlayersTab'
import { NativeConfigCodeEditor } from '@/components/instance/NativeConfigCodeEditor'
import { type ReactNode, useEffect, useState } from 'react'
import { BackupsTab } from '@/components/instance/BackupsTab'
import { SaveFilesTab } from '@/components/instance/SaveFilesTab'
import { Link, Navigate, useParams } from 'react-router-dom'
import { toast } from 'sonner'
import { ManagedInstanceHeader } from '@/components/instance/ManagedInstanceHeader'
import { ManagedLogsTab } from '@/components/instance/ManagedLogsTab'
import { ManagedResourcesTab } from '@/components/instance/ManagedResourcesTab'
import { ResourceLimitsTab } from '@/components/instance/ResourceLimitsTab'
import { RuntimeHealthCard } from '@/components/instance/RuntimeHealthCard'
import { LiveLogOutput } from '@/components/instance/LiveLogOutput'
import { Button } from '@/components/ui/button'
import { buttonVariants } from '@/components/ui/button-variants'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { RouteTabs } from '@/components/RouteTabs'
import { GameBackdrop } from '@/components/GameBackdrop'
import { Skeleton } from '@/components/ui/skeleton'
import { QueryError } from '@/components/QueryError'
import {
  useManagedInstanceById,
  useManagedInstances,
  useManagedInstanceLogs,
  useUpdateGenericConfig,
  useUpdateRustConfig,
  useAdvancedConfig,
  useAdoptSevenDaysTemplateBaseline,
  useApplySevenDaysTemplateReview,
  useInitializeAdvancedConfig,
  useSevenDaysTemplateReview,
  useUpdateAdvancedConfig,
  useRawAdvancedConfig,
  useUpdateRawAdvancedConfig,
} from '@/lib/queries'
import type { AdvancedConfigChange, Cs2GameMode, GameId, GenericConfigUpdateRequest } from '@/lib/types'
import { useLogSocket } from '@/hooks/useLogSocket'
import { stripAnsi } from '@/lib/ansi'

function isConsoleError(line: string) {
  return /(?:^|\W)(?:error|exception|fatal|assertion failed|stack trace)(?:\W|$)/i.test(stripAnsi(line))
}

function isGameId(value: string | undefined): value is GameId {
  return value === 'ark-asa' || value === 'valheim' || value === 'rust' || value === 'palworld' || value === '7d2d' || value === 'cs2'
}

type RustConfig = {
  port: number
  queryPort: number
  rconPort: number
  rconPassword: string
  autoRestart: boolean
}

type GenericConfig = GenericConfigUpdateRequest & { has_gslt?: boolean; tv_port?: number }

const CS2_GAME_MODES: { id: Cs2GameMode; label: string }[] = [
  { id: 'casual', label: 'Casual' },
  { id: 'competitive', label: 'Competitive' },
  { id: 'wingman', label: 'Wingman' },
  { id: 'deathmatch', label: 'Deathmatch' },
]

function asRustConfig(config: Record<string, unknown>): RustConfig | null {
  const port = config.port
  const queryPort = config.query_port
  const rconPort = config.rcon_port
  const rconPassword = config.rcon_password
  const autoRestart = config.auto_restart
  if (
    typeof port !== 'number' || typeof queryPort !== 'number' || typeof rconPort !== 'number' || typeof rconPassword !== 'string' || typeof autoRestart !== 'boolean'
  ) return null
  return { port, queryPort, rconPort, rconPassword, autoRestart }
}

function asGenericConfig(config: Record<string, unknown>): GenericConfig | null {
  const port = config.port
  const queryPort = config.query_port
  const adminPort = config.admin_port
  if (typeof port !== 'number' || (typeof queryPort !== 'number' && queryPort !== null) || (typeof adminPort !== 'number' && adminPort !== null) || typeof config.auto_restart !== 'boolean') return null
  const arkConfig = typeof config.map === 'string'
    && typeof config.session_name === 'string'
    && Array.isArray(config.mods)
    && config.mods.every((mod) => typeof mod === 'number')
    ? { map: config.map, session_name: config.session_name, mods: config.mods }
    : {}
  const cs2Config = typeof config.map === 'string'
    && typeof config.game_mode === 'string'
    && typeof config.max_players === 'number'
    ? { map: config.map, game_mode: config.game_mode as Cs2GameMode, max_players: config.max_players, has_gslt: config.has_gslt === true, ...(typeof config.tv_port === 'number' ? { tv_port: config.tv_port } : {}) }
    : {}
  return {
    port,
    query_port: queryPort,
    admin_port: adminPort,
    auto_restart: config.auto_restart,
    ...arkConfig,
    ...cs2Config,
  }
}

function RustConfigForm({ id, config, running }: { id: string; config: RustConfig; running: boolean }) {
  const update = useUpdateRustConfig()
  const [port, setPort] = useState(String(config.port))
  const [queryPort, setQueryPort] = useState(String(config.queryPort))
  const [rconPort, setRconPort] = useState(String(config.rconPort))
  const [rconPassword, setRconPassword] = useState(config.rconPassword)
  const [autoRestart, setAutoRestart] = useState(config.autoRestart)

  const save = () => update.mutate(
    {
      id,
      request: {
        port: Number(port),
        query_port: Number(queryPort),
        rcon_port: Number(rconPort),
        rcon_password: rconPassword,
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
      </div>
      <div className="flex items-center justify-between rounded-xl border p-3">
        <div>
          <Label htmlFor="rust-auto-restart">Restart automatically</Label>
          <p className="text-xs text-muted-foreground">Restart this server after an unexpected exit.</p>
        </div>
        <Switch id="rust-auto-restart" checked={autoRestart} disabled={running} onCheckedChange={setAutoRestart} />
      </div>
      <p className="text-sm text-muted-foreground">Odin manages process ports, RCON, and restart behavior. Game rules and listing settings belong in server.cfg below.</p>
      <Button className="w-fit" type="submit" disabled={running || update.isPending}>Save Odin settings</Button>
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

function SevenDaysTemplateReview({ id, running }: { id: string; running: boolean }) {
  const review = useSevenDaysTemplateReview(id, true)
  const apply = useApplySevenDaysTemplateReview(id)
  const adopt = useAdoptSevenDaysTemplateBaseline(id)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  useEffect(() => setSelected(new Set()), [review.data])
  if (!review.data?.instance_config_exists || !review.data.template_exists) return null
  if (!review.data.baseline_exists) return <Card><CardHeader><CardTitle>Server config template</CardTitle><CardDescription>Track future 7 Days to Die template updates without changing this instance’s current settings.</CardDescription></CardHeader><CardContent><Button type="button" variant="outline" disabled={running || adopt.isPending} onClick={() => adopt.mutate(undefined, { onSuccess: () => toast.success('Configuration template baseline saved'), onError: (error) => toast.error(error.message) })}>Track template updates</Button></CardContent></Card>
  if (review.data.changes.length === 0) return null
  const labels = { added: 'New setting', removed: 'Removed setting', default_changed: 'Changed default', conflict: 'Manual review needed' }
  const toggle = (key: string, checked: boolean) => setSelected((current) => {
    const next = new Set(current)
    if (checked) next.add(key)
    else next.delete(key)
    return next
  })
  const confirm = () => apply.mutate([...selected], { onSuccess: () => toast.success(selected.size > 0 ? 'Selected template updates applied' : 'Template update reviewed'), onError: (error) => toast.error(error.message) })
  return <Card><CardHeader><CardTitle>Server config update available</CardTitle><CardDescription>Review changes from the installed 7 Days to Die template. Odin never overwrites settings that differ from the original template.</CardDescription></CardHeader><CardContent className="flex flex-col gap-3">{review.data.changes.map((change) => <label key={change.key} className="flex items-center gap-3 rounded-md border p-3 text-sm"><input type="checkbox" checked={selected.has(change.key)} disabled={!change.applyable || running || apply.isPending} onChange={(event) => toggle(change.key, event.target.checked)} /><span className="flex-1"><span className="font-medium">{change.key}</span><span className="block text-xs text-muted-foreground">{labels[change.kind]}</span></span></label>)}<Button className="w-fit" type="button" disabled={running || apply.isPending} onClick={confirm}>{selected.size > 0 ? 'Apply selected changes' : 'Keep instance settings'}</Button></CardContent></Card>
}

function AdvancedConfigSection({ id, game, running }: { id: string; game: GameId; running: boolean }) {
  const config = useAdvancedConfig(id, true)
  const update = useUpdateAdvancedConfig(id)
  const initialize = useInitializeAdvancedConfig(id)
  const [values, setValues] = useState<Record<string, string>>({})
  const [newEntries, setNewEntries] = useState<{ id: number; file: string; key: string; value: string }[]>([])
  const [deletedEntries, setDeletedEntries] = useState<Set<string>>(new Set())
  const [nextNewEntryId, setNextNewEntryId] = useState(0)
  const [search, setSearch] = useState('')
  useEffect(() => {
    const initial: Record<string, string> = {}
    for (const file of config.data?.files ?? []) for (const section of file.sections) for (const entry of section.entries) initial[`${file.id}:${entry.key}`] = entry.value
    setValues(initial)
    setNewEntries([])
    setDeletedEntries(new Set())
  }, [config.data])
  if (config.isLoading) return null
  if (config.isError) return <QueryError error={config.error} />
  const files = config.data?.files ?? []
  const missingFiles = files.filter((file) => !file.exists)
  const canInitialize = game === '7d2d' || game === 'palworld' || game === 'cs2'
  const normalizedSearch = search.trim().toLocaleLowerCase()
  const generatedFiles = files.filter((file) => file.exists)
  const visibleFiles = generatedFiles.map((file) => ({
    ...file,
    sections: file.sections.map((section) => ({ ...section, entries: section.entries.filter((entry) => entry.label.toLocaleLowerCase().includes(normalizedSearch)) })).filter((section) => section.entries.length > 0),
  })).filter((file) => file.sections.length > 0 || file.allows_new_keys)
  const save = () => {
    const changes: AdvancedConfigChange[] = []
    for (const file of generatedFiles) for (const section of file.sections) for (const entry of section.entries) {
      const entryId = `${file.id}:${entry.key}`
      if (deletedEntries.has(entryId)) {
        changes.push({ file: file.id, key: entry.key, value: '', delete: true })
        continue
      }
      const value = values[`${file.id}:${entry.key}`] ?? entry.value
      if (!entry.managed && value !== entry.value) changes.push({ file: file.id, key: entry.key, value })
    }
    for (const entry of newEntries) {
      if (!entry.key.trim()) {
        toast.error('Enter a configuration key')
        return
      }
      changes.push({ file: entry.file, key: entry.key.trim(), value: entry.value })
    }
    if (changes.length > 0) update.mutate(changes, { onSuccess: () => toast.success('Advanced configuration saved'), onError: (error) => toast.error(error.message) })
  }
  const addEntry = (file: string) => {
    setNewEntries((current) => [...current, { id: nextNewEntryId, file, key: '', value: '' }])
    setNextNewEntryId((current) => current + 1)
  }
  const updateNewEntry = (entryId: number, field: 'key' | 'value', value: string) => setNewEntries((current) => current.map((entry) => entry.id === entryId ? { ...entry, [field]: value } : entry))
  const toggleDeleted = (entryId: string) => setDeletedEntries((current) => {
    const next = new Set(current)
    if (next.has(entryId)) next.delete(entryId)
    else next.add(entryId)
    return next
  })
  return <div className="flex flex-col gap-4"><div><p className="text-sm text-muted-foreground">Settings come directly from native server files. Stop the server before saving.</p>{missingFiles.length > 0 && canInitialize ? <div className="mt-2 flex flex-wrap items-center gap-3"><p className="text-sm text-muted-foreground">Initialize missing files from the installed server template.</p><Button type="button" variant="outline" disabled={running || initialize.isPending} onClick={() => initialize.mutate(undefined, { onSuccess: () => toast.success('Configuration files initialized'), onError: (error) => toast.error(error.message) })}>Initialize config files</Button></div> : missingFiles.length > 0 && <p className="mt-2 text-sm text-muted-foreground">Missing: {missingFiles.map((file) => file.path).join(', ')}.</p>}</div>{generatedFiles.length > 0 && <Input aria-label="Search configuration keys" placeholder="Search configuration keys…" value={search} onChange={(event) => setSearch(event.target.value)} />}{visibleFiles.map((file) => <div key={file.id} className="rounded-xl border"><div className="border-b px-4 py-3"><p className="text-sm font-medium">{file.path}</p><p className="text-xs text-muted-foreground">{file.format}</p></div><div className="divide-y">{file.sections.map((section) => <details key={section.id} open={normalizedSearch.length > 0} className="group"><summary className="cursor-pointer px-4 py-3 text-sm font-medium">{section.label}</summary><div className="grid gap-3 border-t px-4 py-4 sm:grid-cols-2">{section.entries.map((entry) => { const entryId = `${file.id}:${entry.key}`; const deleted = deletedEntries.has(entryId); return <div key={entry.key} className="flex flex-col gap-1"><Label className={deleted ? 'text-muted-foreground line-through' : undefined} htmlFor={`advanced-${file.id}-${entry.key}`}>{entry.label}</Label><Input id={`advanced-${file.id}-${entry.key}`} type={entry.sensitive ? 'password' : 'text'} placeholder={entry.sensitive && entry.configured ? '••••••••' : entry.managed ? 'Managed by Odin' : undefined} value={values[entryId] ?? entry.value} disabled={running || entry.managed || deleted} onChange={(event) => setValues((current) => ({ ...current, [entryId]: event.target.value }))} />{file.allows_new_keys && !entry.managed && <Button className="w-fit" type="button" variant="outline" size="sm" disabled={running} onClick={() => toggleDeleted(entryId)}>{deleted ? 'Restore' : 'Delete'}</Button>}</div> })}</div></details>)}</div>{file.allows_new_keys && <div className="flex flex-col gap-3 border-t px-4 py-4"><div><p className="text-sm font-medium">Add configuration key</p><p className="text-xs text-muted-foreground">Use the game key and its value exactly as the server expects.</p></div>{newEntries.filter((entry) => entry.file === file.id).map((entry) => <div key={entry.id} className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]"><Input aria-label="New configuration key" placeholder="server.example" value={entry.key} disabled={running} onChange={(event) => updateNewEntry(entry.id, 'key', event.target.value)} /><Input aria-label="New configuration value" placeholder="Value" value={entry.value} disabled={running} onChange={(event) => updateNewEntry(entry.id, 'value', event.target.value)} /><Button type="button" variant="outline" disabled={running} onClick={() => setNewEntries((current) => current.filter((currentEntry) => currentEntry.id !== entry.id))}>Remove</Button></div>)}<Button className="w-fit" type="button" variant="outline" disabled={running} onClick={() => addEntry(file.id)}>Add key</Button></div>}<details className="border-t"><summary className="cursor-pointer px-4 py-3 text-sm font-medium">Edit source</summary><div className="p-4"><InstanceConfigCodeFile id={id} file={file.id} running={running} /></div></details></div>)}{generatedFiles.length > 0 && visibleFiles.length === 0 && <p className="text-sm text-muted-foreground">No configuration keys match “{search}”.</p>}{generatedFiles.length > 0 && <Button className="w-fit" type="button" disabled={running || update.isPending} onClick={save}>Save configuration</Button>}</div>
}

function InstanceConfigCodeFile({ id, file, running }: { id: string; file: string; running: boolean }) {
  const source = useRawAdvancedConfig(id, file, true)
  const save = useUpdateRawAdvancedConfig(id, file)
  if (source.isLoading) return <p className="text-sm text-muted-foreground">Loading source…</p>
  if (source.isError || !source.data) return <p className="text-sm text-destructive">Source could not be loaded.</p>
  return <NativeConfigCodeEditor key={`${file}:${source.data.content}`} content={source.data.content} format={source.data.format} disabled={running} saving={save.isPending} onSave={(content) => save.mutate(content, { onSuccess: () => toast.success('Configuration source saved'), onError: (error) => toast.error(error.message) })} />
}

function GenericConfigForm({ id, game, config, running, installMode }: { id: string; game: Extract<GameId, 'ark-asa' | 'palworld' | '7d2d' | 'cs2'>; config: GenericConfig; running: boolean; installMode: 'isolated' | 'shared' }) {
  const update = useUpdateGenericConfig()
  const [port, setPort] = useState(String(config.port))
  const [queryPort, setQueryPort] = useState(config.query_port === null ? '' : String(config.query_port))
  const [adminPort, setAdminPort] = useState(config.admin_port === null ? '' : String(config.admin_port))
  const [autoRestart, setAutoRestart] = useState(config.auto_restart)
  const [map, setMap] = useState(config.map ?? 'TheIsland_WP')
  const [sessionName, setSessionName] = useState(config.session_name ?? '')
  const [mods, setMods] = useState((config.mods ?? []).join(', '))
  const [gameMode, setGameMode] = useState<Cs2GameMode>(config.game_mode ?? 'competitive')
  const [maxPlayers, setMaxPlayers] = useState(String(config.max_players ?? 10))
  const [gslt, setGslt] = useState('')
  const [removeGslt, setRemoveGslt] = useState(false)
  const save = () => {
    const request: GenericConfigUpdateRequest = { port: Number(port), query_port: queryPort ? Number(queryPort) : null, admin_port: adminPort ? Number(adminPort) : null, auto_restart: autoRestart }
    if (game === 'ark-asa') {
      const parsedMods = mods.trim() === '' ? [] : mods.split(',').map((mod) => Number(mod.trim()))
      if (parsedMods.some((mod) => !Number.isSafeInteger(mod) || mod <= 0)) {
        toast.error('Mod IDs must be positive whole numbers separated by commas')
        return
      }
      request.map = map
      request.session_name = sessionName
      request.mods = parsedMods
    }
    if (game === 'cs2') {
      request.query_port = null
      request.admin_port = null
      request.map = map
      request.game_mode = gameMode
      request.max_players = Number(maxPlayers)
      if (removeGslt) request.gslt = ''
      else if (gslt.trim()) request.gslt = gslt.trim()
    }
    update.mutate({ id, request }, { onSuccess: () => toast.success('Odin runtime settings saved'), onError: (error) => toast.error(error.message) })
  }
  const sharedArk = game === 'ark-asa' && installMode === 'shared'
  return <div className="flex flex-col gap-6"><form className="flex flex-col gap-4" onSubmit={(event) => { event.preventDefault(); save() }}><p className="text-sm text-muted-foreground">Odin manages process ports and restart behavior. Game settings are below and come directly from the server-generated file.</p><div className="grid gap-4 sm:grid-cols-2"><ConfigInput id="generic-port" label={game === 'cs2' ? 'Game port (UDP and TCP, also used by RCON)' : 'Game port'} type="number" min={1} max={65535} value={port} disabled={running} onChange={setPort} />{config.query_port !== null && <ConfigInput id="generic-query-port" label={game === 'palworld' ? 'Steam query port' : 'Query port'} type="number" min={1} max={65535} value={queryPort} disabled={running} onChange={setQueryPort} />}{config.admin_port !== null && <ConfigInput id="generic-admin-port" label={game === 'palworld' ? 'REST API port' : game === 'ark-asa' ? 'RCON port' : 'Local console port'} type="number" min={1} max={65535} value={adminPort} disabled={running} onChange={setAdminPort} />}{game === 'cs2' && <><ConfigInput id="cs2-map" label="Start map" value={map} disabled={running} onChange={setMap} /><div className="flex flex-col gap-1"><Label htmlFor="cs2-game-mode">Game mode</Label><select id="cs2-game-mode" className="h-9 border bg-background px-3 text-sm" value={gameMode} disabled={running} onChange={(event) => setGameMode(event.target.value as Cs2GameMode)}>{CS2_GAME_MODES.map((mode) => <option key={mode.id} value={mode.id}>{mode.label}</option>)}</select></div><ConfigInput id="cs2-max-players" label="Max players" type="number" min={1} max={64} value={maxPlayers} disabled={running} onChange={setMaxPlayers} /><ConfigInput id="cs2-gslt" label="Game server login token" type="password" placeholder={config.has_gslt ? 'Configured — leave empty to keep it' : 'Optional; needed to be listed publicly'} value={gslt} disabled={running || removeGslt} onChange={setGslt} required={false} />{config.has_gslt && <div className="flex items-center gap-2 sm:col-span-2"><Switch id="cs2-remove-gslt" checked={removeGslt} disabled={running} onCheckedChange={setRemoveGslt} /><Label htmlFor="cs2-remove-gslt">Remove the saved token</Label></div>}{config.tv_port !== undefined && <p className="text-xs text-muted-foreground sm:col-span-2">SourceTV uses UDP port {config.tv_port}, derived from the game port.</p>}</>}{game === 'ark-asa' && <><ConfigInput id="ark-map" label="Map" value={map} disabled={running} onChange={setMap} /><ConfigInput id="ark-session-name" label="Session name" value={sessionName} disabled={running} onChange={setSessionName} /><ConfigInput id="ark-mods" label="Mods" placeholder="CurseForge IDs, comma-separated" value={mods} disabled={running} onChange={setMods} required={false} /></>}</div><div className="flex items-center justify-between rounded-xl border p-3"><div><Label htmlFor="generic-auto-restart">Restart automatically</Label><p className="text-xs text-muted-foreground">Restart this server after an unexpected exit.</p></div><Switch id="generic-auto-restart" checked={autoRestart} disabled={running} onCheckedChange={setAutoRestart} /></div><Button className="w-fit" type="submit" disabled={running || update.isPending}>Save Odin settings</Button></form>{sharedArk ? <div className="border p-4 text-sm text-muted-foreground">Shared ARK native configuration is common to every shared instance. <Link className="text-foreground underline underline-offset-4" to="/games/ark-asa/config">Open shared configuration</Link>.</div> : <AdvancedConfigSection id={id} game={game} running={running} />}{game === '7d2d' && <SevenDaysTemplateReview id={id} running={running} />}</div>
}

function TabPanel({ active, className, children }: { active: boolean; className?: string; children: ReactNode }) {
  return active ? <div className={className}>{children}</div> : null
}

export function ManagedInstanceDetailPage() {
  const { id, game, name, '*': tabPath } = useParams<{ id: string; game: string; name: string; '*': string }>()
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
  if (!instance.data) return <Skeleton className="h-40" />
  const detail = instance.data
  // Keep bookmarks to the historical game/name URL working, but immediately
  // replace them with the immutable UUID route so a later rename is safe.
  if (!id) {
    return <Navigate replace to={`/instance/${detail.id}${tabPath ? `/${tabPath}` : ''}`} />
  }
  const rustConfig = detail.game === 'rust' ? asRustConfig(detail.config) : null
  const genericConfig = detail.game === 'ark-asa' || detail.game === 'palworld' || detail.game === '7d2d' || detail.game === 'cs2' ? asGenericConfig(detail.config) : null
  const tabs = [
    { id: 'logs', label: 'Logs' },
    ...(detail.game === 'rust' ? [{ id: 'rcon', label: 'RCON' }] : []),
    ...(detail.game === '7d2d' ? [{ id: 'console', label: 'Console' }, { id: 'players', label: 'Players' }] : []),
    { id: 'errors', label: 'Errors' },
    { id: 'config', label: 'Config' },
    ...((detail.game === 'valheim' || detail.game === '7d2d') && detail.capabilities.mods ? [{ id: 'mods', label: 'Mods' }] : []),
    ...(detail.game === 'palworld' ? [{ id: 'admin', label: 'Admin' }] : []),
    ...((detail.game === 'valheim' || detail.game === 'rust') && detail.capabilities.access_lists ? [{ id: 'lists', label: 'Access lists' }] : []),
    ...(detail.capabilities.backups ? [{ id: 'backups', label: 'Backups' }] : []),
    ...(detail.game === 'cs2' ? [] : [{ id: 'saves', label: 'Save files' }]),
    { id: 'resources', label: 'Resources' },
    { id: 'limits', label: 'Limits' },
    ...(detail.game === 'valheim' && detail.capabilities.players ? [{ id: 'players', label: 'Players' }] : []),
  ]
  const [tab, ...nestedPath] = tabPath?.split('/').filter(Boolean) ?? []
  const listsBasePath = `/instance/${id}/lists`
  const defaultListTab = detail.game === 'valheim' ? 'admin' : 'owner'
  if (!tab || (tab !== 'lists' && tab !== 'mods' && nestedPath.length > 0) || !tabs.some((candidate) => candidate.id === tab)) {
    return <Navigate replace to={`/instance/${id}/logs`} />
  }

  if (tab === 'lists' && nestedPath.length === 0) return <Navigate replace to={`${listsBasePath}/${defaultListTab}`} />

  return (
    <div className="relative isolate flex flex-col gap-6">
      <GameBackdrop game={detail.game} />
      <ManagedInstanceHeader instance={detail} />
      <RuntimeHealthCard id={detail.id} game={detail.game} running={detail.running} restartRequired={detail.restart_required} />
      <RouteTabs label="Instance sections" tabs={tabs.map((item) => ({ label: item.label, to: `/instance/${id}/${item.id}` }))} />
      <div className="flex flex-col gap-2">
        <TabPanel active={tab === 'logs'}><ManagedLogsTab id={detail.id} /></TabPanel>
        {detail.game === 'rust' && <TabPanel active={tab === 'rcon'}><RustRconTab id={detail.id} running={detail.running} /></TabPanel>}
        {detail.game === '7d2d' && <TabPanel active={tab === 'console'}><SevenDaysConsoleTab id={detail.id} running={detail.running} /></TabPanel>}
        {detail.game === '7d2d' && <TabPanel active={tab === 'players'}><SevenDaysPlayersTab id={detail.id} running={detail.running} /></TabPanel>}
        <TabPanel active={tab === 'errors'}>
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
        </TabPanel>
        <TabPanel active={tab === 'config'} className="flex flex-col gap-6">
          {detail.game === 'valheim'
            ? <ConfigTab id={detail.id} />
            : rustConfig
            ? <div className="flex flex-col gap-6"><RustConfigForm key={`${detail.id}-${JSON.stringify(detail.config)}`} id={detail.id} config={rustConfig} running={detail.running} /><AdvancedConfigSection id={detail.id} game={detail.game} running={detail.running} /></div>
            : genericConfig && (detail.game === 'ark-asa' || detail.game === 'palworld' || detail.game === '7d2d' || detail.game === 'cs2')
              ? <GenericConfigForm key={`${detail.id}-${JSON.stringify(detail.config)}`} id={detail.id} game={detail.game} config={genericConfig} running={detail.running} installMode={detail.install_mode} />
              : <p className="text-sm text-muted-foreground">No editable configuration is available for this game.</p>}
          {detail.install_mode === 'isolated' && detail.capabilities.shared_install_migration && <SharedInstallMigrationCard instance={detail} />}
          <UptimeScheduleCard id={detail.id} />
          {detail.game === 'rust' && <WipeMapCard id={detail.id} name={detail.name} running={detail.running} />}
        </TabPanel>
        {detail.game === 'palworld' && <TabPanel active={tab === 'admin'}><PalworldAdminTab id={detail.id} running={detail.running} /></TabPanel>}
        {detail.capabilities.backups && (
          <TabPanel active={tab === 'backups'}><BackupsTab id={detail.id} name={detail.name} game={detail.game} running={detail.running} /></TabPanel>
        )}
        {detail.game === 'rust' && tab === 'lists' && (
          <TabPanel active={tab === 'lists'}><RustAccessListsTab id={detail.id} path={nestedPath} running={detail.running} basePath={listsBasePath} /></TabPanel>
        )}
        {detail.game === 'valheim' && tab === 'lists' && (
          <TabPanel active={tab === 'lists'}><AccessListsTab id={detail.id} path={nestedPath} /></TabPanel>
        )}
        {detail.game === 'valheim' && tab === 'mods' && (
          <TabPanel active={tab === 'mods'}><ModsTab id={detail.id} name={detail.name} running={detail.running} path={nestedPath} /></TabPanel>
        )}
        {detail.game === '7d2d' && tab === 'mods' && <TabPanel active={tab === 'mods'}><SevenDaysModsTab id={detail.id} running={detail.running} /></TabPanel>}
        {detail.game === 'ark-asa' && detail.install_mode === 'shared'
          ? <TabPanel active={tab === 'saves'}><Card><CardHeader><CardTitle>Shared ARK save files</CardTitle><CardDescription>Browse this instance’s world files and cluster transfer files from the ARK game page.</CardDescription></CardHeader><CardContent><Link className={buttonVariants({ variant: 'outline' })} to="/games/ark-asa/saves">Open ARK Save files</Link></CardContent></Card></TabPanel>
          : <TabPanel active={tab === 'saves'}><SaveFilesTab id={detail.id} running={detail.running} /></TabPanel>}
        <TabPanel active={tab === 'resources'}><ManagedResourcesTab id={detail.id} running={detail.running} /></TabPanel>
        <TabPanel active={tab === 'limits'}><ResourceLimitsTab id={detail.id} /></TabPanel>
        {detail.game === 'valheim' && <TabPanel active={tab === 'players'}><PlayersTab id={detail.id} running={detail.running} /></TabPanel>}
      </div>
    </div>
  )
}
