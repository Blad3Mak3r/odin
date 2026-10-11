import type { ComponentPropsWithoutRef } from 'react'
import { Gamepad2 } from 'lucide-react'
import sevenDaysIcon from '@/assets/games/7d2d.webp'
import palworldIcon from '@/assets/games/palworld.webp'
import runescapeDragonwildsIcon from '@/assets/games/runescape_dragonwilds.webp'
import rustIcon from '@/assets/games/rust.webp'
import valheimIcon from '@/assets/games/valheim.webp'
import vrisingIcon from '@/assets/games/v_rising.webp'
import type { GameId } from '@/lib/types'
import { cn } from '@/lib/utils'

const GAME_ICON_SOURCES: Record<GameId, string> = {
  valheim: valheimIcon,
  rust: rustIcon,
  vrising: vrisingIcon,
  palworld: palworldIcon,
  'runescape-dragonwilds': runescapeDragonwildsIcon,
  '7d2d': sevenDaysIcon,
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
