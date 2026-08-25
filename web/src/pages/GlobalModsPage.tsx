import { useState } from 'react'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import {
  useAddMod,
  useGlobalMods,
  useInstances,
  useModSearch,
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

      <InstalledMods instanceNames={instanceNames} />
      <ModSearch instanceNames={instanceNames} />
    </div>
  )
}

function InstalledMods({ instanceNames }: { instanceNames: string[] }) {
  const globalMods = useGlobalMods()

  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-sm font-medium">Installed mods</h2>

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

  const installedOn = new Set(mod.instances.map((i) => i.instance))
  const candidateInstances = instanceNames.filter((n) => !installedOn.has(n))

  return (
    <div className="rounded-md border p-3">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <p className="text-sm font-medium">{mod.mod_id}</p>
          <p className="text-xs text-muted-foreground">
            {mod.global_version ? `v${mod.global_version} in the shared store` : 'missing from the shared store'}
          </p>
        </div>
        {mod.instances.length === 0 ? (
          <Button
            size="sm"
            variant="ghost"
            disabled={pruneMod.isPending}
            onClick={() =>
              pruneMod.mutate(mod.mod_id, { onError: (e) => toast.error(e.message) })
            }
          >
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
                  onClick={() =>
                    removeMod.mutate(
                      { name: entry.instance, modId: mod.mod_id },
                      { onError: (e) => toast.error(e.message) },
                    )
                  }
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

function ModSearch({ instanceNames }: { instanceNames: string[] }) {
  const [query, setQuery] = useState('')
  const results = useModSearch(query)
  const [dialogModId, setDialogModId] = useState<string | null>(null)

  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-sm font-medium">Search Thunderstore</h2>
      <Input
        placeholder="Search mods by name or author…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />

      {results.isLoading && <p className="text-sm text-muted-foreground">Searching…</p>}

      <div className="flex flex-col gap-2">
        {results.data?.slice(0, 20).map((mod) => (
          <div
            key={mod.mod_id}
            className="flex flex-col gap-2 rounded-md border p-3 sm:flex-row sm:items-center sm:justify-between"
          >
            <div>
              <p className="text-sm font-medium">
                {mod.name} <span className="text-muted-foreground">by {mod.owner}</span>
              </p>
              <p className="line-clamp-1 text-xs text-muted-foreground">{mod.description}</p>
              <div className="mt-1 flex gap-2">
                <Badge variant="outline">v{mod.version}</Badge>
                <Badge variant="outline">{mod.downloads.toLocaleString()} downloads</Badge>
              </div>
            </div>
            <Button size="sm" onClick={() => setDialogModId(mod.mod_id)}>
              Install
            </Button>
          </div>
        ))}
      </div>

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
  const addMod = useAddMod()

  const toggle = (name: string) =>
    setSelected((prev) => (prev.includes(name) ? prev.filter((n) => n !== name) : [...prev, name]))

  const handleInstall = () => {
    for (const name of selected) {
      addMod.mutate(
        { name, modId },
        {
          onSuccess: () => toast.success(`Installing '${modId}' on '${name}'`),
          onError: (e) => toast.error(`${name}: ${e.message}`),
        },
      )
    }
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
          <Button disabled={selected.length === 0} onClick={handleInstall}>
            Install
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
