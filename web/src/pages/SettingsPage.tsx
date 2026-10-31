import { useState } from 'react'
import { toast } from 'sonner'
import { AccountSettings } from '@/components/AccountSettings'
import { PageHeader } from '@/components/PageHeader'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import {
  useClearNexusApiKey,
  useCheckRuntimeImages,
  useIsAdmin,
  usePullRuntimeImage,
  useRuntimeImages,
  useSetInstanceDefaults,
  useSetNexusApiKey,
  useSettings,
} from '@/lib/queries'
import type { InstanceDefaults, PodmanSettingsView, RuntimeImageView } from '@/lib/types'
import { gameLabel } from '@/lib/games'

function RuntimeImages() {
  const images = useRuntimeImages()
  const pull = usePullRuntimeImage()
  const check = useCheckRuntimeImages()
  if (images.isLoading) return <Skeleton className="h-36 w-full max-w-2xl" />
  if (images.isError) return <QueryError error={images.error} />
  if (!images.data?.length) return null
  return <section className="max-w-2xl border"><div className="flex items-start justify-between gap-3 border-b px-4 py-3"><div><h2 className="text-sm font-medium">Runtime images</h2><p className="mt-1 text-xs text-muted-foreground">Images are pulled only when requested. Stopped runtimes are recreated automatically after a pull; running servers are never interrupted. Default tags are checked for updates every six hours.</p></div><Button className="shrink-0" variant="outline" disabled={check.isPending} onClick={() => check.mutate(undefined, { onError: (error) => toast.error(error.message) })}>Check for updates</Button></div><div className="divide-y">{images.data.map((image) => <RuntimeImageRow key={image.role} image={image} pulling={pull.isPending} onPull={() => pull.mutate(image.role, { onSuccess: () => toast.success(`${image.role} image pull started`), onError: (error) => toast.error(error.message) })} />)}</div></section>
}

function RuntimeImageRow({ image, pulling, onPull }: { image: RuntimeImageView; pulling: boolean; onPull: () => void }) {
  return <div className="flex flex-col gap-3 p-4 sm:flex-row sm:items-center sm:justify-between"><div className="min-w-0"><div className="flex items-center gap-2"><p className="font-medium">{image.role}</p><Badge variant={image.local_id ? 'secondary' : 'outline'}>{image.local_id ? 'available locally' : 'not pulled'}</Badge>{image.pinned && <Badge variant="outline">pinned</Badge>}{image.update_available && <Badge>update available</Badge>}</div><p className="mt-1 truncate font-mono text-xs text-muted-foreground">{image.reference}</p><p className="mt-1 text-xs text-muted-foreground">Used by {image.games.map(gameLabel).join(', ')}{image.local_digest ? ` · ${image.local_digest}` : ''}</p>{image.last_checked && <p className="mt-1 text-xs text-muted-foreground">Last checked {new Date(image.last_checked * 1000).toLocaleString()}</p>}</div><Button className="w-fit" variant="outline" disabled={pulling} onClick={onPull}>Pull image</Button></div>
}

function InstanceDefaultsForm({ defaults }: { defaults: InstanceDefaults }) {
  const saveDefaults = useSetInstanceDefaults()
  const [autoRestart, setAutoRestart] = useState(defaults.auto_restart)
  const [backupEnabled, setBackupEnabled] = useState(defaults.backup_enabled)
  const [intervalHours, setIntervalHours] = useState(String(defaults.backup_interval_hours))
  const [retainCount, setRetainCount] = useState(String(defaults.backup_retain_count))
  const interval = Number(intervalHours)
  const retain = Number(retainCount)
  const invalid = !Number.isInteger(interval) || interval < 1 || !Number.isInteger(retain) || retain < 1

  return (
    <form
      className="flex max-w-md flex-col gap-4 rounded-xl border p-4"
      onSubmit={(event) => {
        event.preventDefault()
        if (invalid) return
        saveDefaults.mutate(
          {
            auto_restart: autoRestart,
            backup_enabled: backupEnabled,
            backup_interval_hours: interval,
            backup_retain_count: retain,
          },
          {
            onSuccess: () => toast.success('Instance defaults saved'),
            onError: (error) => toast.error(error.message),
          },
        )
      }}
    >
      <div>
        <h2 className="text-sm font-medium">New instance defaults</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          Applied when a Valheim or Rust server is created. Existing servers keep their settings.
        </p>
      </div>
      <div className="flex items-center justify-between gap-4">
        <div>
          <Label htmlFor="default-auto-restart">Automatic restart</Label>
          <p className="text-xs text-muted-foreground">Restart after an unexpected server exit.</p>
        </div>
        <Switch id="default-auto-restart" checked={autoRestart} onCheckedChange={setAutoRestart} />
      </div>
      <div className="flex items-center justify-between gap-4">
        <div>
          <Label htmlFor="default-backups">Automatic backups</Label>
          <p className="text-xs text-muted-foreground">Create a schedule for every new server.</p>
        </div>
        <Switch id="default-backups" checked={backupEnabled} onCheckedChange={setBackupEnabled} />
      </div>
      <div className="grid grid-cols-2 gap-3">
        <div className="flex flex-col gap-2">
          <Label htmlFor="default-backup-interval">Every (hours)</Label>
          <Input id="default-backup-interval" type="number" min={1} step={1} value={intervalHours} onChange={(event) => setIntervalHours(event.target.value)} />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="default-backup-retain">Keep last</Label>
          <Input id="default-backup-retain" type="number" min={1} step={1} value={retainCount} onChange={(event) => setRetainCount(event.target.value)} />
        </div>
      </div>
      <Button className="w-fit" type="submit" disabled={invalid || saveDefaults.isPending}>
        Save defaults
      </Button>
    </form>
  )
}

