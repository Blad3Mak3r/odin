import { Users } from 'lucide-react'
import { usePlayers } from '@/lib/queries'

/// A small "N online" indicator, shown next to an instance's running badge.
/// Renders nothing while stopped or with no players connected, so it never
/// competes for attention with the status badge it sits beside.
export function PlayersBadge({ name, running }: { name: string; running: boolean }) {
  const players = usePlayers(name, running)

  if (!running || !players.data || players.data.length === 0) {
    return null
  }

  return (
    <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
      <Users className="size-3.5" />
      {players.data.length}
    </span>
  )
}
