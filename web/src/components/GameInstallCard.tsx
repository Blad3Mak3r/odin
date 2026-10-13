import type { ReactNode } from 'react'
import { Link } from 'react-router-dom'
import { GameIcon } from '@/components/GameIcon'
import { ProtonIcon, SteamIcon } from '@/components/PlatformIcons'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { GAME_BANNERS } from '@/lib/games'
import type { GameView } from '@/lib/types'

// Sits above the card's stretched link so it can be hovered and focused.
function PlatformBadge({ label, children }: { label: string; children: ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={<span role="img" aria-label={label} tabIndex={0} />}
        className="relative z-10 text-muted-foreground outline-none hover:text-foreground focus-visible:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      >
        {children}
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

export function GameCard({ game, instances, running }: { game: GameView; instances: number; running: number }) {
  return (
    <Card className="relative transition-colors focus-within:ring-2 focus-within:ring-ring hover:bg-muted/40">
      <img src={GAME_BANNERS[game.id]} alt="" loading="lazy" decoding="async" className="h-24 w-full border-b object-cover object-top" />
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <GameIcon game={game.id} className="size-7" />
          <Link to={`/games/${game.id}`} className="outline-none after:absolute after:inset-0">{game.name}</Link>
        </CardTitle>
        <CardDescription className="flex items-center gap-2">
          <PlatformBadge label={`SteamCMD App ID ${game.steam_app_id}`}>
            <SteamIcon className="size-4" />
          </PlatformBadge>
          {game.requires_proton && (
            <PlatformBadge label="Runs under Proton">
              <ProtonIcon className="size-4" />
            </PlatformBadge>
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-wrap items-center gap-2 text-sm">
        <span className="tabular-nums text-muted-foreground">
          {instances} {instances === 1 ? 'instance' : 'instances'} · {running} running
        </span>
      </CardContent>
    </Card>
  )
}
