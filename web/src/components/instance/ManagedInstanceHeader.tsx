import { ChevronRight, Loader2 } from 'lucide-react'
import { Link, useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { GameIcon } from '@/components/GameIcon'
import { ManageInstanceDialog } from '@/components/instance/ManageInstanceDialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useManagedInstanceAction, useManagedInstanceTransition, useManagedResources, useMigrateRuntime, useUpdateRuntime } from '@/lib/queries'
import { gameLabel } from '@/lib/games'
import type { ManagedInstanceView } from '@/lib/types'

export function ManagedInstanceHeader({ instance }: { instance: ManagedInstanceView }) {
  const navigate = useNavigate()
  const start = useManagedInstanceAction('start')
  const stop = useManagedInstanceAction('stop')
  const restart = useManagedInstanceAction('restart')
  const migrate = useMigrateRuntime()
  const update = useUpdateRuntime()
  const transition = useManagedInstanceTransition(instance.id, instance.game, instance.name)
  const resources = useManagedResources(instance.id, instance.running && instance.capabilities.readiness)
  const busy = start.isPending || stop.isPending || restart.isPending || migrate.isPending || update.isPending || transition.data !== null
  const action = { id: instance.id }
  const port = numberConfig(instance.config, 'port')
  const queryPort = numberConfig(instance.config, 'query_port')
  const rconPort = numberConfig(instance.config, 'rcon_port')
  const hostname = stringConfig(instance.config, 'hostname')
  const map = stringConfig(instance.config, 'level')

  return (
    <header className="flex flex-col gap-3 pt-2">
      <nav aria-label="Breadcrumb" className="flex flex-wrap items-center gap-1 text-sm text-foreground/80">
        <Link to="/games" className="hover:text-foreground hover:underline">Games</Link>
        <ChevronRight aria-hidden="true" className="size-3.5" />
        <Link to={`/games/${instance.game}`} className="hover:text-foreground hover:underline">{gameLabel(instance.game)}</Link>
        <ChevronRight aria-hidden="true" className="size-3.5" />
        <Link to={`/games/${instance.game}/instances`} className="hover:text-foreground hover:underline">Instances</Link>
      </nav>

      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex flex-wrap items-center gap-3">
          <GameIcon game={instance.game} className="size-12" />
          <div className="flex flex-col">
            <h1 className="text-2xl font-semibold tracking-tight">{instance.name}</h1>
            {instance.odin_version && <span className="text-sm text-foreground/80">Running Odin v{instance.odin_version}</span>}
          </div>
          <Badge variant={instance.running ? 'default' : 'secondary'}>
            {transition.data ?? (instance.running ? 'running' : 'stopped')}
          </Badge>
          {instance.capabilities.container_runtime && <Badge variant="outline">{instance.execution}</Badge>}
          {instance.running && instance.capabilities.readiness && resources.data && <Badge variant={resources.data.ready ? 'default' : 'secondary'}>{resources.data.ready ? 'ready' : 'starting'}</Badge>}
          {instance.tags.map((tag) => <Badge key={tag} variant="outline">{tag}</Badge>)}
        </div>

        <div className="flex flex-wrap items-center gap-2">
          {instance.migration_required ? (
            <Button
              size="sm"
              className="max-sm:h-11"
              disabled={busy || instance.running}
              onClick={() => migrate.mutate(instance.id, { onSuccess: () => toast.success(`${gameLabel(instance.game)} instance migrated to Podman`), onError: (error) => toast.error(error.message) })}
            >
              {migrate.isPending && <Loader2 className="size-4 animate-spin" />}
              Migrate to Podman
            </Button>
          ) : instance.execution === 'podman' && instance.running ? (
            <Button
              size="sm"
              className="max-sm:h-11"
              variant="outline"
              disabled={busy}
              onClick={() => update.mutate(instance.id, { onSuccess: () => toast.success(`${gameLabel(instance.game)} update queued`), onError: (error) => toast.error(error.message) })}
            >
              {update.isPending && <Loader2 className="size-4 animate-spin" />}
              Update
            </Button>
          ) : null}
          {instance.running ? (
            <>
              <Button
                size="sm"
              className="max-sm:h-11"
                variant="outline"
                disabled={busy}
                onClick={() => restart.mutate(action, { onError: (error) => toast.error(error.message) })}
              >
                {restart.isPending && <Loader2 className="size-4 animate-spin" />}
                Restart
              </Button>
              <Button
                size="sm"
              className="max-sm:h-11"
                variant="outline"
                disabled={busy}
                onClick={() => stop.mutate(action, { onError: (error) => toast.error(error.message) })}
              >
                {stop.isPending && <Loader2 className="size-4 animate-spin" />}
                Stop
              </Button>
            </>
          ) : (
            <Button
              size="sm"
              className="max-sm:h-11"
              disabled={busy}
              onClick={() => start.mutate(action, { onError: (error) => toast.error(error.message) })}
            >
              {start.isPending && <Loader2 className="size-4 animate-spin" />}
              Start
            </Button>
          )}
          <ManageInstanceDialog instance={instance} onNavigate={navigate} />
        </div>
      </div>

      <dl className="flex flex-wrap gap-x-6 gap-y-1 text-sm tabular-nums">
        {hostname && <Datum label="Hostname" value={hostname} />}
        {port !== null && <Datum label="Port" value={port} />}
        {queryPort !== null && <Datum label="Query port" value={queryPort} />}
        {rconPort !== null && <Datum label="RCON port" value={rconPort} />}
        {map && <Datum label="Map" value={map} />}
      </dl>
    </header>
  )
}

function Datum({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="flex gap-1.5">
      <dt className="text-foreground/80">{label}</dt>
      <dd className="font-medium">{value}</dd>
    </div>
  )
}

function numberConfig(config: Record<string, unknown>, key: string) {
  const value = config[key]
  return typeof value === 'number' ? value : null
}

function stringConfig(config: Record<string, unknown>, key: string) {
  const value = config[key]
  return typeof value === 'string' && value ? value : null
}
