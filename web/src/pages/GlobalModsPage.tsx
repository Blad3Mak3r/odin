import { Loader2 } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { Navigate, useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { ModIcon } from '@/components/ModIcon'
import { ModSearch } from '@/components/ModSearch'
import { NexusModSearch } from '@/components/NexusModSearch'
import { JobProgress } from '@/components/JobProgress'
import { QueryError } from '@/components/QueryError'
import { UploadModForm } from '@/components/UploadModForm'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { useJobSocket } from '@/hooks/useJobSocket'
import { getModSource, MOD_SOURCE_LABEL } from '@/lib/modSource'
import {
  useAddMod,
  useBepInExStatus,
  useGlobalMods,
  useInstallBepInEx,
  useManagedInstances,
  usePruneMod,
  usePruneModVersion,
  useRemoveMod,
  useSetModEnabled,
  useSetModPinned,
  useSelectModVersion,
  useUpdateMods,
  useUpdateBepInEx,
} from '@/lib/queries'
import type { GlobalMod, ManagedInstanceView } from '@/lib/types'

const MOD_TABS = ['installed', 'marketplace'] as const
const MOD_SOURCES = ['thunderstore', 'nexus', 'upload'] as const
type ModTab = (typeof MOD_TABS)[number]
type ModSource = (typeof MOD_SOURCES)[number]

function isModTab(value: string | undefined): value is ModTab {
  return MOD_TABS.some((tab) => tab === value)
}

function isModSource(value: string | undefined): value is ModSource {
  return MOD_SOURCES.some((source) => source === value)
}

const BASE_PATH = '/games/valheim/mods'

export function ValheimModsTab({ path }: { path: string[] }) {
  const instances = useManagedInstances()
  const navigate = useNavigate()
  const valheimInstances = useMemo(
    () => instances.data?.filter((instance) => instance.game === 'valheim') ?? [],
    [instances.data],
  )
  const [tab, source, ...rest] = path
  const activeSource = isModSource(source) ? source : null

  if (!isModTab(tab) || rest.length > 0 || (tab === 'installed' && source)) {
    return <Navigate to={`${BASE_PATH}/installed`} replace />
  }
  if (tab === 'marketplace' && activeSource === null) {
    return <Navigate to={`${BASE_PATH}/marketplace/thunderstore`} replace />
  }

  return (
    <div className="flex flex-col gap-8">
      {instances.isError && <QueryError error={instances.error} />}
      <BepInExInstances instances={valheimInstances} />
      <ModUpdateInstances instances={valheimInstances} />

      <Tabs
        value={tab}
        onValueChange={(value) =>
          navigate(value === 'marketplace' ? `${BASE_PATH}/marketplace/thunderstore` : `${BASE_PATH}/installed`)
        }
      >
        <TabsList>
          <TabsTrigger value="installed">Installed</TabsTrigger>
          <TabsTrigger value="marketplace">Marketplace</TabsTrigger>
        </TabsList>
        {tab === 'installed' && (
          <TabsContent value="installed">
            <InstalledMods instances={valheimInstances} />
          </TabsContent>
        )}
        {tab === 'marketplace' && (
          <TabsContent value="marketplace">
            <ModSearchSection instances={valheimInstances} source={activeSource ?? 'thunderstore'} />
          </TabsContent>
        )}
      </Tabs>
    </div>
  )
}

function BepInExInstances({ instances }: { instances: ManagedInstanceView[] }) {
  return (
    <section className="flex flex-col gap-3">
      <div>
        <h2 className="text-sm font-medium">BepInEx</h2>
        <p className="text-sm text-muted-foreground">Install or update the mod framework for each Valheim instance.</p>
      </div>
      {instances.length === 0 ? (
        <p className="text-sm text-muted-foreground">Create a Valheim instance before installing BepInEx.</p>
      ) : (
        <div className="grid gap-3 xl:grid-cols-2">
          {instances.map((instance) => <BepInExInstanceCard key={instance.id} instance={instance} />)}
        </div>
      )}
    </section>
  )
}

function BepInExInstanceCard({ instance }: { instance: ManagedInstanceView }) {
  const status = useBepInExStatus(instance.id)
  const install = useInstallBepInEx()
  const update = useUpdateBepInEx()
  const [jobId, setJobId] = useState<string | null>(null)
  const job = useJobSocket(jobId)

  useEffect(() => {
    if (job.status?.status !== 'succeeded' && job.status?.status !== 'failed') return
    status.refetch()
  }, [job.status?.status, status.refetch])

  const bepinex = status.data
  const installed = bepinex?.installed ?? false
  const canUpdate = installed && (!bepinex?.installed_version || Boolean(bepinex?.update_available))
  const busy = install.isPending || update.isPending || job.status?.status === 'queued' || job.status?.status === 'running'
  const actionLabel = !installed ? 'Install BepInEx' : 'Update BepInEx'

  return (
    <Card>
      <CardHeader className="flex-row items-center justify-between space-y-0">
        <div>
          <p className="font-medium">{instance.name}</p>
          {status.isError ? (
            <p className="text-sm text-destructive">Could not check BepInEx status.</p>
          ) : !installed ? (
            <p className="text-sm text-muted-foreground">Not installed</p>
          ) : (
            <p className="text-sm text-muted-foreground">
              Installed: {bepinex?.installed_version ? `v${bepinex.installed_version}` : 'unknown version'}
              {bepinex?.update_available && bepinex.latest_version ? ` · Latest: v${bepinex.latest_version}` : ''}
            </p>
          )}
        </div>
        <Badge variant={instance.running ? 'default' : 'secondary'}>{instance.running ? 'Running' : 'Stopped'}</Badge>
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        {instance.running && <p className="text-xs text-muted-foreground">Stop this instance before changing BepInEx.</p>}
        <div className="flex gap-2">
          {status.isError && <Button size="sm" variant="outline" onClick={() => status.refetch()}>Retry</Button>}
          {!status.isError && !status.isLoading && (!installed || canUpdate) && (
            <Button
              size="sm"
              disabled={instance.running || busy}
              onClick={() => {
                if (installed) {
                  update.mutate(instance.id, { onSuccess: (handle) => setJobId(handle.id), onError: (error) => toast.error(error.message) })
                } else {
                  install.mutate(instance.id, { onSuccess: (handle) => setJobId(handle.id), onError: (error) => toast.error(error.message) })
                }
              }}
            >
              {busy && <Loader2 className="size-4 animate-spin" />}
              {actionLabel}
            </Button>
          )}
          {installed && !canUpdate && !status.isError && <Badge variant="outline">Up to date</Badge>}
        </div>
        {jobId && <JobProgress log={job.log} status={job.status} connected={job.connected} />}
      </CardContent>
    </Card>
  )
}

function ModUpdateInstances({ instances }: { instances: ManagedInstanceView[] }) {
  const updateMods = useUpdateMods()

  if (instances.length === 0) return null
  return (
    <section className="flex flex-col gap-3">
      <div>
        <h2 className="text-sm font-medium">Mod updates</h2>
        <p className="text-sm text-muted-foreground">Update every unpinned mod installed on an instance.</p>
      </div>
      <div className="flex flex-wrap gap-2">
        {instances.map((instance) => (
          <Button
            key={instance.id}
            size="sm"
            variant="outline"
            disabled={instance.running || updateMods.isPending}
            onClick={() => updateMods.mutate(instance.id, { onError: (error) => toast.error(error.message) })}
          >
            Update {instance.name}
          </Button>
        ))}
      </div>
    </section>
  )
}

function InstalledMods({ instances }: { instances: ManagedInstanceView[] }) {
  const globalMods = useGlobalMods()

  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
        Installed mods
      </h2>

      {globalMods.isLoading && (
        <div className="grid gap-3 xl:grid-cols-2">
          <Skeleton className="h-20 w-full" />
          <Skeleton className="h-20 w-full" />
        </div>
      )}
      {globalMods.isError && <QueryError error={globalMods.error} />}
      {globalMods.data?.length === 0 && (
        <p className="text-sm text-muted-foreground">No mods installed anywhere yet.</p>
      )}

      <div className="grid gap-3 xl:grid-cols-2">
        {globalMods.data?.map((mod) => (
          <GlobalModCard key={mod.mod_id} mod={mod} instances={instances} />
        ))}
      </div>
    </div>
  )
}

