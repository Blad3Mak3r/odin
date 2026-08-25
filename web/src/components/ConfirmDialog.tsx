import { useCallback, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

interface ConfirmOptions {
  title: string
  description: string
  confirmLabel?: string
}

/**
 * Renders a shadcn Dialog gated behind an awaitable `confirm()` call, so a
 * destructive action can do `if (!(await confirm({...}))) return` instead of
 * the browser's blocking `confirm()`. Spread `dialog` into the component's
 * JSX once; call `confirm()` from any handler.
 */
export function useConfirmDialog() {
  const [open, setOpen] = useState(false)
  const [options, setOptions] = useState<ConfirmOptions | null>(null)
  const resolveRef = useRef<((confirmed: boolean) => void) | null>(null)

  const confirm = useCallback((opts: ConfirmOptions) => {
    setOptions(opts)
    setOpen(true)
    return new Promise<boolean>((resolve) => {
      resolveRef.current = resolve
    })
  }, [])

  const settle = (confirmed: boolean) => {
    resolveRef.current?.(confirmed)
    resolveRef.current = null
    setOpen(false)
  }

  const dialog = (
    <Dialog open={open} onOpenChange={(next) => !next && settle(false)}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{options?.title}</DialogTitle>
          <DialogDescription>{options?.description}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={() => settle(false)}>
            Cancel
          </Button>
          <Button variant="destructive" onClick={() => settle(true)}>
            {options?.confirmLabel ?? 'Confirm'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )

  return { confirm, dialog }
}
