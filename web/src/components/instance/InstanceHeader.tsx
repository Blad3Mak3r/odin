import { ArrowLeft, Eye, EyeOff, Loader2, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { Link } from 'react-router-dom'
import { toast } from 'sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useRestartInstance, useStartInstance, useStopInstance } from '@/lib/queries'
import type { InstanceView } from '@/lib/types'

export function InstanceHeader({
  instance,
  loading,
  onDelete,
}: {
  instance: InstanceView | undefined
  loading: boolean
  onDelete: () => void
}) {
  const start = useStartInstance()
  const stop = useStopInstance()
  const restart = useRestartInstance()
  const busy = start.isPending || stop.isPending || restart.isPending

  return (
    <div className="flex flex-col gap-3">
      <Link
        to="/instances"
        className="flex w-fit items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
      >
        <ArrowLeft className="size-4" />
        Instances
      </Link>

      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-semibold tracking-tight">{instance?.name ?? '…'}</h1>
          {!loading && instance && (
            <Badge variant={instance.running ? 'default' : 'secondary'}>
              {instance.running ? 'running' : 'stopped'}
            </Badge>
          )}
        </div>

        <div className="flex gap-2">
          {instance?.running ? (
            <>
              <Button
                variant="outline"
                disabled={busy}
                onClick={() => restart.mutate(instance.name)}
              >
                {restart.isPending && <Loader2 className="size-4 animate-spin" />}
                Restart
              </Button>
              <Button
                variant="outline"
                disabled={busy}
                onClick={() =>
                  stop.mutate(instance.name, { onError: (e) => toast.error(e.message) })
                }
              >
                {stop.isPending && <Loader2 className="size-4 animate-spin" />}
                Stop
              </Button>
            </>
          ) : (
            <Button
              disabled={busy || !instance}
              onClick={() =>
                instance &&
                start.mutate(instance.name, { onError: (e) => toast.error(e.message) })
              }
            >
              {start.isPending && <Loader2 className="size-4 animate-spin" />}
              Start
            </Button>
          )}
          <Button variant="destructive" size="icon" onClick={onDelete} disabled={instance?.running}>
            <Trash2 className="size-4" />
          </Button>
        </div>
      </div>

      {instance && (
        <div className="flex gap-6 text-sm text-muted-foreground">
          <span>World: {instance.world_name}</span>
          <span>Port: {instance.port}</span>
          <PasswordField password={instance.password} />
          <span>Visibility: {instance.public ? 'public' : 'private'}</span>
        </div>
      )}
    </div>
  )
}

function PasswordField({ password }: { password: string | null }) {
  const [visible, setVisible] = useState(false)

  if (!password) {
    return <span>Password: —</span>
  }

  return (
    <span className="inline-flex items-center gap-1.5">
      Password: {visible ? password : '••••••••'}
      <button
        type="button"
        aria-label={visible ? 'Hide password' : 'Show password'}
        onClick={() => setVisible((v) => !v)}
        className="text-muted-foreground hover:text-foreground"
      >
        {visible ? <EyeOff className="size-3.5" /> : <Eye className="size-3.5" />}
      </button>
    </span>
  )
}
