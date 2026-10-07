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
  useAdoptSevenDaysTemplateBaseline,
  useApplySevenDaysTemplateReview,
  useInitializeAdvancedConfig,
  useSevenDaysTemplateReview,
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
  autoRestart: boolean
}

type GenericConfig = GenericConfigUpdateRequest

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
  return {
    port,
    query_port: queryPort,
    admin_port: adminPort,
    auto_restart: config.auto_restart,
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
  const [nextNewEntryId, setNextNewEntryId] = useState(0)
  const [search, setSearch] = useState('')
  useEffect(() => {
    const initial: Record<string, string> = {}
    for (const file of config.data?.files ?? []) for (const section of file.sections) for (const entry of section.entries) initial[`${file.id}:${entry.key}`] = entry.value
    setValues(initial)
    setNewEntries([])
  }, [config.data])
  if (config.isLoading) return null
  if (config.isError) return <QueryError error={config.error} />
  const files = config.data?.files ?? []
  const missingFiles = files.filter((file) => !file.exists)
  const canInitialize = game === '7d2d' || game === 'vrising' || game === 'palworld' || game === 'runescape-dragonwilds'
  const normalizedSearch = search.trim().toLocaleLowerCase()
  const generatedFiles = files.filter((file) => file.exists)
  const visibleFiles = generatedFiles.map((file) => ({
    ...file,
    sections: file.sections.map((section) => ({ ...section, entries: section.entries.filter((entry) => entry.label.toLocaleLowerCase().includes(normalizedSearch)) })).filter((section) => section.entries.length > 0),
  })).filter((file) => file.sections.length > 0 || file.allows_new_keys)
  const save = () => {
    const changes: AdvancedConfigChange[] = []
    for (const file of generatedFiles) for (const section of file.sections) for (const entry of section.entries) {
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
  return <div className="flex flex-col gap-4"><div><p className="text-sm text-muted-foreground">Settings come directly from native server files. Stop the server before saving.</p>{missingFiles.length > 0 && canInitialize ? <div className="mt-2 flex flex-wrap items-center gap-3"><p className="text-sm text-muted-foreground">Initialize missing files from the installed server template.</p><Button type="button" variant="outline" disabled={running || initialize.isPending} onClick={() => initialize.mutate(undefined, { onSuccess: () => toast.success('Configuration files initialized'), onError: (error) => toast.error(error.message) })}>Initialize config files</Button></div> : missingFiles.length > 0 && <p className="mt-2 text-sm text-muted-foreground">Missing: {missingFiles.map((file) => file.path).join(', ')}.</p>}</div>{generatedFiles.length > 0 && <Input aria-label="Search configuration keys" placeholder="Search configuration keys…" value={search} onChange={(event) => setSearch(event.target.value)} />}{visibleFiles.map((file) => <div key={file.id} className="rounded-xl border"><div className="border-b px-4 py-3"><p className="text-sm font-medium">{file.path}</p><p className="text-xs text-muted-foreground">{file.format}</p></div><div className="divide-y">{file.sections.map((section) => <details key={section.id} open={normalizedSearch.length > 0} className="group"><summary className="cursor-pointer px-4 py-3 text-sm font-medium">{section.label}</summary><div className="grid gap-3 border-t px-4 py-4 sm:grid-cols-2">{section.entries.map((entry) => <div key={entry.key} className="flex flex-col gap-1"><Label htmlFor={`advanced-${file.id}-${entry.key}`}>{entry.label}</Label><Input id={`advanced-${file.id}-${entry.key}`} type={entry.sensitive ? 'password' : 'text'} placeholder={entry.sensitive && entry.configured ? '••••••••' : entry.managed ? 'Managed by Odin' : undefined} value={values[`${file.id}:${entry.key}`] ?? entry.value} disabled={running || entry.managed} onChange={(event) => setValues((current) => ({ ...current, [`${file.id}:${entry.key}`]: event.target.value }))} /></div>)}</div></details>)}</div>{file.allows_new_keys && <div className="flex flex-col gap-3 border-t px-4 py-4"><div><p className="text-sm font-medium">Add configuration key</p><p className="text-xs text-muted-foreground">Use the Rust convar name and its value exactly as the server expects.</p></div>{newEntries.filter((entry) => entry.file === file.id).map((entry) => <div key={entry.id} className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]"><Input aria-label="New configuration key" placeholder="server.example" value={entry.key} disabled={running} onChange={(event) => updateNewEntry(entry.id, 'key', event.target.value)} /><Input aria-label="New configuration value" placeholder="Value" value={entry.value} disabled={running} onChange={(event) => updateNewEntry(entry.id, 'value', event.target.value)} /><Button type="button" variant="outline" disabled={running} onClick={() => setNewEntries((current) => current.filter((currentEntry) => currentEntry.id !== entry.id))}>Remove</Button></div>)}<Button className="w-fit" type="button" variant="outline" disabled={running} onClick={() => addEntry(file.id)}>Add key</Button></div>}</div>)}{generatedFiles.length > 0 && visibleFiles.length === 0 && <p className="text-sm text-muted-foreground">No configuration keys match “{search}”.</p>}{generatedFiles.length > 0 && <Button className="w-fit" type="button" disabled={running || update.isPending} onClick={save}>Save configuration</Button>}</div>
}

function GenericConfigForm({ id, game, config, running }: { id: string; game: Extract<GameId, 'vrising' | 'palworld' | 'runescape-dragonwilds' | '7d2d'>; config: GenericConfig; running: boolean }) {
  const update = useUpdateGenericConfig()
  const [port, setPort] = useState(String(config.port))
  const [queryPort, setQueryPort] = useState(config.query_port === null ? '' : String(config.query_port))
  const [adminPort, setAdminPort] = useState(config.admin_port === null ? '' : String(config.admin_port))
  const [autoRestart, setAutoRestart] = useState(config.auto_restart)
  const save = () => update.mutate({ id, request: { port: Number(port), query_port: queryPort ? Number(queryPort) : null, admin_port: adminPort ? Number(adminPort) : null, auto_restart: autoRestart } }, { onSuccess: () => toast.success('Odin runtime settings saved'), onError: (error) => toast.error(error.message) })
  return <div className="flex flex-col gap-6"><form className="flex flex-col gap-4" onSubmit={(event) => { event.preventDefault(); save() }}><p className="text-sm text-muted-foreground">Odin manages process ports and restart behavior. Game settings are below and come directly from the server-generated file.</p><div className="grid gap-4 sm:grid-cols-2"><ConfigInput id="generic-port" label="Game port" type="number" min={1} max={65535} value={port} disabled={running} onChange={setPort} />{config.query_port !== null && <ConfigInput id="generic-query-port" label={game === 'palworld' ? 'Steam query port' : game === 'runescape-dragonwilds' ? 'Beacon port' : 'Query port'} type="number" min={1} max={65535} value={queryPort} disabled={running} onChange={setQueryPort} />}{config.admin_port !== null && <ConfigInput id="generic-admin-port" label={game === 'palworld' ? 'REST API port' : game === '7d2d' ? 'Local console port' : 'RCON port'} type="number" min={1} max={65535} value={adminPort} disabled={running} onChange={setAdminPort} />}</div><div className="flex items-center justify-between rounded-xl border p-3"><div><Label htmlFor="generic-auto-restart">Restart automatically</Label><p className="text-xs text-muted-foreground">Restart this server after an unexpected exit.</p></div><Switch id="generic-auto-restart" checked={autoRestart} disabled={running} onCheckedChange={setAutoRestart} /></div><Button className="w-fit" type="submit" disabled={running || update.isPending}>Save Odin settings</Button></form><AdvancedConfigSection id={id} game={game} running={running} />{game === '7d2d' && <SevenDaysTemplateReview id={id} running={running} />}</div>
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
            ? <div className="flex flex-col gap-6"><RustConfigForm key={`${detail.id}-${JSON.stringify(detail.config)}`} id={detail.id} config={rustConfig} running={detail.running} /><AdvancedConfigSection id={detail.id} game={detail.game} running={detail.running} /></div>
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
