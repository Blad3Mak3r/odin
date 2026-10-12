import { GAME_BANNERS } from '@/lib/games'
import type { GameId } from '@/lib/types'

// Game banner as page atmosphere: strongest at the top edge and dissolving
// into --background by the bottom, so content below sits on the plain page.
// Render inside a `relative isolate` page root; it bleeds over AppShell's
// content padding.
export function GameBackdrop({ game }: { game: GameId }) {
  return (
    <div
      aria-hidden="true"
      className="pointer-events-none absolute -inset-x-4 -top-6 -z-10 h-72 sm:-inset-x-6 sm:-top-8 sm:h-[26rem] min-[114rem]:[mask-image:linear-gradient(to_right,transparent,black_12%,black_88%,transparent)]"
    >
      <div
        className="absolute inset-0 bg-cover bg-top"
        style={{ backgroundImage: `url(${GAME_BANNERS[game]})` }}
      />
      <div
        className="absolute inset-0"
        style={{
          backgroundImage:
            'linear-gradient(to bottom, color-mix(in oklab, var(--background) 55%, transparent), color-mix(in oklab, var(--background) 88%, transparent) 45%, var(--background))',
        }}
      />
    </div>
  )
}
