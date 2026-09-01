import { Download, Loader2 } from 'lucide-react'
import { useQueryClient } from '@tanstack/react-query'
import { lazy, Suspense, useEffect, useState } from 'react'
import { toast } from 'sonner'
import { JobProgress } from '@/components/JobProgress'
import { ModIcon } from '@/components/ModIcon'
import { ModSearch } from '@/components/ModSearch'
import { NexusModSearch } from '@/components/NexusModSearch'
import { UploadModForm } from '@/components/UploadModForm'
import { Badge } from '@/components/ui/badge'
import { Button, buttonVariants } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { useJobSocket } from '@/hooks/useJobSocket'
import { getModSource, MOD_SOURCE_LABEL } from '@/lib/modSource'
import {
  useAddMod,
  useBepInExStatus,
  useMods,
  useRemoveMod,
  useSelectModVersion,
  useSetModEnabled,
  useSetModPinned,
  useUpdateMods,
  useUpdateBepInEx,
} from '@/lib/queries'
import type { InstalledMod } from '@/lib/types'
import { cn } from '@/lib/utils'

const ModConfigFiles = lazy(() =>
  import('./ModConfigFiles').then((m) => ({ default: m.ModConfigFiles })),
)

export function ModsTab({ name, running }: { name: string; running: boolean }) {
  return (
    <Tabs defaultValue="installed">
      <TabsList variant="line">
        <TabsTrigger value="installed">Installed</TabsTrigger>
        <TabsTrigger value="marketplace">Marketplace</TabsTrigger>
      </TabsList>
      <TabsContent value="installed">
        <div className="flex flex-col gap-8">
          <BepInExCard name={name} running={running} />
          <InstalledMods name={name} running={running} />
          <Suspense fallback={<Loader2 className="size-4 animate-spin text-muted-foreground" />}>
            <ModConfigFiles name={name} />
          </Suspense>
        </div>
      </TabsContent>
      <TabsContent value="marketplace">
        <ModInstallSearch name={name} running={running} />
      </TabsContent>
    </Tabs>
  )
}

function BepInExCard({ name, running }: { name: string; running: boolean }) {
  const status = useBepInExStatus(name)
  const update = useUpdateBepInEx()
  const queryClient = useQueryClient()
  const [jobId, setJobId] = useState<string | null>(null)
  const job = useJobSocket(jobId)

  useEffect(() => {
    if (job.status?.status !== 'succeeded' && job.status?.status !== 'failed') return
    queryClient.invalidateQueries({ queryKey: ['instances', name, 'bepinex-status'] })
    queryClient.invalidateQueries({ queryKey: ['instances', name] })
    queryClient.invalidateQueries({ queryKey: ['instances'] })
    queryClient.invalidateQueries({ queryKey: ['jobs'] })
    queryClient.invalidateQueries({ queryKey: ['activity-feed'] })
  }, [job.status?.status, name, queryClient])

  const active = update.isPending || job.status?.status === 'queued' || job.status?.status === 'running'
  const installed = status.data?.installed
  const unknown = installed && !status.data?.installed_version
  const canUpdate = unknown || status.data?.update_available

  return (
    <Card>
      <CardContent className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="flex items-center gap-2">
            <h2 className="font-medium">BepInEx</h2>
            {installed && !canUpdate && <Badge variant="secondary">Up to date</Badge>}
            {status.isError && <Badge variant="destructive">Check failed</Badge>}
          </div>
          {!installed && !status.isLoading && !status.isError && (
            <p className="text-sm text-muted-foreground">
              Not installed. Odin installs BepInEx automatically when you add the first mod.
            </p>
          )}
          {installed && (
            <p className="text-sm text-muted-foreground">
              Installed: {status.data?.installed_version ? `v${status.data.installed_version}` : 'unknown version'}
              {status.data?.update_available && status.data.latest_version
                ? ` · Latest: v${status.data.latest_version}`
                : ''}
            </p>
          )}
          {status.isError && (
            <p className="text-sm text-destructive">
              Could not check Thunderstore. Local version information is unchanged.
            </p>
          )}
          {running && installed && canUpdate && (
            <p className="text-xs text-muted-foreground">Stop '{name}' before updating BepInEx.</p>
          )}
        </div>
        <div className="flex gap-2">
          {status.isError && (
            <Button size="sm" variant="outline" onClick={() => status.refetch()}>
              Retry
            </Button>
          )}
          {installed && canUpdate && !status.isError && (
            <Button
              size="sm"
              disabled={running || active}
              onClick={() =>
                update.mutate(name, {
                  onSuccess: (handle) => setJobId(handle.id),
                  onError: (error) => toast.error(error.message),
                })
              }
            >
              {active && <Loader2 className="size-4 animate-spin" />}
              {unknown ? 'Install latest' : 'Update BepInEx'}
            </Button>
          )}
        </div>
      </CardContent>
      {jobId && (
        <CardContent>
          <JobProgress log={job.log} status={job.status} connected={job.connected} />
        </CardContent>
      )}
    </Card>
  )
}

