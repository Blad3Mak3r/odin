import type { ComponentPropsWithoutRef } from 'react'
import { Gamepad2 } from 'lucide-react'
import type { GameId } from '@/lib/types'
import { cn } from '@/lib/utils'

const GAME_ICON_SOURCES: Partial<Record<GameId, string>> = {
  valheim: '/games/valheim.png',
  rust: '/games/rust.png',
  vrising: '/games/vrising.png',
  palworld: '/games/palworld.png',
  'runescape-dragonwilds': '/games/runescape-dragonwilds.png',
}

export function GameIcon({
  game,
  className,
  ...props
}: { game: GameId } & Omit<ComponentPropsWithoutRef<'img'>, 'alt' | 'src'>) {
  const source = GAME_ICON_SOURCES[game]
  if (!source) return <Gamepad2 aria-hidden="true" className={cn('size-5 shrink-0', className)} />
  return (
    <img
      src={source}
      alt=""
      aria-hidden="true"
      className={cn('size-5 shrink-0 rounded object-cover', className)}
      {...props}
    />
  )
}
