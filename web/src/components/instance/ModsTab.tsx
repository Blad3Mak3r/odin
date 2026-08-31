import { Download, Loader2 } from 'lucide-react'
import { lazy, Suspense, useState } from 'react'
import { toast } from 'sonner'
import { JobProgress } from '@/components/JobProgress'
import { ModIcon } from '@/components/ModIcon'
import { ModSearch } from '@/components/ModSearch'
import { NexusModSearch } from '@/components/NexusModSearch'
import { UploadModForm } from '@/components/UploadModForm'
import { Badge } from '@/components/ui/badge'
import { Button, buttonVariants } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useConfirmDialog } from '@/components/ConfirmDialog'
import { useJobSocket } from '@/hooks/useJobSocket'
import { getModSource, MOD_SOURCE_LABEL } from '@/lib/modSource'
import { useAddMod, useMods, useRemoveMod, useSetModEnabled, useUpdateMods } from '@/lib/queries'
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

function InstalledMods({ name, running }: { name: string; running: boolean }) {
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
                </div>
              </div>
              <div className="flex items-center gap-3">
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
    </div>
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
