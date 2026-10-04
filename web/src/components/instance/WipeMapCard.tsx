import { useState } from 'react'
import { toast } from 'sonner'
import { JobProgress } from '@/components/JobProgress'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useJobSocket } from '@/hooks/useJobSocket'
import { useFullWipeRust, useWipeRustMap } from '@/lib/queries'

type WipeMode = 'map' | 'full'

export function WipeMapCard({ name, running }: { name: string; running: boolean }) {
  const [open, setOpen] = useState(false)
  const [confirmation, setConfirmation] = useState('')
  const [jobId, setJobId] = useState<string | null>(null)
  const [mode, setMode] = useState<WipeMode>('map')
  const wipeMap = useWipeRustMap()
  const fullWipe = useFullWipeRust()
  const job = useJobSocket(jobId)
  const confirmed = confirmation === name
  const pending = wipeMap.isPending || fullWipe.isPending
  const full = mode === 'full'

  const onOpenChange = (next: boolean) => {
    setOpen(next)
    if (!next) setConfirmation('')
  }

  const openDialog = (nextMode: WipeMode) => {
    setMode(nextMode)
    onOpenChange(true)
  }

  const onSuccess = (handle: { id: string }) => {
    setJobId(handle.id)
    onOpenChange(false)
    toast.success((full ? 'Full wipe' : 'Map wipe') + " queued for '" + name + "'")
  }

  const wipe = () => {
    const variables = { name, confirmation }
    const options = {
      onSuccess,
      onError: (error: Error) => toast.error(error.message),
    }
    if (full) {
      fullWipe.mutate(variables, options)
    } else {
      wipeMap.mutate(variables, options)
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Danger zone</CardTitle>
        <CardDescription>Destructive server actions that cannot be undone.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="flex flex-col gap-3 rounded-xl border p-3 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex flex-col gap-1">
            <h3 className="text-sm font-medium">Wipe Map</h3>
            <p className="text-sm text-muted-foreground">
              Wipe Map removes the current world, player progress, buildings, and inventories. Blueprints and Rust+ pairing are kept. This cannot be undone.
            </p>
            {running && <p className="text-xs text-muted-foreground">Stop this server before wiping its map.</p>}
          </div>
          <Button size="sm" variant="destructive" disabled={running || pending} onClick={() => openDialog('map')}>
            Wipe map
          </Button>
        </div>
        <div className="flex flex-col gap-3 rounded-xl border p-3 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex flex-col gap-1">
            <h3 className="text-sm font-medium">Full Wipe</h3>
            <p className="text-sm text-muted-foreground">
              Full Wipe also removes every learned blueprint. Rust+ pairing and server configuration are kept. This cannot be undone.
            </p>
            {running && <p className="text-xs text-muted-foreground">Stop this server before fully wiping it.</p>}
          </div>
          <Button size="sm" variant="destructive" disabled={running || pending} onClick={() => openDialog('full')}>
            Full wipe
          </Button>
        </div>

        {jobId && <JobProgress log={job.log} status={job.status} connected={job.connected} />}

        <Dialog open={open} onOpenChange={onOpenChange}>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>{full ? 'Fully wipe' : 'Wipe map for'} '{name}'?</DialogTitle>
              <DialogDescription>
                {full
                  ? 'This permanently removes the current world, player progress, buildings, inventories, and learned blueprints. Rust+ pairing is kept.'
                  : 'This permanently removes the current world, player progress, buildings, and inventories. Blueprints and Rust+ pairing are kept.'}
              </DialogDescription>
            </DialogHeader>
            <div className="flex flex-col gap-2">
              <Label htmlFor="wipe-map-confirmation">Type '{name}' to confirm</Label>
              <Input
                id="wipe-map-confirmation"
                autoComplete="off"
                value={confirmation}
                disabled={pending}
                onChange={(event) => setConfirmation(event.target.value)}
              />
            </div>
            <DialogFooter>
              <Button variant="outline" disabled={pending} onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              <Button variant="destructive" disabled={!confirmed || pending} onClick={wipe}>
                {full ? 'Full wipe' : 'Wipe map'}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </CardContent>
    </Card>
  )
}
