import { Loader2 } from 'lucide-react'
import { lazy, Suspense, useState } from 'react'
import { toast } from 'sonner'
import { JobProgress } from '@/components/JobProgress'
import { ModIcon } from '@/components/ModIcon'
import { ModSearch } from '@/components/ModSearch'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { useJobSocket } from '@/hooks/useJobSocket'
import { useAddMod, useMods, useRemoveMod, useSetModEnabled, useUpdateMods } from '@/lib/queries'

const ModConfigFiles = lazy(() =>
  import('./ModConfigFiles').then((m) => ({ default: m.ModConfigFiles })),
)

export function ModsTab({ name }: { name: string }) {
  return (
    <Tabs defaultValue="installed">
      <TabsList variant="line">
        <TabsTrigger value="installed">Installed</TabsTrigger>
        <TabsTrigger value="marketplace">Marketplace</TabsTrigger>
      </TabsList>
      <TabsContent value="installed">
        <div className="flex flex-col gap-8">
          <InstalledMods name={name} />
          <Suspense fallback={<Loader2 className="size-4 animate-spin text-muted-foreground" />}>
            <ModConfigFiles name={name} />
          </Suspense>
        </div>
      </TabsContent>
      <TabsContent value="marketplace">
        <ModInstallSearch name={name} />
      </TabsContent>
    </Tabs>
  )
}

function InstalledMods({ name }: { name: string }) {
  const mods = useMods(name)
  const setEnabled = useSetModEnabled()
  const removeMod = useRemoveMod()
  const updateMods = useUpdateMods()
  const [jobId, setJobId] = useState<string | null>(null)
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

      {mods.data?.length === 0 && (
        <p className="text-sm text-muted-foreground">No mods installed yet.</p>
      )}

      <div className="grid gap-2 xl:grid-cols-2 2xl:grid-cols-3">
        {mods.data?.map((m) => (
          <Card key={m.mod_id} size="sm">
            <CardContent className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
              <div className="flex items-center gap-3">
                <ModIcon src={m.icon} />
                <div>
                  <p className="text-sm font-medium">{m.mod_id}</p>
                  <p className="text-xs text-muted-foreground">v{m.version}</p>
                </div>
              </div>
              <div className="flex items-center gap-3">
                <Switch
                  checked={m.enabled}
                  onCheckedChange={(enabled) =>
                    setEnabled.mutate(
                      { name, modId: m.mod_id, enabled },
                      { onError: (e) => toast.error(e.message) },
                    )
                  }
                />
                <Button size="sm" variant="destructive" onClick={() => handleRemove(m.mod_id)}>
                  Remove
                </Button>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {jobId && <JobProgress log={job.log} status={job.status} connected={job.connected} />}
    </div>
  )
}

function ModInstallSearch({ name }: { name: string }) {
  const addMod = useAddMod()
  const [jobId, setJobId] = useState<string | null>(null)
  const job = useJobSocket(jobId)

  return (
    <div className="flex flex-col gap-3">
      <ModSearch
        selectDisabled={() => addMod.isPending}
        onSelect={(mod) =>
          addMod.mutate(
            { name, modId: mod.mod_id },
            {
              onSuccess: (handle) => setJobId(handle.id),
              onError: (e) => toast.error(e.message),
            },
          )
        }
      />

      {jobId && <JobProgress log={job.log} status={job.status} connected={job.connected} />}
    </div>
  )
}
