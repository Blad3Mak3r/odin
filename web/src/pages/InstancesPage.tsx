import { Trash2 } from 'lucide-react'
import { useState } from 'react'
import { Link } from 'react-router-dom'
import { toast } from 'sonner'
import { DeleteInstanceDialog } from '@/components/instance/DeleteInstanceDialog'
import { PageHeader } from '@/components/PageHeader'
import { PlayersBadge } from '@/components/PlayersBadge'
import { QueryError } from '@/components/QueryError'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import {
  useCreateInstance,
  useInstanceResources,
  useInstances,
  useRestartInstance,
  useStartInstance,
  useStopInstance,
} from '@/lib/queries'
import { formatBytes } from '@/lib/utils'

export function InstancesPage() {
  const instances = useInstances()

  return (
    <div className="flex flex-col gap-6">
      <PageHeader
        title="Instances"
        description="Every Valheim server Odin manages."
        action={<CreateInstanceDialog />}
      />

      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-full">Name</TableHead>
            <TableHead>Status</TableHead>
            <TableHead className="hidden md:table-cell">World</TableHead>
            <TableHead className="hidden md:table-cell">Port</TableHead>
            <TableHead className="hidden sm:table-cell">Mods</TableHead>
            <TableHead className="hidden lg:table-cell">Resources</TableHead>
            <TableHead className="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {instances.isLoading &&
            Array.from({ length: 3 }, (_, i) => (
              // Loading placeholder rows have no stable id and never reorder.
              // eslint-disable-next-line react/no-array-index-key
              <TableRow key={i}>
                <TableCell colSpan={7}>
                  <Skeleton className="h-5 w-full" />
                </TableCell>
              </TableRow>
            ))}
          {instances.isError && (
            <TableRow>
              <TableCell colSpan={7}>
                <QueryError error={instances.error} />
              </TableCell>
            </TableRow>
          )}
          {instances.data?.length === 0 && (
            <TableRow>
              <TableCell colSpan={7} className="text-center text-muted-foreground">
                No instances yet — create one to get started.
              </TableCell>
            </TableRow>
          )}
          {instances.data?.map((instance) => (
            <TableRow key={instance.name}>
              <TableCell className="font-medium">
                <Link to={`/instances/${instance.name}`} className="hover:underline">
                  {instance.name}
                </Link>
              </TableCell>
              <TableCell>
                <div className="flex items-center gap-2">
                  <Badge variant={instance.running ? 'default' : 'secondary'}>
                    {instance.running ? 'running' : 'stopped'}
                  </Badge>
                  <PlayersBadge name={instance.name} running={instance.running} />
                </div>
              </TableCell>
              <TableCell className="hidden md:table-cell">{instance.world_name}</TableCell>
              <TableCell className="hidden md:table-cell">{instance.port}</TableCell>
              <TableCell className="hidden sm:table-cell">{instance.installed_mods.length}</TableCell>
              <TableCell className="hidden text-muted-foreground lg:table-cell">
                <InstanceResourceCell name={instance.name} running={instance.running} />
              </TableCell>
              <TableCell className="text-right">
                <InstanceActions name={instance.name} running={instance.running} />
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

function InstanceResourceCell({ name, running }: { name: string; running: boolean }) {
  const resources = useInstanceResources(name, running)

  if (!running || !resources.data) {
    return <span>—</span>
  }

  return (
    <span className="text-sm">
      {resources.data.cpu_percent.toFixed(0)}% · {formatBytes(resources.data.memory_bytes)}
    </span>
  )
}

function InstanceActions({ name, running }: { name: string; running: boolean }) {
  const start = useStartInstance()
  const stop = useStopInstance()
  const restart = useRestartInstance()
  const [deleteOpen, setDeleteOpen] = useState(false)

  const busy = start.isPending || stop.isPending || restart.isPending

  return (
    <div className="flex justify-end gap-2">
      <DeleteInstanceDialog name={name} open={deleteOpen} onOpenChange={setDeleteOpen} />
      {running ? (
        <>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => restart.mutate(name)}>
            Restart
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() =>
              stop.mutate(name, {
                onError: (e) => toast.error(e.message),
              })
            }
          >
            Stop
          </Button>
        </>
      ) : (
        <Button
          size="sm"
          disabled={busy}
          onClick={() =>
            start.mutate(name, {
              onError: (e) => toast.error(e.message),
            })
          }
        >
          Start
        </Button>
      )}
      <Button
        size="sm"
        variant="destructive"
        disabled={running}
        aria-label={`Delete ${name}`}
        onClick={() => setDeleteOpen(true)}
      >
        <Trash2 className="size-4" />
      </Button>
    </div>
  )
}

function CreateInstanceDialog() {
  const [open, setOpen] = useState(false)
  const [name, setName] = useState('')
  const createInstance = useCreateInstance()

  const handleCreate = () => {
    createInstance.mutate(name, {
      onSuccess: () => {
        setOpen(false)
        setName('')
        toast.success(`Instance '${name}' created`)
      },
      onError: (e) => toast.error(e.message),
    })
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button>New instance</Button>} />
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Create instance</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-2">
          <Label htmlFor="instance-name">Name</Label>
          <Input
            id="instance-name"
            placeholder="my-server"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && name && handleCreate()}
          />
          <p className="text-xs text-muted-foreground">
            Lowercase letters, digits, and hyphens only. A port and password are assigned
            automatically.
          </p>
        </div>
        <DialogFooter>
          <Button disabled={!name || createInstance.isPending} onClick={handleCreate}>
            Create
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
