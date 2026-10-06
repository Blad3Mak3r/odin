import { GameCard } from '@/components/GameInstallCard'
import { PageHeader } from '@/components/PageHeader'
import { QueryError } from '@/components/QueryError'
import { Skeleton } from '@/components/ui/skeleton'
import { useGames } from '@/lib/queries'

export function GamesPage() {
  const games = useGames()

  return (
    <div className="flex flex-col gap-6">
      <PageHeader title="Games" description="Manage game server installations." />
      {games.isError && <QueryError error={games.error} />}
      {games.isLoading && (
        <div className="grid gap-4 md:grid-cols-2">
          <Skeleton className="h-36" />
          <Skeleton className="h-36" />
        </div>
      )}
      {games.data && (
        <section className="grid gap-4 md:grid-cols-2">
          {games.data.map((game) => <GameCard key={game.id} game={game} />)}
        </section>
      )}
    </div>
  )
}
