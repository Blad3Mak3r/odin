import type { ComponentPropsWithoutRef } from 'react'
import { Gamepad2 } from 'lucide-react'
import sevenDaysIcon from '@/assets/games/7d2d.webp'
import arkAsaIcon from '@/assets/games/ark-asa.webp'
import cs2Icon from '@/assets/games/cs2.webp'
import palworldIcon from '@/assets/games/palworld.webp'
import rustIcon from '@/assets/games/rust.webp'
import valheimIcon from '@/assets/games/valheim.webp'
import type { GameId } from '@/lib/types'
import { cn } from '@/lib/utils'

const GAME_ICON_SOURCES: Record<GameId, string> = {
  'ark-asa': arkAsaIcon,
  valheim: valheimIcon,
  rust: rustIcon,
  palworld: palworldIcon,
  '7d2d': sevenDaysIcon,
  cs2: cs2Icon,
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
