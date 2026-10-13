import { Link } from 'react-router-dom'
import { GameIcon } from '@/components/GameIcon'
import { GamePlatformBadges } from '@/components/PlatformIcons'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { GAME_BANNERS } from '@/lib/games'
import type { GameView } from '@/lib/types'

export function GameCard({ game, instances, running }: { game: GameView; instances: number; running: number }) {
  return (
    <Card className="relative transition-colors focus-within:ring-2 focus-within:ring-ring hover:bg-muted/40">
      <img src={GAME_BANNERS[game.id]} alt="" loading="lazy" decoding="async" className="h-24 w-full border-b object-cover object-top" />
      <div className="absolute top-2 right-2 z-10 flex items-center gap-2 rounded-full bg-background/80 px-2 py-1 backdrop-blur-sm">
        <GamePlatformBadges game={game} />
      </div>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <GameIcon game={game.id} className="size-7" />
          <Link to={`/games/${game.id}`} className="outline-none after:absolute after:inset-0">{game.name}</Link>
        </CardTitle>
      </CardHeader>
      <CardContent className="flex flex-wrap items-center gap-2 text-sm">
        <span className="tabular-nums text-muted-foreground">
          {instances} {instances === 1 ? 'instance' : 'instances'} · {running} running
        </span>
      </CardContent>
    </Card>
  )
}