function GlobalModCard({ mod, instances }: { mod: GlobalMod; instances: ManagedInstanceView[] }) {
  const setEnabled = useSetModEnabled()
  const removeMod = useRemoveMod()
  const setPinned = useSetModPinned()
  const pruneMod = usePruneMod()
  const pruneVersion = usePruneModVersion()
  const [installOpen, setInstallOpen] = useState(false)
  const [versionTarget, setVersionTarget] = useState<ManagedInstanceView | null>(null)
  const { confirm, dialog } = useConfirmDialog()

  const installedOn = new Set(mod.instances.map((i) => i.instance))
  const candidateInstances = instances.filter((instance) => !installedOn.has(instance.name))
  const versionsInUse = new Set(mod.instances.map((entry) => entry.version))

  const handlePrune = async () => {
    const confirmed = await confirm({
      title: `Remove '${mod.mod_id}' from the store?`,
      description: `Delete the downloaded copy of '${mod.mod_id}' from the shared mod store. It isn't installed on any instance right now.`,
      confirmLabel: 'Remove from store',
    })
    if (!confirmed) return
    pruneMod.mutate(mod.mod_id, { onError: (e) => toast.error(e.message) })
  }

  const handleRemove = async (instance: ManagedInstanceView) => {
    const confirmed = await confirm({
      title: `Remove '${mod.mod_id}'?`,
      description: `Remove '${mod.mod_id}' from '${instance.name}'? You can reinstall it later.`,
      confirmLabel: 'Remove',
    })
    if (!confirmed) return
    removeMod.mutate({ name: instance.id, modId: mod.mod_id }, { onError: (e) => toast.error(e.message) })
  }

  const handlePruneVersion = async (version: string) => {
    const confirmed = await confirm({
      title: `Remove ${mod.mod_id} v${version}?`,
      description: `Delete this cached version from the shared store. Other versions are preserved.`,
      confirmLabel: 'Remove version',
    })
    if (!confirmed) return
    pruneVersion.mutate(
      { modId: mod.mod_id, version },
      { onError: (e) => toast.error(e.message) },
    )
  }

  return (
    <Card>
      {dialog}
      <CardHeader className="flex-row items-center justify-between space-y-0">
        <div className="flex items-center gap-3">
          <ModIcon src={mod.icon} />
          <div>
            <div className="flex items-center gap-2">
              <p className="text-sm font-medium">{mod.mod_id}</p>
              <Badge variant="outline">{MOD_SOURCE_LABEL[getModSource(mod.mod_id)]}</Badge>
            </div>
            <p className="text-xs text-muted-foreground">
              {mod.stored_versions.length > 0
                ? `${mod.stored_versions.length} cached version${mod.stored_versions.length === 1 ? '' : 's'}`
                : 'missing from the shared store'}
            </p>
          </div>
        </div>
        {mod.instances.length === 0 ? (
          <Button size="sm" variant="destructive" disabled={pruneMod.isPending} onClick={handlePrune}>
            Remove from store
          </Button>
        ) : (
          candidateInstances.length > 0 && (
            <Button size="sm" variant="outline" onClick={() => setInstallOpen(true)}>
              Install on more instances
            </Button>
          )
        )}
      </CardHeader>

      <CardContent>
        {mod.stored_versions.length > 0 && (
          <div className="mb-3 flex flex-wrap items-center gap-2">
            {mod.stored_versions.map((version) => (
              <div key={version} className="flex items-center gap-1">
                <Badge variant={versionsInUse.has(version) ? 'secondary' : 'outline'}>
                  v{version}
                </Badge>
                {!versionsInUse.has(version) && (
                  <Button
                    size="xs"
                    variant="ghost"
                    disabled={pruneVersion.isPending}
                    onClick={() => handlePruneVersion(version)}
                  >
                    Remove
                  </Button>
                )}
              </div>
            ))}
          </div>
        )}
        {mod.instances.length === 0 ? (
          <p className="text-xs text-muted-foreground">
            Not installed on any instance — an orphaned download.
          </p>
        ) : (
          <div className="flex flex-col gap-2">
            {mod.instances.map((entry) => {
              const instance = instances.find((candidate) => candidate.name === entry.instance)
              if (!instance) return null
              return (
              <div
                key={entry.instance}
                className="flex flex-col gap-2 rounded-xl bg-muted/30 px-3 py-2 sm:flex-row sm:items-center sm:justify-between"
              >
                <div className="flex items-center gap-2 text-sm">
                  <span className="font-medium">{entry.instance}</span>
                  <span className="text-xs text-muted-foreground">v{entry.version}</span>
                  {entry.pinned && <Badge variant="secondary">pinned</Badge>}
                  {entry.running && <Badge variant="default">running</Badge>}
                </div>
                <div className="flex items-center gap-3">
                  {mod.stored_versions.length > 1 && (
                    <Button size="sm" variant="outline" disabled={entry.running} onClick={() => setVersionTarget(instance)}>
                      Change version
                    </Button>
                  )}
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={entry.running || setPinned.isPending}
                    onClick={() => setPinned.mutate({ name: instance.id, modId: mod.mod_id, pinned: !entry.pinned }, { onError: (error) => toast.error(error.message) })}
                  >
                    {entry.pinned ? 'Allow updates' : 'Pin version'}
                  </Button>
                  <Switch
                    checked={entry.enabled}
                    disabled={entry.running || setEnabled.isPending}
                    onCheckedChange={(enabled) =>
                      setEnabled.mutate(
                        { name: instance.id, modId: mod.mod_id, enabled },
                        { onError: (e) => toast.error(e.message) },
                      )
                    }
                  />
                  <Button
                    size="sm"
                    variant="destructive"
                    disabled={entry.running}
                    onClick={() => handleRemove(instance)}
                  >
                    Remove
                  </Button>
                </div>
              </div>
              )
            })}
          </div>
        )}
      </CardContent>

      <InstallOnInstancesDialog
        open={installOpen}
        onOpenChange={setInstallOpen}
        modId={mod.mod_id}
        candidateInstances={candidateInstances}
      />
      <VersionDialog
        open={versionTarget !== null}
        onOpenChange={(open) => !open && setVersionTarget(null)}
        instance={versionTarget}
        modId={mod.mod_id}
        versions={mod.stored_versions}
      />
    </Card>
  )
}

