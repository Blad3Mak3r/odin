import { Loader2 } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useDeleteInstance } from '@/lib/queries'

export function DeleteInstanceDialog({
  name,
  open,
  onOpenChange,
  onDeleted,
}: {
  name: string
  open: boolean
  onOpenChange: (open: boolean) => void
  onDeleted?: () => void
}) {
  const [keepBackups, setKeepBackups] = useState(false)
  const deleteInstance = useDeleteInstance()

  // Reset to the default every time the dialog opens, rather than carrying
  // over the choice from the last instance this dialog was used for.
  // Comparing against the previous `open` during render (instead of an
  // effect) avoids an extra commit.
  const [prevOpen, setPrevOpen] = useState(open)
  if (open !== prevOpen) {
    setPrevOpen(open)
    if (open) setKeepBackups(false)
  }

  const handleDelete = () => {
    deleteInstance.mutate(
      { name, keepBackups },
      {
        onSuccess: () => {
          onOpenChange(false)
          toast.success(`Instance '${name}' deleted`)
          onDeleted?.()
        },
        onError: (e) => toast.error(e.message),
      },
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete '{name}'?</DialogTitle>
        </DialogHeader>
        <p className="text-sm text-muted-foreground">
          Permanently delete '{name}'? This removes its world saves, config, and mods.
        </p>
        <label className="flex items-center gap-2 text-sm">
          <Checkbox checked={keepBackups} onCheckedChange={setKeepBackups} />
          Keep its existing backups
        </label>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="destructive" disabled={deleteInstance.isPending} onClick={handleDelete}>
            {deleteInstance.isPending && <Loader2 className="size-4 animate-spin" />}
            Delete instance
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
