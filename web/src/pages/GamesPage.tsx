import { GameCard } from '@/components/GameInstallCard'
import { PageHeader } from '@/components/PageHeader'
import { QueryError } from '@/components/QueryError'
import { Skeleton } from '@/components/ui/skeleton'
import { useGames, useManagedInstances } from '@/lib/queries'

const GRID = 'grid gap-4 md:grid-cols-2 xl:grid-cols-3'

export function GamesPage() {
  const games = useGames()
  const instances = useManagedInstances()

  return (
    <div className="flex flex-col gap-6">
      <PageHeader title="Games" description="Supported games, their server installs, and the instances running on this host." />
      {games.isError && <QueryError error={games.error} />}
      {games.isLoading && (
        <div className={GRID}>
          {Array.from({ length: 6 }, (_, index) => <Skeleton key={index} className="h-52" />)}
        </div>
      )}
      {games.data?.length === 0 && <p className="text-sm text-muted-foreground">This Odin build ships no game drivers.</p>}
      {games.data && games.data.length > 0 && (
        <section aria-label="Games" className={GRID}>
          {games.data.map((game) => {
            const mine = instances.data?.filter((instance) => instance.game === game.id) ?? []
            return <GameCard key={game.id} game={game} instances={mine.length} running={mine.filter((instance) => instance.running).length} />
          })}
        </section>
      )}
    </div>
  )
}