function InstalledMods({ name, running }: { name: string; running: boolean }) {
  const mods = useMods(name)
  const setEnabled = useSetModEnabled()
  const removeMod = useRemoveMod()
  const updateMods = useUpdateMods()
  const setPinned = useSetModPinned()
  const [jobId, setJobId] = useState<string | null>(null)
  const [versionMod, setVersionMod] = useState<InstalledMod | null>(null)
  const job = useJobSocket(jobId)
  const { confirm, dialog } = useConfirmDialog()

  const handleRemove = async (modId: string) => {
    const confirmed = await confirm({
      title: `Remove '${modId}'?`,
      description: `Remove '${modId}' from '${name}'? You can reinstall it later from the shared store.`,
      confirmLabel: 'Remove',
    })
    if (!confirmed) return
    removeMod.mutate({ name, modId }, { onError: (e) => toast.error(e.message) })
  }

  return (
    <div className="flex flex-col gap-3">
      {dialog}
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
          Installed mods
        </h2>
        <div className="flex items-center gap-2">
          <a
            href={`/api/instances/${name}/mods/modpack`}
            download
            className={cn(
              buttonVariants({ variant: 'outline', size: 'sm' }),
              !mods.data?.some((m) => m.enabled) && 'pointer-events-none opacity-50',
            )}
          >
            <Download className="size-4" />
            Download ModPack
          </a>
          <Button
            size="sm"
            variant="outline"
            disabled={updateMods.isPending}
            onClick={() =>
              updateMods.mutate(name, {
                onSuccess: (handle) => setJobId(handle.id),
                onError: (e) => toast.error(e.message),
              })
            }
          >
            {updateMods.isPending && <Loader2 className="size-4 animate-spin" />}
            Update all
          </Button>
        </div>
      </div>

      {mods.data?.length === 0 && (
        <p className="text-sm text-muted-foreground">No mods installed yet.</p>
      )}

      {running && (
        <p className="text-xs text-muted-foreground">
          Mods can't be changed while '{name}' is running — stop it first.
        </p>
      )}

      <div className="grid gap-2 xl:grid-cols-2 2xl:grid-cols-3">
        {mods.data?.map((m) => (
          <Card key={m.mod_id} size="sm">
            <CardContent className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
              <div className="flex items-center gap-3">
                <ModIcon src={m.icon} />
                <div>
                  <div className="flex items-center gap-2">
                    <p className="text-sm font-medium">{m.mod_id}</p>
                    <Badge variant="outline">{MOD_SOURCE_LABEL[getModSource(m.mod_id)]}</Badge>
                  </div>
                  <p className="text-xs text-muted-foreground">v{m.version}</p>
                  {m.pinned && <Badge variant="secondary">pinned</Badge>}
                </div>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                {m.available_versions.length > 1 && (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={running}
                    onClick={() => setVersionMod(m)}
                  >
                    Change version
                  </Button>
                )}
                <Button
                  size="sm"
                  variant="outline"
                  disabled={setPinned.isPending}
                  onClick={() =>
                    setPinned.mutate(
                      { name, modId: m.mod_id, pinned: !m.pinned },
                      { onError: (e) => toast.error(e.message) },
                    )
                  }
                >
                  {m.pinned ? 'Allow updates' : 'Pin version'}
                </Button>
                <Switch
                  checked={m.enabled}
                  disabled={running || setEnabled.isPending}
                  onCheckedChange={(enabled) =>
                    setEnabled.mutate(
                      { name, modId: m.mod_id, enabled },
                      { onError: (e) => toast.error(e.message) },
                    )
                  }
                />
                <Button
                  size="sm"
                  variant="destructive"
                  disabled={running}
                  onClick={() => handleRemove(m.mod_id)}
                >
                  Remove
                </Button>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {jobId && <JobProgress log={job.log} status={job.status} connected={job.connected} />}
      <VersionDialog
        name={name}
        mod={versionMod}
        open={versionMod !== null}
        onOpenChange={(open) => !open && setVersionMod(null)}
      />
    </div>
  )
}

function VersionDialog({
  name,
  mod,
  open,
  onOpenChange,
}: {
  name: string
  mod: InstalledMod | null
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const selectVersion = useSelectModVersion()

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Choose version</DialogTitle>
          <DialogDescription>
            Switching versions pins this mod. Allow updates again when you want it to follow the
            latest release.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-2">
          {mod?.available_versions.map((version) => (
            <Button
              key={version}
              variant={version === mod.version ? 'secondary' : 'outline'}
              disabled={selectVersion.isPending}
              onClick={() =>
                selectVersion.mutate(
                  { name, modId: mod.mod_id, version },
                  {
                    onSuccess: () => {
                      toast.success(`Using ${mod.mod_id} v${version}`)
                      onOpenChange(false)
                    },
                    onError: (e) => toast.error(e.message),
                  },
                )
              }
            >
              v{version}
              {version === mod.version ? ' (current)' : ''}
            </Button>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  )
}

function ModInstallSearch({ name, running }: { name: string; running: boolean }) {
  const addMod = useAddMod()
  const [jobId, setJobId] = useState<string | null>(null)
  const job = useJobSocket(jobId)

  const handleSelect = (mod: { mod_id: string }) =>
    addMod.mutate(
      { name, modId: mod.mod_id },
      {
        onSuccess: (handle) => setJobId(handle.id),
        onError: (e) => toast.error(e.message),
      },
    )

  return (
    <div className="flex flex-col gap-3">
      <Tabs defaultValue="thunderstore">
        <TabsList variant="line">
          <TabsTrigger value="thunderstore">Thunderstore</TabsTrigger>
          <TabsTrigger value="nexus">Nexus Mods</TabsTrigger>
          <TabsTrigger value="upload">Upload</TabsTrigger>
        </TabsList>
        <TabsContent value="thunderstore">
          <ModSearch selectDisabled={() => running || addMod.isPending} onSelect={handleSelect} />
        </TabsContent>
        <TabsContent value="nexus">
          <NexusModSearch
            selectDisabled={() => running || addMod.isPending}
            onSelect={handleSelect}
          />
        </TabsContent>
        <TabsContent value="upload">
          <UploadModForm name={name} running={running} />
        </TabsContent>
      </Tabs>

      {jobId && <JobProgress log={job.log} status={job.status} connected={job.connected} />}
    </div>
  )
}
