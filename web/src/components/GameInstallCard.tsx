import { ArrowRight } from 'lucide-react'
import { Link } from 'react-router-dom'
import { GameIcon } from '@/components/GameIcon'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import type { GameView } from '@/lib/types'

export function GameCard({ game }: { game: GameView }) {
  return (
    <Card>
      <CardHeader>
        <div>
          <CardTitle className="flex items-center gap-2 text-base"><GameIcon game={game.id} className="size-7 rounded-md" />{game.name}</CardTitle>
          <CardDescription>Steam App ID {game.steam_app_id}</CardDescription>
        </div>
      </CardHeader>
      <CardContent>
        <Button size="sm" render={<Link to={`/games/${game.id}`} />}>
          Manage
          <ArrowRight className="size-4" />
        </Button>
      </CardContent>
    </Card>
  )
}
