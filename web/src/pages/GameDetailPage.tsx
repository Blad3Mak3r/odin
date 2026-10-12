import { Navigate, useParams } from 'react-router-dom'
import { GameBackdrop } from '@/components/GameBackdrop'
import { GameIcon } from '@/components/GameIcon'
import { CreateManagedInstanceDialog } from '@/components/instance/CreateManagedInstanceDialog'
import { ManagedInstancesTable } from '@/components/instance/ManagedInstancesTable'
import { QueryError } from '@/components/QueryError'
import { RouteTabs } from '@/components/RouteTabs'
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { ValheimModsTab } from '@/pages/GlobalModsPage'
import { GAME_DESCRIPTIONS } from '@/lib/games'
import { useGames, useManagedInstances } from '@/lib/queries'
import type { GameView } from '@/lib/types'

function RuntimeCard({ game }: { game: GameView }) {
  return <Card><CardHeader><CardTitle>Per-instance runtime</CardTitle><CardDescription>{game.name} is installed and updated inside each running Podman runtime. Create an instance, start it, then use its Update action.</CardDescription></CardHeader></Card>
}

function GameInstances({ game }: { game: GameView }) {
  const instances = useManagedInstances()
  const gameInstances = instances.data?.filter((instance) => instance.game === game.id) ?? []

  return (
    <ManagedInstancesTable
      instances={gameInstances}
      isLoading={instances.isLoading}
      error={instances.isError ? instances.error : undefined}
      emptyMessage={`No ${game.name} instances yet.`}
      withGame={false}
    />
  )
}

function GameHeader({ game, games }: { game: GameView; games: GameView[] }) {
  const instances = useManagedInstances()
  const mine = instances.data?.filter((instance) => instance.game === game.id)
  const running = mine?.filter((instance) => instance.running).length ?? 0

  return (
    <header className="flex flex-col gap-4 pt-2 sm:flex-row sm:items-end sm:justify-between">
      <div className="flex items-center gap-4 sm:gap-5">
        <GameIcon game={game.id} className="size-16 sm:size-20" />
        <div>
          <h1 className="text-3xl font-semibold tracking-tight">{game.name}</h1>
          <p className="mt-2 max-w-2xl text-sm text-foreground/80">{GAME_DESCRIPTIONS[game.id]}</p>
          {mine && (
            <p className="mt-1 text-sm tabular-nums text-foreground/80">
              {mine.length} {mine.length === 1 ? 'instance' : 'instances'} · {running} running
            </p>
          )}
        </div>
      </div>
      <CreateManagedInstanceDialog games={games} defaultGame={game.id} />
    </header>
  )
}

function hasGlobalMods(game: GameView) {
  return game.id === 'valheim' && game.capabilities.mods
}

export function GameDetailPage() {
  const { game: gameParam, '*': tabPath } = useParams<{ game: string; '*': string }>()
  const games = useGames()
  const game = games.data?.find((candidate) => candidate.id === gameParam)
  const [tab, ...nestedPath] = tabPath?.split('/').filter(Boolean) ?? []
  const activeTab = tab || 'overview'

  if (games.isError) return <QueryError error={games.error} />
  if (games.isLoading) return <Skeleton className="h-56" />
  if (!game) return <Navigate replace to="/games" />
  if (tab === 'overview' && nestedPath.length === 0) return <Navigate replace to={`/games/${game.id}`} />
  if (!['overview', 'instances', 'mods'].includes(activeTab) || (nestedPath.length > 0 && activeTab !== 'mods') || (activeTab === 'mods' && !hasGlobalMods(game))) {
    return <Navigate replace to={`/games/${game.id}`} />
  }

  return (
    <div className="relative isolate flex flex-col gap-6">
      <GameBackdrop game={game.id} />
      <GameHeader game={game} games={games.data ?? []} />
      <RouteTabs
        label={`${game.name} sections`}
        tabs={[
          { label: 'Overview', to: `/games/${game.id}`, end: true },
          { label: 'Instances', to: `/games/${game.id}/instances` },
          ...(hasGlobalMods(game) ? [{ label: 'Mods', to: `/games/${game.id}/mods` }] : []),
        ]}
      />
      {activeTab === 'overview' && <RuntimeCard game={game} />}
      {activeTab === 'instances' && <GameInstances game={game} />}
      {activeTab === 'mods' && <ValheimModsTab path={nestedPath} />}
    </div>
  )
}
