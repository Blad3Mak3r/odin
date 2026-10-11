import { Download } from 'lucide-react'
import { lazy, Suspense } from 'react'
import { Navigate } from 'react-router-dom'
import { toast } from 'sonner'
import { ModIcon } from '@/components/ModIcon'
import { Badge } from '@/components/ui/badge'
import { buttonVariants } from '@/components/ui/button-variants'
import { Card, CardContent } from '@/components/ui/card'
import { Switch } from '@/components/ui/switch'
import { getModSource, MOD_SOURCE_LABEL } from '@/lib/modSource'
import { useMods, useSetModEnabled } from '@/lib/queries'
import { cn } from '@/lib/utils'

const ModConfigFiles = lazy(() =>
  import('./ModConfigFiles').then((m) => ({ default: m.ModConfigFiles })),
)

export function ModsTab({ id, name, running, path }: { id: string; name: string; running: boolean; path: string[] }) {
  if (path.length > 0 && !(path.length === 1 && path[0] === 'installed')) {
    return <Navigate to={`/instance/${id}/mods`} replace />
  }

  return (
    <div className="flex flex-col gap-8">
      <InstalledMods id={id} name={name} running={running} />
      <Suspense fallback={<span className="text-sm text-muted-foreground">Loading configuration files…</span>}>
        <ModConfigFiles id={id} />
      </Suspense>
    </div>
  )
}

function InstalledMods({ id, name, running }: { id: string; name: string; running: boolean }) {
  const mods = useMods(id)
  const setEnabled = useSetModEnabled()

  return (
    <section className="flex flex-col gap-3">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Installed mods</h2>
          <p className="mt-1 text-sm text-muted-foreground">Install, update, and remove mods from the Valheim game page.</p>
        </div>
        <a
          href={`/api/instances/${id}/valheim/mods/modpack`}
          download
          className={cn(
            buttonVariants({ variant: 'outline', size: 'sm' }),
            !mods.data?.some((mod) => mod.enabled) && 'pointer-events-none opacity-50',
          )}
        >
          <Download className="size-4" />
          Download ModPack
        </a>
      </div>

      {mods.data?.length === 0 && <p className="text-sm text-muted-foreground">No mods installed yet.</p>}
      {running && <p className="text-xs text-muted-foreground">Stop '{name}' before enabling or disabling mods.</p>}

      <div className="grid gap-2 xl:grid-cols-2 2xl:grid-cols-3">
        {mods.data?.map((mod) => (
          <Card key={mod.mod_id} size="sm">
            <CardContent className="flex items-center justify-between gap-3">
              <div className="flex min-w-0 items-center gap-3">
                <ModIcon src={mod.icon} />
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <p className="truncate text-sm font-medium">{mod.mod_id}</p>
                    <Badge variant="outline">{MOD_SOURCE_LABEL[getModSource(mod.mod_id)]}</Badge>
                  </div>
                  <p className="text-xs text-muted-foreground">v{mod.version}</p>
                </div>
              </div>
              <Switch
                aria-label={`${mod.enabled ? 'Disable' : 'Enable'} ${mod.mod_id}`}
                checked={mod.enabled}
                disabled={running || setEnabled.isPending}
                onCheckedChange={(enabled) =>
                  setEnabled.mutate(
                    { name: id, modId: mod.mod_id, enabled },
                    { onError: (error) => toast.error(error.message) },
                  )}
              />
            </CardContent>
          </Card>
        ))}
      </div>
    </section>
  )
}
