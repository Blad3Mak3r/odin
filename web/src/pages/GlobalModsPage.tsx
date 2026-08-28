import { Loader2 } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { toast } from 'sonner'
import { ModIcon } from '@/components/ModIcon'
import { ModSearch } from '@/components/ModSearch'
import { NexusModSearch } from '@/components/NexusModSearch'
import { PageHeader } from '@/components/PageHeader'
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
import { getModSource, MOD_SOURCE_LABEL } from '@/lib/modSource'
import {
  useAddMod,
  useGlobalMods,
  useInstances,
  usePruneMod,
  useRemoveMod,
  useSetModEnabled,
} from '@/lib/queries'
import type { GlobalMod } from '@/lib/types'

export function GlobalModsPage() {
  const instances = useInstances()
  const instanceNames = useMemo(() => instances.data?.map((i) => i.name) ?? [], [instances.data])

  return (
    <div className="flex flex-col gap-8">
      <PageHeader
        title="Mods"
        description="Every mod across all instances, backed by the shared download store."
      />

      {instances.isError && <QueryError error={instances.error} />}

      <Tabs defaultValue="installed">
        <TabsList>
          <TabsTrigger value="installed">Installed</TabsTrigger>
          <TabsTrigger value="marketplace">Marketplace</TabsTrigger>
        </TabsList>
        <TabsContent value="installed">
          <InstalledMods instanceNames={instanceNames} />
        </TabsContent>
        <TabsContent value="marketplace">
          <ModSearchSection instanceNames={instanceNames} />
        </TabsContent>
      </Tabs>
    </div>
  )
}

function InstalledMods({ instanceNames }: { instanceNames: string[] }) {
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
          <GlobalModCard key={mod.mod_id} mod={mod} instanceNames={instanceNames} />
        ))}
      </div>
    </div>
  )
}

function GlobalModCard({ mod, instanceNames }: { mod: GlobalMod; instanceNames: string[] }) {
  const setEnabled = useSetModEnabled()
  const removeMod = useRemoveMod()
  const pruneMod = usePruneMod()
  const [installOpen, setInstallOpen] = useState(false)
  const { confirm, dialog } = useConfirmDialog()

  const installedOn = new Set(mod.instances.map((i) => i.instance))
  const candidateInstances = instanceNames.filter((n) => !installedOn.has(n))

  const handlePrune = async () => {
    const confirmed = await confirm({
      title: `Remove '${mod.mod_id}' from the store?`,
      description: `Delete the downloaded copy of '${mod.mod_id}' from the shared mod store. It isn't installed on any instance right now.`,
      confirmLabel: 'Remove from store',
    })
    if (!confirmed) return
    pruneMod.mutate(mod.mod_id, { onError: (e) => toast.error(e.message) })
  }

  const handleRemove = async (instanceName: string) => {
    const confirmed = await confirm({
      title: `Remove '${mod.mod_id}'?`,
      description: `Remove '${mod.mod_id}' from '${instanceName}'? You can reinstall it later.`,
      confirmLabel: 'Remove',
    })
    if (!confirmed) return
    removeMod.mutate({ name: instanceName, modId: mod.mod_id }, { onError: (e) => toast.error(e.message) })
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
              {mod.global_version ? `v${mod.global_version} in the shared store` : 'missing from the shared store'}
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
        {mod.instances.length === 0 ? (
          <p className="text-xs text-muted-foreground">
            Not installed on any instance — an orphaned download.
          </p>
        ) : (
          <div className="flex flex-col gap-2">
            {mod.instances.map((entry) => (
              <div
                key={entry.instance}
                className="flex flex-col gap-2 rounded-xl bg-muted/30 px-3 py-2 sm:flex-row sm:items-center sm:justify-between"
              >
                <div className="flex items-center gap-2 text-sm">
                  <span className="font-medium">{entry.instance}</span>
                  <span className="text-xs text-muted-foreground">v{entry.version}</span>
                  {entry.running && <Badge variant="default">running</Badge>}
                </div>
                <div className="flex items-center gap-3">
                  <Switch
                    checked={entry.enabled}
                    onCheckedChange={(enabled) =>
                      setEnabled.mutate(
                        { name: entry.instance, modId: mod.mod_id, enabled },
                        { onError: (e) => toast.error(e.message) },
                      )
                    }
                  />
                  <Button
                    size="sm"
                    variant="destructive"
                    onClick={() => handleRemove(entry.instance)}
                  >
                    Remove
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </CardContent>

      <InstallOnInstancesDialog
        open={installOpen}
        onOpenChange={setInstallOpen}
        modId={mod.mod_id}
        candidateInstances={candidateInstances}
      />
    </Card>
  )
}

function ModSearchSection({ instanceNames }: { instanceNames: string[] }) {
  const [dialogModId, setDialogModId] = useState<string | null>(null)

  return (
    <div className="flex flex-col gap-3">
      <Tabs defaultValue="thunderstore">
        <TabsList variant="line">
          <TabsTrigger value="thunderstore">Thunderstore</TabsTrigger>
          <TabsTrigger value="nexus">Nexus Mods</TabsTrigger>
          <TabsTrigger value="upload">Upload</TabsTrigger>
        </TabsList>
        <TabsContent value="thunderstore">
          <ModSearch onSelect={(mod) => setDialogModId(mod.mod_id)} />
        </TabsContent>
        <TabsContent value="nexus">
          <NexusModSearch onSelect={(mod) => setDialogModId(mod.mod_id)} />
        </TabsContent>
        <TabsContent value="upload">
          <UploadSection instanceNames={instanceNames} />
        </TabsContent>
      </Tabs>

      <InstallOnInstancesDialog
        open={dialogModId !== null}
        onOpenChange={(open) => !open && setDialogModId(null)}
        modId={dialogModId ?? ''}
        candidateInstances={instanceNames}
      />
    </div>
  )
}

// Unlike the per-instance Mods tab, this page has no single target instance
// in scope — pick one before showing the upload form.
function UploadSection({ instanceNames }: { instanceNames: string[] }) {
  const [target, setTarget] = useState('')

  if (instanceNames.length === 0) {
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
          {instanceNames.map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
      </div>
      {target && <UploadModForm name={target} />}
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
  candidateInstances: string[]
}) {
  const [selected, setSelected] = useState<string[]>([])
  const [installing, setInstalling] = useState(false)
  const addMod = useAddMod()

  // Reset checkboxes whenever the dialog opens for a (possibly different)
  // mod, rather than carrying over a stale selection from the last time it
  // was open — this component instance is reused across mods.
  useEffect(() => {
    if (open) setSelected([])
  }, [open])

  const toggle = (name: string) =>
    setSelected((prev) => (prev.includes(name) ? prev.filter((n) => n !== name) : [...prev, name]))

  const handleInstall = async () => {
    const targets = selected
    setInstalling(true)
    await Promise.all(
      targets.map((name) =>
        addMod
          .mutateAsync({ name, modId })
          .then(() => toast.success(`Installing '${modId}' on '${name}'`))
          .catch((e: Error) => toast.error(`${name}: ${e.message}`)),
      ),
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
          {candidateInstances.map((name) => (
            <label key={name} className="flex items-center gap-2 text-sm">
              <Checkbox checked={selected.includes(name)} onCheckedChange={() => toggle(name)} />
              {name}
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
