import { ArrowLeft, Eye, EyeOff, Loader2, Pencil, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { PlayersBadge } from '@/components/PlayersBadge'
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
import { Label } from '@/components/ui/label'
import {
  useInstanceResources,
  useInstanceTransition,
  useRenameInstance,
  useRestartInstance,
  useStartInstance,
  useStopInstance,
} from '@/lib/queries'
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
  const transition = useInstanceTransition(instance?.name ?? '').data
  const busy = start.isPending || stop.isPending || restart.isPending || transition !== null
  // `undefined`/`true` both read as "don't second-guess running" — only an
  // explicit `false` (a live tick that's actually seen the supervisor say
  // so) shows "starting" instead, so a fresh page load never flashes it
  // incorrectly before the first tick arrives.
  const resources = useInstanceResources(instance?.name ?? '', !!instance?.running)
  const starting = instance?.running && resources.data?.ready === false

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
        <div className="flex items-center gap-3">
          <div className="flex flex-col">
            <h1 className="text-2xl font-semibold tracking-tight">{instance?.name ?? '…'}</h1>
            {instance && (
              <span className="text-sm text-muted-foreground">
                Odin {instance.odin_version ? `v${instance.odin_version}` : '—'}
              </span>
            )}
          </div>
          {!loading && instance && (
            <>
              <Badge variant={instance.running ? 'default' : 'secondary'}>
                {transition ?? (instance.running ? (starting ? 'starting' : 'running') : 'stopped')}
              </Badge>
              <PlayersBadge name={instance.name} running={instance.running} />
              <RenameInstanceDialog name={instance.name} />
            </>
          )}
        </div>

        <div className="flex flex-wrap gap-2">
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
          <Button
            variant="destructive"
            size="icon"
            aria-label="Delete instance"
            onClick={onDelete}
            disabled={busy || instance?.running}
          >
            <Trash2 className="size-4" />
          </Button>
        </div>
      </div>

      {instance && (
        <div className="flex flex-wrap gap-x-6 gap-y-2 text-sm text-muted-foreground">
          <span>World: {instance.world_name}</span>
          <span>Port: {instance.port}</span>
          <PasswordField password={instance.password} />
          <span>Visibility: {instance.public ? 'public' : 'private'}</span>
        </div>
      )}
    </div>
  )
}

function RenameInstanceDialog({ name }: { name: string }) {
  const [open, setOpen] = useState(false)
  const [newName, setNewName] = useState(name)
  const navigate = useNavigate()
  const renameInstance = useRenameInstance()

  const handleOpenChange = (next: boolean) => {
    setOpen(next)
    if (next) setNewName(name)
  }

  const handleRename = () => {
    const trimmed = newName.trim()
    if (!trimmed || trimmed === name) return
    renameInstance.mutate(
      { name, newName: trimmed },
      {
        onSuccess: () => {
          setOpen(false)
          toast.success(`Instance renamed to '${trimmed}'`)
          navigate(`/instances/${trimmed}`)
        },
        onError: (e) => toast.error(e.message),
      },
    )
  }

  return (
    <>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Rename instance"
        onClick={() => handleOpenChange(true)}
      >
        <Pencil className="size-3.5" />
      </Button>
      <Dialog open={open} onOpenChange={handleOpenChange}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Rename instance</DialogTitle>
          </DialogHeader>
          <div className="flex flex-col gap-2">
            <Label htmlFor="rename-instance">Name</Label>
            <Input
              id="rename-instance"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleRename()}
            />
            <p className="text-xs text-muted-foreground">
              Lowercase letters, digits, and hyphens only.
            </p>
          </div>
          <DialogFooter>
            <Button
              disabled={!newName.trim() || newName.trim() === name || renameInstance.isPending}
              onClick={handleRename}
            >
              {renameInstance.isPending && <Loader2 className="size-4 animate-spin" />}
              Rename
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
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
