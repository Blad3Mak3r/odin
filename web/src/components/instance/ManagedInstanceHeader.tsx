import { ArrowLeft, Loader2 } from 'lucide-react'
import { Link, useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { GameIcon } from '@/components/GameIcon'
import { ManageInstanceDialog } from '@/components/instance/ManageInstanceDialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useManagedInstanceAction, useManagedInstanceTransition } from '@/lib/queries'
import type { ManagedInstanceView } from '@/lib/types'

export function ManagedInstanceHeader({ instance }: { instance: ManagedInstanceView }) {
  const navigate = useNavigate()
  const start = useManagedInstanceAction('start')
  const stop = useManagedInstanceAction('stop')
  const restart = useManagedInstanceAction('restart')
  const transition = useManagedInstanceTransition(instance.id, instance.game, instance.name)
  const busy = start.isPending || stop.isPending || restart.isPending || transition.data !== null
  const action = { id: instance.id }
  const port = numberConfig(instance.config, 'port')
  const queryPort = numberConfig(instance.config, 'query_port')
  const rconPort = numberConfig(instance.config, 'rcon_port')
  const hostname = stringConfig(instance.config, 'hostname')
  const map = stringConfig(instance.config, 'level')

  return (
    <div className="flex flex-col gap-3">
      <Link
        to="/instances"
        className="flex w-fit items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
      >
        <ArrowLeft className="size-4" />
        Instances
      </Link>

      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex flex-wrap items-center gap-3">
          <GameIcon game={instance.game} className="size-9 rounded-md" />
          <div className="flex flex-col">
            <h1 className="text-2xl font-semibold tracking-tight">{instance.name}</h1>
            <span className="text-sm text-muted-foreground">
              Odin {instance.odin_version ? `v${instance.odin_version}` : '—'}
            </span>
          </div>
          <Badge variant={instance.running ? 'default' : 'secondary'}>
            {transition.data ?? (instance.running ? 'running' : 'stopped')}
          </Badge>
          {instance.tags.map((tag) => <Badge key={tag} variant="outline">{tag}</Badge>)}
        </div>

        <div className="flex flex-wrap items-center gap-2">
          {instance.running ? (
            <>
              <Button
                size="sm"
                variant="outline"
                disabled={busy}
                onClick={() => restart.mutate(action, { onError: (error) => toast.error(error.message) })}
              >
                {restart.isPending && <Loader2 className="size-4 animate-spin" />}
                Restart
              </Button>
              <Button
                size="sm"
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

      <div className="flex flex-wrap gap-x-6 gap-y-2 text-sm text-muted-foreground">
        <span>Game: {gameName(instance.game)}</span>
        {hostname && <span>Hostname: {hostname}</span>}
        {port !== null && <span>Port: {port}</span>}
        {queryPort !== null && <span>Query port: {queryPort}</span>}
        {rconPort !== null && <span>RCON port: {rconPort}</span>}
        {map && <span>Map: {map}</span>}
      </div>
    </div>
  )
}

function gameName(game: ManagedInstanceView['game']) {
  return {
    valheim: 'Valheim',
    rust: 'Rust',
    vrising: 'V Rising',
    palworld: 'Palworld',
    'runescape-dragonwilds': 'RuneScape: Dragonwilds',
  }[game]
}

function numberConfig(config: Record<string, unknown>, key: string) {
  const value = config[key]
  return typeof value === 'number' ? value : null
}

function stringConfig(config: Record<string, unknown>, key: string) {
  const value = config[key]
  return typeof value === 'string' && value ? value : null
}