function VersionDialog({
  open,
  onOpenChange,
  instance,
  modId,
  versions,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  instance: ManagedInstanceView | null
  modId: string
  versions: string[]
}) {
  const selectVersion = useSelectModVersion()

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader><DialogTitle>Choose version</DialogTitle></DialogHeader>
        <div className="flex flex-col gap-2">
          {versions.map((version) => (
            <Button
              key={version}
              variant="outline"
              disabled={!instance || selectVersion.isPending}
              onClick={() => instance && selectVersion.mutate(
                { name: instance.id, modId, version },
                { onSuccess: () => onOpenChange(false), onError: (error) => toast.error(error.message) },
              )}
            >
              v{version}
            </Button>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  )
}

function ModSearchSection({ instances, source }: { instances: ManagedInstanceView[]; source: ModSource }) {
  const [dialogModId, setDialogModId] = useState<string | null>(null)
  const navigate = useNavigate()

  return (
    <div className="flex flex-col gap-3">
      <Tabs value={source} onValueChange={(value) => navigate(`${BASE_PATH}/marketplace/${value}`)}>
        <TabsList variant="line">
          <TabsTrigger value="thunderstore">Thunderstore</TabsTrigger>
          <TabsTrigger value="nexus">Nexus Mods</TabsTrigger>
          <TabsTrigger value="upload">Upload</TabsTrigger>
        </TabsList>
        {source === 'thunderstore' && (
          <TabsContent value="thunderstore">
            <ModSearch onSelect={(mod) => setDialogModId(mod.mod_id)} />
          </TabsContent>
        )}
        {source === 'nexus' && (
          <TabsContent value="nexus">
            <NexusModSearch onSelect={(mod) => setDialogModId(mod.mod_id)} />
          </TabsContent>
        )}
        {source === 'upload' && (
          <TabsContent value="upload">
            <UploadSection instances={instances} />
          </TabsContent>
        )}
      </Tabs>

      <InstallOnInstancesDialog
        open={dialogModId !== null}
        onOpenChange={(open) => !open && setDialogModId(null)}
        modId={dialogModId ?? ''}
        candidateInstances={instances}
      />
    </div>
  )
}

// Unlike the per-instance Mods tab, this page has no single target instance
// in scope — pick one before showing the upload form.
function UploadSection({ instances }: { instances: ManagedInstanceView[] }) {
  const [target, setTarget] = useState('')

  if (instances.length === 0) {
    return <p className="text-sm text-muted-foreground">Create an instance first.</p>
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex max-w-md flex-col gap-2">
        <Label htmlFor="upload-target-instance">Install on</Label>
        <select
          id="upload-target-instance"
          value={target}
          onChange={(e) => setTarget(e.target.value)}
          className="h-8 w-full rounded-lg border border-input bg-transparent px-2.5 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30"
        >
          <option value="" disabled>
            Choose an instance…
          </option>
          {instances.map((instance) => (
            <option key={instance.id} value={instance.id}>
              {instance.name}
            </option>
          ))}
        </select>
      </div>
      {target && <UploadModForm name={target} instanceName={instances.find((instance) => instance.id === target)?.name} />}
    </div>
  )
}

function InstallOnInstancesDialog({
  open,
  onOpenChange,
  modId,
  candidateInstances,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  modId: string
  candidateInstances: ManagedInstanceView[]
}) {
  const [selected, setSelected] = useState<string[]>([])
  const [installing, setInstalling] = useState(false)
  const addMod = useAddMod()

  // Reset checkboxes whenever the dialog opens for a (possibly different)
  // mod, rather than carrying over a stale selection from the last time it
  // was open — this component instance is reused across mods. Comparing
  // against the previous `open` during render (instead of an effect) avoids
  // an extra commit.
  const [prevOpen, setPrevOpen] = useState(open)
  if (open !== prevOpen) {
    setPrevOpen(open)
    if (open) setSelected([])
  }

  const toggle = (id: string) =>
    setSelected((prev) => (prev.includes(id) ? prev.filter((current) => current !== id) : [...prev, id]))

  const handleInstall = async () => {
    const targets = selected
    setInstalling(true)
    await Promise.all(
      targets.map((id) => {
        const instance = candidateInstances.find((candidate) => candidate.id === id)
        return addMod
          .mutateAsync({ name: id, modId })
          .then(() => toast.success(`Installing '${modId}' on '${instance?.name ?? id}'`))
          .catch((e: Error) => toast.error(`${instance?.name ?? id}: ${e.message}`))
      }),
    )
    setInstalling(false)
    setSelected([])
    onOpenChange(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Install &lsquo;{modId}&rsquo;</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-2">
          {candidateInstances.length === 0 && (
            <p className="text-sm text-muted-foreground">
              Already installed on every instance.
            </p>
          )}
          {candidateInstances.map((instance) => (
            <label key={instance.id} className="flex items-center gap-2 text-sm">
              <Checkbox checked={selected.includes(instance.id)} onCheckedChange={() => toggle(instance.id)} />
              {instance.name}
            </label>
          ))}
        </div>
        <DialogFooter>
          <Button disabled={selected.length === 0 || installing} onClick={handleInstall}>
            {installing && <Loader2 className="size-4 animate-spin" />}
            Install
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
