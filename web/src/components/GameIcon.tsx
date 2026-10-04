import type { ComponentPropsWithoutRef } from 'react'
import type { GameId } from '@/lib/types'
import { cn } from '@/lib/utils'

const GAME_ICON_SOURCES: Record<GameId, string> = {
  valheim: '/games/valheim.png',
  rust: '/games/rust.png',
}

export function GameIcon({
  game,
  className,
  ...props
}: { game: GameId } & Omit<ComponentPropsWithoutRef<'img'>, 'alt' | 'src'>) {
  return (
    <img
      src={GAME_ICON_SOURCES[game]}
      alt=""
      aria-hidden="true"
      className={cn('size-5 shrink-0 rounded object-cover', className)}
      {...props}
    />
  )
}