function PodmanSettings({ podman, configFile }: { podman: PodmanSettingsView; configFile: string }) {
  return (
    <section className="flex max-w-md flex-col gap-4 rounded-xl border p-4">
      <div>
        <h2 className="text-sm font-medium">Podman</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          Podman is configured outside the dashboard. Edit the <code>{configFile}</code> file, then restart Odin.
        </p>
      </div>
      <p className="text-xs text-muted-foreground">
        Required for Valheim. The system service normally uses the rootful socket at /run/podman/podman.sock.
      </p>
      <dl className="grid gap-2 text-sm">
        <div>
          <dt className="text-muted-foreground">Unix socket</dt>
          <dd>{podman.socket ?? 'Not configured'}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Runtime image</dt>
          <dd>{podman.runtime_image ?? 'Default image'}</dd>
        </div>
      </dl>
    </section>
  )
}

export function SettingsPage() {
  const isAdmin = useIsAdmin()
  const settings = useSettings()
  const setApiKey = useSetNexusApiKey()
  const clearApiKey = useClearNexusApiKey()
  const [apiKey, setApiKeyInput] = useState('')

  const handleSave = () => {
    if (!apiKey.trim()) return
    setApiKey.mutate(apiKey.trim(), {
      onSuccess: () => {
        setApiKeyInput('')
        toast.success('Nexus Mods API key saved')
      },
      onError: (e) => toast.error(e.message),
    })
  }

  const handleClear = () => {
    clearApiKey.mutate(undefined, {
      onSuccess: () => toast.success('Nexus Mods API key cleared'),
      onError: (e) => toast.error(e.message),
    })
  }

  return (
    <div className="flex flex-col gap-8">
      <PageHeader title="Settings" description="Global configuration shared across all instances." />

      <AccountSettings />

      <fieldset disabled={!isAdmin} className="flex min-w-0 flex-col gap-8 border-0 p-0">
      {settings.isError && <QueryError error={settings.error} />}

      {settings.isLoading ? (
        <Skeleton className="h-72 w-full max-w-md" />
      ) : settings.data ? (
        <>
          <PodmanSettings podman={settings.data.podman} configFile={settings.data.config_file} />
          <RuntimeImages />
          <InstanceDefaultsForm
            key={JSON.stringify(settings.data.instance_defaults)}
            defaults={settings.data.instance_defaults}
          />
        </>
      ) : null}

      <div className="flex max-w-md flex-col gap-4 rounded-xl border p-4">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-medium">Nexus Mods API key</h2>
          {settings.isLoading ? (
            <Skeleton className="h-5 w-20" />
          ) : (
            <Badge variant={settings.data?.nexus_api_key_configured ? 'default' : 'outline'}>
              {settings.data?.nexus_api_key_configured ? 'configured' : 'not configured'}
            </Badge>
          )}
        </div>
        <p className="text-xs text-muted-foreground">
          Used to look up and install mods from Nexus Mods. Get a personal API key from your{' '}
          <a
            href="https://www.nexusmods.com/users/myaccount?tab=api"
            target="_blank"
            rel="noreferrer"
            className="underline"
          >
            Nexus Mods account settings
          </a>
          .
        </p>

        <div className="flex flex-col gap-2">
          <Label htmlFor="nexus-api-key">API key</Label>
          <Input
            id="nexus-api-key"
            type="password"
            placeholder="•••••••••••••••••"
            value={apiKey}
            onChange={(e) => setApiKeyInput(e.target.value)}
          />
        </div>

        <div className="flex gap-2">
          <Button
            className="w-fit"
            disabled={!apiKey.trim() || setApiKey.isPending}
            onClick={handleSave}
          >
            Save
          </Button>
          {settings.data?.nexus_api_key_configured && (
            <Button
              variant="outline"
              className="w-fit"
              disabled={clearApiKey.isPending}
              onClick={handleClear}
            >
              Clear
            </Button>
          )}
        </div>
      </div>
      </fieldset>
    </div>
  )
}
