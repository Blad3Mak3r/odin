import { CircleCheck, Circle, Loader2, TriangleAlert } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { ApiError, api } from '@/lib/api-client'
import type { VersionView } from '@/lib/types'

type Phase = 'installing' | 'reconnecting' | 'reloading' | 'failed' | 'timeout'

const POLL_INTERVAL = 2_000
const REQUEST_TIMEOUT = 4_000
// `systemctl start --no-block` returns before the unit is reported as
// running, so "not running yet" is not a failure until this has passed.
const START_GRACE = 20_000
const MAX_DURATION = 5 * 60_000

const STEPS: Array<{ phase: Phase; label: string }> = [
  { phase: 'installing', label: 'Installing the new package' },
  { phase: 'reconnecting', label: 'Restarting Odin' },
  { phase: 'reloading', label: 'Reloading the dashboard' },
]

/// A request that fails like this means Odin is down or restarting, as opposed
/// to a real application error from a live server.
function isOutage(error: unknown) {
  if (!(error instanceof ApiError)) return true
  return error.status === 502 || error.status === 503 || error.status === 504
}

/// Follows an Odin self-update from the dashboard: polls `/api/version` until
/// the process has restarted (or the version changed), then reloads the page
/// so the browser picks up the new embedded bundle.
export function UpdateProgressDialog({
  startVersion,
  onClose,
}: {
  startVersion: string
  onClose: () => void
}) {
  const [phase, setPhase] = useState<Phase>('installing')
  const [elapsed, setElapsed] = useState(0)

  useEffect(() => {
    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const startedAt = Date.now()
    let sawOutage = false
    let sawRunning = false

    const tick = async () => {
      let next: Phase | null = null
      try {
        const version = await api.get<VersionView>('/version', {
          signal: AbortSignal.timeout(REQUEST_TIMEOUT),
        })
        if (cancelled) return
        if (sawOutage || version.current_version !== startVersion) {
          setPhase('reloading')
          window.location.reload()
          return
        }
        if (version.system_update_running) {
          sawRunning = true
          next = 'installing'
        } else if (sawRunning || Date.now() - startedAt > START_GRACE) {
          setPhase('failed')
          return
        }
      } catch (error) {
        if (cancelled) return
        if (isOutage(error)) {
          sawOutage = true
          next = 'reconnecting'
        }
      }
      const duration = Date.now() - startedAt
      setElapsed(Math.round(duration / 1000))
      if (duration > MAX_DURATION) {
        setPhase('timeout')
        return
      }
      if (next) setPhase(next)
      timer = setTimeout(tick, POLL_INTERVAL)
    }

    timer = setTimeout(tick, POLL_INTERVAL)
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [startVersion])

  const finished = phase === 'failed' || phase === 'timeout'
  const activeStep = STEPS.findIndex((step) => step.phase === phase)

  return (
    <Dialog open onOpenChange={(open) => !open && finished && onClose()} disablePointerDismissal>
      <DialogContent showCloseButton={false}>
        <DialogHeader>
          <DialogTitle>
            {phase === 'failed' && 'Update did not complete'}
            {phase === 'timeout' && 'Update is taking longer than expected'}
            {!finished && 'Updating Odin'}
          </DialogTitle>
          <DialogDescription aria-live="polite">
            {phase === 'failed' &&
              `Odin is still running version ${startVersion}. Check the odin-update service logs on the host, then try again.`}
            {phase === 'timeout' &&
              'Odin has not come back yet. It may still be installing; reload the page to check.'}
            {!finished &&
              'Odin restarts when the install finishes and this page reloads by itself. Running game servers stay online.'}
          </DialogDescription>
        </DialogHeader>
        {finished ? (
          <div className="flex items-center gap-2 text-sm">
            <TriangleAlert className="size-4 shrink-0" aria-hidden="true" />
            {phase === 'failed' ? 'The install service stopped without restarting Odin.' : `Waiting for ${elapsed}s.`}
          </div>
        ) : (
          <ol className="flex flex-col gap-2 text-sm">
            {STEPS.map((step, index) => (
              <li
                key={step.phase}
                className={index > activeStep ? 'flex items-center gap-2 text-muted-foreground' : 'flex items-center gap-2'}
                aria-current={index === activeStep ? 'step' : undefined}
              >
                {index < activeStep && <CircleCheck className="size-4 shrink-0" aria-hidden="true" />}
                {index === activeStep && <Loader2 className="size-4 shrink-0 animate-spin" aria-hidden="true" />}
                {index > activeStep && <Circle className="size-4 shrink-0" aria-hidden="true" />}
                {step.phase === 'reconnecting' && phase === 'reconnecting' ? 'Reconnecting to Odin…' : step.label}
              </li>
            ))}
          </ol>
        )}
        {finished && (
          <DialogFooter>
            <Button variant="outline" onClick={onClose}>
              Close
            </Button>
            {phase === 'timeout' && (
              <Button onClick={() => window.location.reload()}>Reload page</Button>
            )}
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  )
}
