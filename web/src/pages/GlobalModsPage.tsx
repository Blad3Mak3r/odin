import { Loader2 } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { ModSearch } from '@/components/ModSearch'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Switch } from '@/components/ui/switch'
import { useConfirmDialog } from '@/components/ConfirmDialog'
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
  const instanceNames = instances.data?.map((i) => i.name) ?? []

  return (
    <div className="flex flex-col gap-8">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Mods</h1>
        <p className="text-sm text-muted-foreground">
          Every mod across all instances, backed by the shared download store.
        </p>
      </div>

      {instances.isError && <QueryError error={instances.error} />}

      <InstalledMods instanceNames={instanceNames} />
      <ModSearchSection instanceNames={instanceNames} />
    </div>
  )
}

function InstalledMods({ instanceNames }: { instanceNames: string[] }) {
  const globalMods = useGlobalMods()

  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-sm font-medium">Installed mods</h2>

      {globalMods.isError && <QueryError error={globalMods.error} />}
      {globalMods.data?.length === 0 && (
        <p className="text-sm text-muted-foreground">No mods installed anywhere yet.</p>
      )}

      <div className="flex flex-col gap-3">
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
    <div className="rounded-md border p-3">
      {dialog}
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <p className="text-sm font-medium">{mod.mod_id}</p>
          <p className="text-xs text-muted-foreground">
            {mod.global_version ? `v${mod.global_version} in the shared store` : 'missing from the shared store'}
          </p>
        </div>
        {mod.instances.length === 0 ? (
          <Button size="sm" variant="ghost" disabled={pruneMod.isPending} onClick={handlePrune}>
            Remove from store
          </Button>
        ) : (
          candidateInstances.length > 0 && (
            <Button size="sm" variant="outline" onClick={() => setInstallOpen(true)}>
              Install on more instances
            </Button>
          )
        )}
      </div>

      {mod.instances.length === 0 ? (
        <p className="mt-2 text-xs text-muted-foreground">
          Not installed on any instance — an orphaned download.
        </p>
      ) : (
        <div className="mt-3 flex flex-col gap-2">
          {mod.instances.map((entry) => (
            <div
              key={entry.instance}
              className="flex flex-col gap-2 rounded-md bg-muted/30 px-3 py-2 sm:flex-row sm:items-center sm:justify-between"
            >
              <div className="flex items-center gap-2 text-sm">
                <span className="font-medium">{entry.instance}</span>
                <span className="text-xs text-muted-foreground">v{entry.version}</span>
                {entry.running && <Badge variant="outline">running</Badge>}
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
                  variant="ghost"
                  onClick={() => handleRemove(entry.instance)}
                >
                  Remove
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      <InstallOnInstancesDialog
        open={installOpen}
        onOpenChange={setInstallOpen}
        modId={mod.mod_id}
        candidateInstances={candidateInstances}
      />
    </div>
  )
}

function ModSearchSection({ instanceNames }: { instanceNames: string[] }) {
  const [dialogModId, setDialogModId] = useState<string | null>(null)

  return (
    <div className="flex flex-col gap-3">
      <ModSearch onSelect={(mod) => setDialogModId(mod.mod_id)} />

      <InstallOnInstancesDialog
        open={dialogModId !== null}
        onOpenChange={(open) => !open && setDialogModId(null)}
        modId={dialogModId ?? ''}
        candidateInstances={instanceNames}
      />
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
              <input
                type="checkbox"
                checked={selected.includes(name)}
                onChange={() => toggle(name)}
                className="size-4 rounded border-input accent-primary"
              />
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
