import { useState } from 'react'
import { Link } from 'react-router-dom'
import { toast } from 'sonner'
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
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import {
  useCreateInstance,
  useInstances,
  useRestartInstance,
  useStartInstance,
  useStopInstance,
} from '@/lib/queries'

export function InstancesPage() {
  const instances = useInstances()

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Instances</h1>
          <p className="text-sm text-muted-foreground">Every Valheim server Odin manages.</p>
        </div>
        <CreateInstanceDialog />
      </div>

      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>World</TableHead>
            <TableHead>Port</TableHead>
            <TableHead>Mods</TableHead>
            <TableHead className="text-right">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {instances.data?.length === 0 && (
            <TableRow>
              <TableCell colSpan={6} className="text-center text-muted-foreground">
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
                <Badge variant={instance.running ? 'default' : 'secondary'}>
                  {instance.running ? 'running' : 'stopped'}
                </Badge>
              </TableCell>
              <TableCell>{instance.world_name}</TableCell>
              <TableCell>{instance.port}</TableCell>
              <TableCell>{instance.installed_mods.length}</TableCell>
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

function InstanceActions({ name, running }: { name: string; running: boolean }) {
  const start = useStartInstance()
  const stop = useStopInstance()
  const restart = useRestartInstance()

  const busy = start.isPending || stop.isPending || restart.isPending

  return (
    <div className="flex justify-end gap-2">
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
