import { Dialog as DialogPrimitive } from '@base-ui/react/dialog'
import { X } from 'lucide-react'
import { Link } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { ACTIVITY_ICONS, describeActivity } from '@/lib/activity'
import { useActivityFeed } from '@/lib/queries'
import type { ActivityEvent } from '@/lib/types'
import { formatRelativeTime } from '@/lib/utils'

function ActivityRow({ event }: { event: ActivityEvent }) {
  const Icon = ACTIVITY_ICONS[event.kind.kind]

  return (
    <div className="flex items-start gap-3 border-b py-3 last:border-b-0">
      <Icon className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="text-sm">{describeActivity(event.kind)}</span>
        <span className="text-xs text-muted-foreground">
          {event.instance && (
            <>
              <Link to={`/instances/${event.instance}`} className="hover:underline">
                {event.instance}
              </Link>
              {' · '}
            </>
          )}
          {formatRelativeTime(event.at)}
        </span>
      </div>
    </div>
  )
}

export function ActivityFeedPanel({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const activity = useActivityFeed()

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Backdrop className="fixed inset-0 z-50 bg-black/30 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0" />
        <DialogPrimitive.Popup className="fixed inset-y-0 right-0 z-50 flex h-svh w-80 max-w-[85vw] translate-x-full flex-col border-l bg-popover text-popover-foreground outline-none transition-transform duration-200 ease-out data-open:translate-x-0">
          <div className="flex items-center justify-between border-b px-4 py-3">
            <DialogPrimitive.Title className="text-sm font-semibold">Activity</DialogPrimitive.Title>
            <DialogPrimitive.Close
              render={<Button variant="ghost" size="icon-sm" aria-label="Close activity" />}
            >
              <X className="size-4" />
            </DialogPrimitive.Close>
          </div>
          <div className="flex-1 overflow-y-auto px-4">
            {activity.data.length === 0 ? (
              <p className="py-6 text-center text-sm text-muted-foreground">No activity yet.</p>
            ) : (
              activity.data.map((event) => <ActivityRow key={event.id} event={event} />)
            )}
          </div>
        </DialogPrimitive.Popup>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  )
}
