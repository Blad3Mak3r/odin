import { Loader2 } from 'lucide-react'
import { lazy, Suspense, useState } from 'react'
import { toast } from 'sonner'
import { ModSearch } from '@/components/ModSearch'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useJobSocket } from '@/hooks/useJobSocket'
import { useAddMod, useMods, useRemoveMod, useSetModEnabled, useUpdateMods } from '@/lib/queries'

const ModConfigFiles = lazy(() =>
  import('./ModConfigFiles').then((m) => ({ default: m.ModConfigFiles })),
)

export function ModsTab({ name }: { name: string }) {
  return (
    <div className="flex flex-col gap-8">
      <InstalledMods name={name} />
      <Suspense fallback={<Loader2 className="size-4 animate-spin text-muted-foreground" />}>
        <ModConfigFiles name={name} />
      </Suspense>
      <ModInstallSearch name={name} />
    </div>
  )
}

function InstalledMods({ name }: { name: string }) {
  const mods = useMods(name)
  const setEnabled = useSetModEnabled()
  const removeMod = useRemoveMod()
  const updateMods = useUpdateMods()
  const [jobId, setJobId] = useState<string | null>(null)
  const job = useJobSocket(jobId)

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <h2 className="text-sm font-medium">Installed mods</h2>
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

      <div className="flex flex-col gap-2">
        {mods.data?.map((m) => (
          <div
            key={m.mod_id}
            className="flex flex-col gap-2 rounded-md border p-3 sm:flex-row sm:items-center sm:justify-between"
          >
            <div>
              <p className="text-sm font-medium">{m.mod_id}</p>
              <p className="text-xs text-muted-foreground">v{m.version}</p>
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
              <Button
                size="sm"
                variant="ghost"
                onClick={() =>
                  removeMod.mutate(
                    { name, modId: m.mod_id },
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

      {jobId && <JobProgress log={job.log} status={job.status} />}
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

      {jobId && <JobProgress log={job.log} status={job.status} />}
    </div>
  )
}

function JobProgress({
  log,
  status,
}: {
  log: string[]
  status: { status: string; message?: string } | null
}) {
  return (
    <div className="rounded-md border bg-muted/30 p-3">
      <div className="mb-2 flex items-center gap-2">
        {status?.status === 'running' || status?.status === 'queued' ? (
          <Loader2 className="size-4 animate-spin" />
        ) : null}
        <Badge variant={status?.status === 'failed' ? 'destructive' : 'secondary'}>
          {status?.status ?? 'starting'}
        </Badge>
        {status?.status === 'failed' && (
          <span className="text-xs text-destructive">{status.message}</span>
        )}
      </div>
      <div className="max-h-32 overflow-y-auto font-mono text-xs">
        {log.map((line, i) => (
          // Job log lines have no stable id and never reorder, only append.
          // eslint-disable-next-line react/no-array-index-key
          <div key={i}>{line}</div>
        ))}
      </div>
    </div>
  )
}
