import { useState } from 'react'
import { toast } from 'sonner'
import { QueryError } from '@/components/QueryError'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { useConfig, useUpdateConfig } from '@/lib/queries'
import type { ConfigView, ValheimModifiers, ValheimSetKey } from '@/lib/types'

const MIN_PORT = 1
const MAX_PORT = 65535

export function ConfigTab({ id }: { id: string }) {
  const config = useConfig(id)
  const updateConfig = useUpdateConfig(id)

  if (config.isError) {
    return <QueryError error={config.error} />
  }

  if (config.isLoading || !config.data) {
    return (
      <div className="flex max-w-md flex-col gap-4">
        <Skeleton className="h-14 w-full" />
        <Skeleton className="h-14 w-full" />
        <Skeleton className="h-16 w-full" />
        <Skeleton className="h-12 w-full" />
      </div>
    )
  }

  // Keyed by instance name so switching instances mounts a fresh form
  // seeded from the newly loaded config, instead of syncing state in an
  // effect every time `config.data` changes (e.g. on refetch).
  return (
    <div className="flex max-w-md flex-col gap-6">
      <ConfigForm key={id} initial={config.data} updateConfig={updateConfig} />
    </div>
  )
}

function ConfigForm({
  initial,
  updateConfig,
}: {
  initial: ConfigView
  updateConfig: ReturnType<typeof useUpdateConfig>
}) {
  const [world, setWorld] = useState(initial.world_name)
  const [port, setPort] = useState(String(initial.port))
  const [password, setPassword] = useState(initial.password ?? '')
  const [isPublic, setIsPublic] = useState(initial.public)
  const [autoRestart, setAutoRestart] = useState(initial.auto_restart)
  const [saveInterval, setSaveInterval] = useState(initial.save_interval?.toString() ?? '')
  const [backups, setBackups] = useState(initial.backups?.toString() ?? '')
  const [backupShort, setBackupShort] = useState(initial.backup_short?.toString() ?? '')
  const [backupLong, setBackupLong] = useState(initial.backup_long?.toString() ?? '')
  const [crossplay, setCrossplay] = useState(initial.crossplay)
  const [instanceId, setInstanceId] = useState(initial.playfab_instance_id ?? '')
  const [preset, setPreset] = useState(initial.preset ?? '')
  const [modifiers, setModifiers] = useState<Record<keyof ValheimModifiers, string>>({
    combat: initial.modifiers.combat ?? '',
    death_penalty: initial.modifiers.death_penalty ?? '',
    resources: initial.modifiers.resources ?? '',
    raids: initial.modifiers.raids ?? '',
    portals: initial.modifiers.portals ?? '',
  })
  const [setKeys, setSetKeys] = useState<ValheimSetKey[]>(initial.set_keys)

  const portNumber = Number(port)
  const portInvalid = port.trim() === '' || Number.isNaN(portNumber) || portNumber < MIN_PORT || portNumber > MAX_PORT
  const optionalNumbersInvalid = [saveInterval, backups, backupShort, backupLong].some((value) => value !== '' && (!Number.isInteger(Number(value)) || Number(value) <= 0))
  const optionalNumber = (value: string) => value === '' ? null : Number(value)

  const handleSave = () => {
    if (portInvalid || optionalNumbersInvalid) return
    updateConfig.mutate(
      {
        world, port: portNumber, password, public: isPublic, auto_restart: autoRestart,
        save_interval: optionalNumber(saveInterval), backups: optionalNumber(backups),
        backup_short: optionalNumber(backupShort), backup_long: optionalNumber(backupLong),
        crossplay, playfab_instance_id: instanceId.trim() || null,
        preset: preset === '' ? null : preset as ConfigView['preset'],
        modifiers: {
          combat: modifiers.combat === '' ? null : modifiers.combat as ValheimModifiers['combat'],
          death_penalty: modifiers.death_penalty === '' ? null : modifiers.death_penalty as ValheimModifiers['death_penalty'],
          resources: modifiers.resources === '' ? null : modifiers.resources as ValheimModifiers['resources'],
          raids: modifiers.raids === '' ? null : modifiers.raids as ValheimModifiers['raids'],
          portals: modifiers.portals === '' ? null : modifiers.portals as ValheimModifiers['portals'],
        },
        set_keys: setKeys,
      },
      {
        onSuccess: () => toast.success('Config updated — restart the instance to apply it'),
        onError: (e) => toast.error(e.message),
      },
    )
  }

  return (
    <div className="flex max-w-md flex-col gap-4">
      <div className="flex flex-col gap-2">
        <Label htmlFor="world">World name</Label>
        <Input id="world" value={world} onChange={(e) => setWorld(e.target.value)} />
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor="port">Port</Label>
        <Input id="port" type="number" value={port} onChange={(e) => setPort(e.target.value)} />
        {portInvalid && (
          <p className="text-xs text-destructive">Enter a valid port number ({MIN_PORT}-{MAX_PORT}).</p>
        )}
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor="password">Password</Label>
        <Input id="password" value={password} onChange={(e) => setPassword(e.target.value)} />
        <p className="text-xs text-muted-foreground">At least 5 characters (Valheim's minimum).</p>
      </div>
      <div className="flex items-center justify-between rounded-xl border p-3">
        <Label htmlFor="public">Public</Label>
        <Switch id="public" checked={isPublic} onCheckedChange={setIsPublic} />
      </div>
      <div className="flex items-center justify-between rounded-xl border p-3">
        <div>
          <Label htmlFor="crossplay">Crossplay</Label>
          <p className="text-xs text-muted-foreground">Use PlayFab so players from every supported platform can join.</p>
        </div>
        <Switch id="crossplay" checked={crossplay} onCheckedChange={setCrossplay} />
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        <OptionalNumber id="save-interval" label="Save interval (seconds)" value={saveInterval} onChange={setSaveInterval} />
        <OptionalNumber id="backups" label="Internal backups" value={backups} onChange={setBackups} />
        <OptionalNumber id="backup-short" label="Short backup interval" value={backupShort} onChange={setBackupShort} />
        <OptionalNumber id="backup-long" label="Long backup interval" value={backupLong} onChange={setBackupLong} />
      </div>
      {optionalNumbersInvalid && <p className="text-xs text-destructive">Optional numeric values must be whole numbers greater than zero.</p>}
      <div className="flex flex-col gap-2">
        <Label htmlFor="instance-id">PlayFab instance ID</Label>
        <Input id="instance-id" value={instanceId} onChange={(event) => setInstanceId(event.target.value)} />
      </div>
      <ConfigSelect id="preset" label="World preset" value={preset} options={['normal', 'casual', 'easy', 'hard', 'hardcore', 'immersive', 'hammer']} onChange={setPreset} />
      <div className="grid gap-4 sm:grid-cols-2">
        <ConfigSelect id="combat" label="Combat" value={modifiers.combat} options={['veryeasy', 'easy', 'hard', 'veryhard']} onChange={(value) => setModifiers((current) => ({ ...current, combat: value }))} />
        <ConfigSelect id="death-penalty" label="Death penalty" value={modifiers.death_penalty} options={['casual', 'veryeasy', 'easy', 'hard', 'hardcore']} onChange={(value) => setModifiers((current) => ({ ...current, death_penalty: value }))} />
        <ConfigSelect id="resources" label="Resources" value={modifiers.resources} options={['muchless', 'less', 'more', 'muchmore', 'most']} onChange={(value) => setModifiers((current) => ({ ...current, resources: value }))} />
        <ConfigSelect id="raids" label="Raids" value={modifiers.raids} options={['none', 'muchless', 'less', 'more', 'muchmore']} onChange={(value) => setModifiers((current) => ({ ...current, raids: value }))} />
        <ConfigSelect id="portals" label="Portals" value={modifiers.portals} options={['casual', 'hard', 'veryhard']} onChange={(value) => setModifiers((current) => ({ ...current, portals: value }))} />
      </div>
      <div className="flex flex-col gap-2">
        <Label>World keys</Label>
        <div className="grid gap-2 sm:grid-cols-2">
          {(['nobuildcost', 'playerevents', 'passivemobs', 'nomap'] satisfies ValheimSetKey[]).map((key) => <label key={key} className="flex items-center gap-2 text-sm"><input type="checkbox" checked={setKeys.includes(key)} onChange={(event) => setSetKeys((current) => event.target.checked ? [...current, key] : current.filter((value) => value !== key))} />{key}</label>)}
        </div>
      </div>
      <div className="flex items-center justify-between rounded-xl border p-3">
        <div>
          <Label htmlFor="auto-restart">Restart automatically</Label>
          <p className="text-xs text-muted-foreground">
            If the server crashes, start it again without waiting for you to notice.
          </p>
        </div>
        <Switch id="auto-restart" checked={autoRestart} onCheckedChange={setAutoRestart} />
      </div>

      <Button className="w-fit" onClick={handleSave} disabled={updateConfig.isPending || portInvalid || optionalNumbersInvalid}>
        Save
      </Button>
      <p className="text-xs text-muted-foreground">
        Changes take effect the next time the instance is restarted.
      </p>
    </div>
  )
}

function OptionalNumber({ id, label, value, onChange }: { id: string; label: string; value: string; onChange: (value: string) => void }) {
  return <div className="flex flex-col gap-2"><Label htmlFor={id}>{label}</Label><Input id={id} type="number" min={1} placeholder="Game default" value={value} onChange={(event) => onChange(event.target.value)} /></div>
}

function ConfigSelect({ id, label, value, options, onChange }: { id: string; label: string; value: string; options: string[]; onChange: (value: string) => void }) {
  return <div className="flex flex-col gap-2"><Label htmlFor={id}>{label}</Label><select id={id} className="h-9 rounded-md border bg-transparent px-3 text-sm" value={value} onChange={(event) => onChange(event.target.value)}><option value="">Game default</option>{options.map((option) => <option key={option} value={option}>{option}</option>)}</select></div>
}
