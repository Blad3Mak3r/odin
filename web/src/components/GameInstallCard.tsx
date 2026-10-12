import { Link } from 'react-router-dom'
import { GameIcon } from '@/components/GameIcon'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { GAME_BANNERS } from '@/lib/games'
import { useGameInstallStatus } from '@/lib/queries'
import type { GameView } from '@/lib/types'

function InstallBadge({ game }: { game: GameView }) {
  const status = useGameInstallStatus(game.id)
  if (!status.data) return null
  if (!status.data.installed) return <Badge variant="outline">Not installed</Badge>
  return <Badge variant={status.data.update_available ? 'secondary' : 'outline'}>{status.data.update_available ? 'Update available' : 'Up to date'}</Badge>
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
        <CardDescription>Steam App ID {game.steam_app_id}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-wrap items-center gap-2 text-sm">
        <span className="tabular-nums text-muted-foreground">
          {instances} {instances === 1 ? 'instance' : 'instances'} · {running} running
        </span>
        {!game.capabilities.container_runtime && <InstallBadge game={game} />}
      </CardContent>
    </Card>
  )
}
